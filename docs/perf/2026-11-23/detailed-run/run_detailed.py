#!/usr/bin/env python3
"""Detailed 2026-11-23 benchmark: chunk streaming, fixture world, forceload, 800 bots.

Runs one server at a time on 127.0.0.1:25567. JVM servers get BenchProbe.jar so TPS and
tick times are sampled by the server itself; Ferrite answers `/tps` on its console.

Usage:
  python docs/perf/2026-11-23/detailed-run/run_detailed.py --pilot
  python docs/perf/2026-11-23/detailed-run/run_detailed.py --all

Raw output goes to /tmp/ferrite-detailed-2026-11-23 (override with --dir).
"""
from __future__ import annotations

import argparse
import json
import os
import queue
import re
import resource
import shutil
import signal
import socket
import statistics
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
DEFAULT_DIR = Path("/tmp/ferrite-detailed-2026-11-23")
DEFAULT_JARS = Path("/tmp/ferrite-full-2026-11-23/jars")
PORT = 25567
BOT = ROOT / "target/release/examples/bots"
SERVER = ROOT / "target/release/ferrite-server"
FIXTURE = ROOT / "vanilla/26.3/fixture-world/world"
VARIANTS = ["ferrite", "folia", "leaf", "paper", "purpur"]
JARS = {
    "folia": "folia-26.3-26.jar",
    "leaf": "leaf-26.3-28.jar",
    "paper": "paper-26.3-259.jar",
    "purpur": "purpur-26.3-2660.jar",
}


def loadavg() -> str:
    return " ".join(Path("/proc/loadavg").read_text().split()[:3])


def cpu_ticks(pid: int) -> int:
    try:
        stat = Path(f"/proc/{pid}/stat").read_text()
        rest = stat.rsplit(")", 1)[1].split()
        return int(rest[11]) + int(rest[12])
    except (OSError, IndexError, ValueError):
        return 0


def rss_kib(pid: int) -> int:
    try:
        for line in Path(f"/proc/{pid}/status").read_text().splitlines():
            if line.startswith("VmRSS:"):
                return int(line.split()[1])
    except (OSError, ValueError):
        pass
    return 0


class BotRun:
    """A running external bot process with a line scanner."""

    def __init__(self, n: int, view: int, log: Path, walk: int = 40):
        env = os.environ.copy()
        env.update({
            "BOTS_ADDR": f"127.0.0.1:{PORT}",
            "BOTS_VIEW": str(view),
            "BOTS_STILL": "1",
            "BOTS_STAGGER_MS": "5",
            "BOTS_SETTLE_SECS": "10",
            "BOTS_WALK_SECS": str(walk),
        })
        self.log = log
        self.lines: list[str] = []
        self.cond = threading.Condition()
        self.scan = 0
        self.file = open(log, "w", encoding="utf-8")
        self.proc = subprocess.Popen(
            [str(BOT), str(n)],
            cwd=str(ROOT),
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
            errors="replace",
            bufsize=1,
        )
        self.thread = threading.Thread(target=self._read, daemon=True)
        self.thread.start()

    def _read(self) -> None:
        assert self.proc.stdout is not None
        for line in self.proc.stdout:
            line = line.rstrip("\n")
            with self.cond:
                self.lines.append(line)
                self.cond.notify_all()
            self.file.write(line + "\n")
            self.file.flush()

    def wait_for(self, needle: str, timeout: float) -> str:
        end = time.monotonic() + timeout
        with self.cond:
            while True:
                for i in range(self.scan, len(self.lines)):
                    if needle in self.lines[i]:
                        self.scan = i + 1
                        return self.lines[i]
                self.scan = len(self.lines)
                left = end - time.monotonic()
                if left <= 0:
                    raise TimeoutError(f"bot line {needle!r} not seen in {timeout}s")
                self.cond.wait(left)

    def wait(self, timeout: float) -> int:
        code = self.proc.wait(timeout=timeout)
        self.thread.join(timeout=5)
        self.file.close()
        return code

    def kill(self) -> None:
        if self.proc.poll() is None:
            self.proc.kill()
        try:
            self.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            pass
        self.thread.join(timeout=5)
        self.file.close()

    def text(self) -> str:
        return self.log.read_text(encoding="utf-8", errors="replace")


class Server:
    def __init__(self, variant: str, world: str, view: int, tag: str, run_dir: Path, jars: Path):
        self.variant = variant
        self.world = world
        self.view = view
        self.tag = tag
        self.dir = run_dir / tag
        self.jars = jars
        self.proc: subprocess.Popen[str] | None = None
        self.lines: list[str] = []
        self.cond = threading.Condition()
        self.scan = 0
        self.reader: threading.Thread | None = None
        self.sampler: threading.Thread | None = None
        self.sampler_stop = threading.Event()
        self.peak_rss = 0
        self.ready_rss = 0
        self.boot_ms = 0.0
        self.t0 = 0.0

    def prepare(self) -> None:
        if self.dir.exists():
            shutil.rmtree(self.dir)
        self.dir.mkdir(parents=True)
        if self.world == "fixture":
            shutil.copytree(
                FIXTURE,
                self.dir / "world",
                ignore=shutil.ignore_patterns("session.lock", "logs", "playerdata"),
            )
        (self.dir / "eula.txt").write_text("eula=true\n", encoding="utf-8")
        if self.variant == "ferrite":
            storage = "anvil" if self.world == "fixture" else "fworld"
            (self.dir / "ferrite.toml").write_text(
                f'''bind-address = "127.0.0.1:{PORT}"
online-mode = false
max-players = 1000
view-distance = {self.view}
compression-threshold = 256
world = "world"
generator = "rust"
storage = "{storage}"
threads = 0
lighting-pool = true
[network]
send-queue-bytes = 16777216
connection-burst = 0
connection-per-second = 0
batch-linger-ms = 2
[worldgen]
seed = "0"
''',
                encoding="utf-8",
            )
            return
        (self.dir / "server.properties").write_text(
            f'''online-mode=false
white-list=false
enforce-whitelist=false
level-seed=0
level-name=world
server-ip=127.0.0.1
server-port={PORT}
view-distance={self.view}
simulation-distance={self.view}
max-players=1000
spawn-protection=0
allow-nether=false
allow-end=false
network-compression-threshold=256
sync-chunk-writes=false
enable-rcon=false
''',
            encoding="utf-8",
        )
        (self.dir / "plugins").mkdir()
        probe = self.jars.parent / "probe" / "BenchProbe.jar"
        if probe.exists():
            shutil.copy2(probe, self.dir / "plugins" / probe.name)
            # The plugin data directory is created by the server; seed it so the
            # probe has somewhere to write before its first sample.
            (self.dir / "plugins" / "BenchProbe").mkdir(exist_ok=True)
        if self.variant == "leaf":
            loader = self.jars.parent / "leaf-loader.yml"
            if loader.exists():
                (self.dir / "config").mkdir(exist_ok=True)
                shutil.copy2(loader, self.dir / "config" / "leaf-loader.yml")
        if self.world == "fresh":
            (self.dir / "world").mkdir()

    def start(self) -> None:
        self.prepare()
        if self.variant == "ferrite":
            cmd = [str(SERVER), "--headless"]
        else:
            cmd = [
                "java", "-Xms1G", "-Xmx4G", "-jar",
                str(self.jars / JARS[self.variant]), "--nogui",
            ]
        self.t0 = time.monotonic()
        self.proc = subprocess.Popen(
            cmd,
            cwd=str(self.dir),
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
            errors="replace",
            bufsize=1,
            start_new_session=True,
        )
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()
        self.sampler = threading.Thread(target=self._sample, daemon=True)
        self.sampler.start()

    def _read(self) -> None:
        assert self.proc is not None and self.proc.stdout is not None
        with open(self.dir / "console.log", "w", encoding="utf-8") as out:
            for line in self.proc.stdout:
                line = line.rstrip("\n")
                with self.cond:
                    self.lines.append(line)
                    self.cond.notify_all()
                out.write(line + "\n")
                out.flush()

    def _sample(self) -> None:
        assert self.proc is not None
        while not self.sampler_stop.is_set() and self.proc.poll() is None:
            rss = rss_kib(self.proc.pid)
            if rss > self.peak_rss:
                self.peak_rss = rss
            self.sampler_stop.wait(0.25)

    def wait_line(self, needle: str, timeout: float) -> str:
        end = time.monotonic() + timeout
        with self.cond:
            while True:
                for i in range(self.scan, len(self.lines)):
                    if needle in self.lines[i]:
                        self.scan = i + 1
                        return self.lines[i]
                self.scan = len(self.lines)
                left = end - time.monotonic()
                if left <= 0:
                    raise TimeoutError(f"{self.tag}: line {needle!r} not seen in {timeout}s")
                self.cond.wait(left)

    def wait_any(self, needles: list[str], timeout: float) -> tuple[str, str]:
        end = time.monotonic() + timeout
        with self.cond:
            while True:
                for i in range(self.scan, len(self.lines)):
                    for needle in needles:
                        if needle in self.lines[i]:
                            self.scan = i + 1
                            return needle, self.lines[i]
                self.scan = len(self.lines)
                left = end - time.monotonic()
                if left <= 0:
                    raise TimeoutError(f"{self.tag}: none of {needles!r} seen in {timeout}s")
                self.cond.wait(left)

    def wait_ready(self, timeout: float = 180) -> None:
        assert self.proc is not None
        if self.variant == "ferrite":
            end = time.monotonic() + min(timeout, 60)
            while time.monotonic() < end:
                try:
                    with socket.create_connection(("127.0.0.1", PORT), timeout=0.2):
                        break
                except OSError:
                    if self.proc.poll() is not None:
                        raise RuntimeError(f"{self.tag}: server exited during boot")
                    time.sleep(0.1)
            else:
                raise TimeoutError(f"{self.tag}: port did not open")
            self.wait_line("listener started", 10)
            self.boot_ms = (time.monotonic() - self.t0) * 1000.0
            time.sleep(1.5)
        else:
            # Leaf (and some Paper forks) ask before migrating a vanilla world folder.
            # The console is a pipe, so answer the prompt instead of waiting forever.
            end = time.monotonic() + timeout
            confirms = 0
            while True:
                left = end - time.monotonic()
                if left <= 0:
                    raise TimeoutError(f"{self.tag}: Done ( not seen in {timeout}s")
                try:
                    needle, _ = self.wait_any(["Done (", "Type 'confirm' to continue"], left)
                except TimeoutError as error:
                    raise TimeoutError(f"{self.tag}: Done ( not seen in {timeout}s") from error
                if needle == "Done (":
                    break
                if confirms >= 3:
                    raise RuntimeError(f"{self.tag}: repeated world-migration prompt")
                self.send("confirm")
                confirms += 1
            self.boot_ms = (time.monotonic() - self.t0) * 1000.0
        time.sleep(1.0)
        self.ready_rss = rss_kib(self.proc.pid)

    def send(self, command: str) -> None:
        if self.proc is None or self.proc.stdin is None or self.proc.poll() is not None:
            raise RuntimeError(f"{self.tag}: server is not running")
        self.proc.stdin.write(command + "\n")
        self.proc.stdin.flush()

    def stop(self, timeout: float = 30) -> None:
        if self.proc is None:
            return
        if self.proc.poll() is None:
            try:
                self.send("stop")
            except RuntimeError:
                pass
            try:
                self.proc.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(os.getpgid(self.proc.pid), signal.SIGTERM)
                except OSError:
                    pass
                try:
                    self.proc.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    try:
                        os.killpg(os.getpgid(self.proc.pid), signal.SIGKILL)
                    except OSError:
                        pass
                    self.proc.wait(timeout=5)
        self.sampler_stop.set()
        if self.sampler:
            self.sampler.join(timeout=5)
        if self.reader:
            self.reader.join(timeout=5)

    def console_tail(self, n: int = 80) -> str:
        return "\n".join(self.lines[-n:])

    def tps(self) -> tuple[float, float, float]:
        self.scan = len(self.lines)
        self.send("tps")
        line = self.wait_line("Region 0 TPS from last 5s, 1m, 5m:", 15)
        m = re.search(r"Region 0 TPS from last 5s, 1m, 5m: ([\d.]+), ([\d.]+), ([\d.]+)", line)
        if not m:
            raise RuntimeError(f"{self.tag}: cannot parse {line!r}")
        return float(m.group(1)), float(m.group(2)), float(m.group(3))


def run_chunks(view: int, tag: str, run_dir: Path) -> dict:
    env = os.environ.copy()
    env.update({
        "BOTS_ADDR": f"127.0.0.1:{PORT}",
        "BOTS_VIEW": str(view),
        "BOTS_CHUNKS": "1",
    })
    log = run_dir / tag / "bots.log"
    t0 = time.monotonic()
    proc = subprocess.run(
        [str(BOT), "1"],
        cwd=str(ROOT),
        env=env,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=240,
    )
    wall = time.monotonic() - t0
    log.write_text(proc.stdout + proc.stderr, encoding="utf-8")
    m = re.search(r"CHUNKS (\d+) ([\d.]+) ([\d.]+)", proc.stdout)
    if not m:
        raise RuntimeError(f"{tag}: no CHUNKS line: {proc.stdout[-400:]}")
    return {
        "count": int(m.group(1)),
        "seconds": float(m.group(2)),
        "rate": float(m.group(3)),
        "wall_s": wall,
    }


def forceload(server: Server, cycles: int = 20) -> dict:
    """Time forceload add/remove command dispatch under the current load.

    A `/seed` marker is queued after each command; the console processes commands in order,
    so the time to the marker includes the command's dispatch on the server thread. It does
    not wait for every chunk to finish loading (same definition as the earlier Ferrite
    `/forceload` figures). `/seed` is used instead of `/say` because Ferrite does not echo
    console `say` broadcasts back to the console.
    """
    server.scan = len(server.lines)
    server.send("seed")
    try:
        server.wait_line("Seed:", 6)
    except TimeoutError:
        return {"supported": False, "add_ms": [], "remove_ms": [], "errors": ["seed marker unavailable"]}
    add: list[float] = []
    remove: list[float] = []
    errors: list[str] = []
    for cycle in range(cycles):
        before = len(server.lines)
        t = time.monotonic()
        server.send("forceload add -48 -48 207 207")
        server.send("seed")
        try:
            server.wait_line("Seed:", 15)
        except TimeoutError:
            errors.append(f"add timeout cycle {cycle}")
            break
        add.append((time.monotonic() - t) * 1000.0)
        text = "\n".join(server.lines[before:])
        if "Unknown command" in text or "unknown command" in text:
            errors.append(f"add failed cycle {cycle}")
            break
        before = len(server.lines)
        t = time.monotonic()
        server.send("forceload remove all")
        server.send("seed")
        try:
            server.wait_line("Seed:", 15)
        except TimeoutError:
            errors.append(f"remove timeout cycle {cycle}")
            break
        remove.append((time.monotonic() - t) * 1000.0)
        time.sleep(0.2)
    return {"supported": True, "add_ms": add, "remove_ms": remove, "errors": errors}


def parse_bots(text: str) -> dict:
    join = re.search(
        r"external bot join time: n=(\d+), p50=([\d.]+) ms, p90=([\d.]+) ms, "
        r"p99=([\d.]+) ms, max=([\d.]+) ms",
        text,
    )
    done = re.search(
        r"DONE: bots=(\d+), elapsed_s=([\d.]+), throughput=([\d.]+) MiB/s "
        r"\(([\d.]+) KiB/s per bot\)",
        text,
    )
    return {
        "join_p50": float(join.group(2)) if join else None,
        "join_p99": float(join.group(4)) if join else None,
        "join_max": float(join.group(5)) if join else None,
        "elapsed_s": float(done.group(2)) if done else None,
        "throughput_mib_s": float(done.group(3)) if done else None,
        "throughput_kib_bot_s": float(done.group(4)) if done else None,
    }


def jvm_ticks(path: Path, bots: int) -> dict:
    rows = []
    if path.exists():
        for line in path.read_text(encoding="utf-8", errors="replace").splitlines()[1:]:
            parts = line.split(",")
            if len(parts) != 6:
                continue
            try:
                rows.append({
                    "mean": float(parts[1]),
                    "p99": float(parts[2]),
                    "max": float(parts[3]),
                    "tps": float(parts[4]),
                    "players": int(parts[5]),
                })
            except ValueError:
                pass
    live = [r for r in rows if r["players"] == bots]
    tail = live[-10:]
    return {
        "probe_rows": len(rows),
        "tps": live[-1]["tps"] if live else None,
        "tick_mean": statistics.mean(r["mean"] for r in tail) if tail else None,
        "tick_p99": statistics.mean(r["p99"] for r in tail) if tail else None,
        "tick_max": max((r["max"] for r in tail), default=None),
        "players_seen": max((r["players"] for r in rows), default=0),
    }


def write_meta(server: Server, meta: dict, run_dir: Path) -> None:
    directory = run_dir / server.tag
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "meta.json").write_text(json.dumps(meta, indent=2) + "\n", encoding="utf-8")
    lines = [f"{k}={v}" for k, v in meta.items() if not isinstance(v, (list, dict))]
    lines.append("\n--- console tail ---")
    lines.append(server.console_tail())
    (directory / "meta.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")


def schedule(run_dir: Path, message: str) -> None:
    with open(run_dir / "schedule.log", "a", encoding="utf-8") as f:
        f.write(f"{time.strftime('%Y-%m-%dT%H:%M:%S')} {message} load={loadavg()}\n")


def chunk_test(variant: str, world: str, view: int, repeat: int, run_dir: Path, jars: Path) -> dict:
    tag = f"{variant}-{world}-v{view}-r{repeat}"
    schedule(run_dir, f"START {tag}")
    s = Server(variant, world, view, tag, run_dir / "runs", jars)
    meta = {
        "kind": "chunk_stream",
        "variant": variant,
        "world": world,
        "view": view,
        "repeat": repeat,
        "boot_ms": None,
        "idle_rss_mib": None,
        "peak_rss_mib": None,
        "cpu_pct": None,
        "count": None,
        "seconds": None,
        "rate": None,
        "wall_s": None,
        "error": None,
    }
    try:
        s.start()
        s.wait_ready()
        s.scan = len(s.lines)
        cpu0 = cpu_ticks(s.proc.pid)
        res = run_chunks(view, tag, run_dir / "runs")
        cpu1 = cpu_ticks(s.proc.pid)
        meta.update({
            "boot_ms": round(s.boot_ms, 1),
            "idle_rss_mib": round(s.ready_rss / 1024.0, 1),
            "peak_rss_mib": round(s.peak_rss / 1024.0, 1),
            "cpu_pct": round((cpu1 - cpu0) / res["wall_s"], 1),
            **res,
        })
    except Exception as error:
        meta["error"] = f"{type(error).__name__}: {error}"
        meta["console_tail"] = s.console_tail(40)
    finally:
        s.stop()
    meta["load_end"] = loadavg()
    write_meta(s, meta, run_dir / "runs")
    schedule(run_dir, f"DONE {tag} chunks={meta.get('count')} rate={meta.get('rate')} error={meta.get('error')}")
    return meta


def load_test(
    variant: str,
    world: str,
    bots: int,
    do_forceload: bool,
    run_dir: Path,
    jars: Path,
) -> dict:
    tag = f"{variant}-{world}-{bots}"
    schedule(run_dir, f"START {tag}")
    s = Server(variant, world, 10, tag, run_dir / "runs", jars)
    b: BotRun | None = None
    meta = {
        "kind": "load",
        "variant": variant,
        "world": world,
        "bots": bots,
        "view": 10,
        "bot_status": None,
        "boot_ms": None,
        "idle_rss_mib": None,
        "peak_rss_mib": None,
        "cpu_pct": None,
        "wall_s": None,
        "tps_5s": None,
        "tps_1m": None,
        "tps_5m": None,
        "join_p50": None,
        "join_p99": None,
        "join_max": None,
        "throughput_mib_s": None,
        "throughput_kib_bot_s": None,
        "forceload_supported": None,
        "forceload_add_ms": [],
        "forceload_remove_ms": [],
        "forceload_errors": [],
        "error": None,
    }
    try:
        s.start()
        s.wait_ready()
        s.scan = len(s.lines)
        cpu0 = cpu_ticks(s.proc.pid)
        t0 = time.monotonic()
        b = BotRun(bots, 10, run_dir / "runs" / tag / "bots.log", walk=40)
        b.wait_for("WALKING", 300)
        walk_start = time.monotonic()
        fl = forceload(s) if do_forceload else None
        tps = None
        if variant == "ferrite":
            # Sample while every bot is still online. The walk is 40 s, so 32 s leaves
            # the command and its 1-minute TPS window inside the measured load.
            due = walk_start + 32.0
            if time.monotonic() < due:
                time.sleep(due - time.monotonic())
            tps = s.tps()
        try:
            code = b.wait(180)
        except subprocess.TimeoutExpired:
            code = -9
            meta["error"] = "bot walk timeout"
            b.kill()
        wall = time.monotonic() - t0
        cpu1 = cpu_ticks(s.proc.pid)
        meta.update({
            "bot_status": code,
            "boot_ms": round(s.boot_ms, 1),
            "idle_rss_mib": round(s.ready_rss / 1024.0, 1),
            "peak_rss_mib": round(s.peak_rss / 1024.0, 1),
            "cpu_pct": round((cpu1 - cpu0) / wall, 1),
            "wall_s": round(wall, 1),
            "walk_s": round(time.monotonic() - walk_start, 1),
            **parse_bots(b.text()),
        })
        if tps is not None:
            meta.update({"tps_5s": tps[0], "tps_1m": tps[1], "tps_5m": tps[2]})
        if fl is not None:
            meta.update({
                "forceload_supported": fl["supported"],
                "forceload_add_ms": fl["add_ms"],
                "forceload_remove_ms": fl["remove_ms"],
                "forceload_errors": fl["errors"],
            })
    except Exception as error:
        meta["error"] = f"{type(error).__name__}: {error}"
        meta["console_tail"] = s.console_tail(40)
    finally:
        if b is not None:
            try:
                b.kill()
            except Exception:
                pass
        s.stop()
    if variant != "ferrite":
        ticks = jvm_ticks(s.dir / "plugins" / "BenchProbe" / "ticks.csv", bots)
        meta.update(ticks)
        meta["tps_1m"] = ticks.get("tps")
        csv = s.dir / "plugins" / "BenchProbe" / "ticks.csv"
        if csv.exists():
            shutil.copy2(csv, run_dir / "runs" / tag / "ticks.csv")
    meta["load_end"] = loadavg()
    write_meta(s, meta, run_dir / "runs")
    schedule(
        run_dir,
        f"DONE {tag} status={meta.get('bot_status')} tps={meta.get('tps_1m')} "
        f"rss={meta.get('peak_rss_mib')} fl_add={len(meta.get('forceload_add_ms') or [])} "
        f"error={meta.get('error')}",
    )
    return meta


def pilot(run_dir: Path, jars: Path) -> None:
    for variant in ("ferrite", "paper"):
        chunk_test(variant, "fixture", 10, 0, run_dir, jars)
        load_test(variant, "fixture", 20, True, run_dir, jars)


def leaf_tests(run_dir: Path, jars: Path) -> None:
    """Re-run only the Leaf fixture points after the migration-confirm fix."""
    for repeat in (0, 1):
        chunk_test("leaf", "fixture", 10, repeat, run_dir, jars)
    load_test("leaf", "fixture", 200, True, run_dir, jars)


def all_tests(run_dir: Path, jars: Path) -> None:
    for view in (6, 10, 16):
        for variant in VARIANTS:
            for repeat in (0, 1):
                chunk_test(variant, "fresh", view, repeat, run_dir, jars)
    for variant in VARIANTS:
        for repeat in (0, 1):
            chunk_test(variant, "fixture", 10, repeat, run_dir, jars)
    for variant in VARIANTS:
        load_test(variant, "fixture", 200, True, run_dir, jars)
    load_test("ferrite", "fixture", 800, False, run_dir, jars)
    load_test("ferrite", "fresh", 800, False, run_dir, jars)
    flush = ROOT / "target/release/examples/flush_bench"
    if flush.exists():
        proc = subprocess.run(
            [str(flush)], cwd=str(ROOT), capture_output=True, text=True,
            encoding="utf-8", errors="replace", timeout=600,
        )
        (run_dir / "results" / "flush_bench.txt").write_text(
            proc.stdout + proc.stderr, encoding="utf-8"
        )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dir", type=Path, default=DEFAULT_DIR)
    parser.add_argument("--jars", type=Path, default=DEFAULT_JARS)
    parser.add_argument("--pilot", action="store_true")
    parser.add_argument("--all", action="store_true")
    parser.add_argument("--leaf-only", action="store_true")
    args = parser.parse_args()
    soft, hard = resource.getrlimit(resource.RLIMIT_NOFILE)
    resource.setrlimit(resource.RLIMIT_NOFILE, (min(65535, hard), hard))
    run_dir = args.dir
    (run_dir / "results").mkdir(parents=True, exist_ok=True)
    (run_dir / "runs").mkdir(parents=True, exist_ok=True)
    schedule(run_dir, f"RUN start nproc={os.cpu_count()} ulimit_nofile={soft}")
    try:
        if args.pilot:
            pilot(run_dir, args.jars)
        elif args.all:
            all_tests(run_dir, args.jars)
        elif args.leaf_only:
            leaf_tests(run_dir, args.jars)
        else:
            parser.error("choose --pilot, --all or --leaf-only")
    finally:
        schedule(run_dir, "RUN end")


if __name__ == "__main__":
    main()
