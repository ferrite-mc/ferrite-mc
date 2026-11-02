"""Generate the vanilla fixture world used by world/light/server tests.

Usage:  python tools/make_fixture_world.py [version]   (run tools/fetch_vanilla.py first)

Starts the official server jar twice with a fixed seed:

1. force-loads chunks (-3..18, -3..18) and saves repeatedly until the region files stop
   changing;
2. clears the `isLightOn` flag of every saved chunk and force-loads the area again, so
   vanilla recomputes light from the stored blocks and writes a self-consistent world.

Output (gitignored):
  vanilla/<version>/fixture-world/world/...   (level.dat, region files)

Chunk generation is asynchronous and a feature in a neighbouring chunk (for example a
tree overhanging from the next chunk) can modify a chunk after it was written, leaving
stale sky light in the saved file. The first pass waits for two identical flushes instead
of a fixed delay, but a chunk that a neighbour touched after its only save is never
rewritten, so its stored light stays stale. The second pass removes that remaining
staleness.

Tests use the inner chunks (0..15, 0..15). The three-chunk margin keeps their light
comparable: vanilla can store stale light for a chunk near the edge of generation when a
later neighbour places blocks (for example a tree overhanging from the next chunk).
Only the inner chunks are compared; the margin itself may hold stale light.

The seed and settings are fixed so tests that assert on this world (spawn position,
stored chunks) are reproducible on every machine.
"""

import gzip
import hashlib
import shutil
import struct
import subprocess
import sys
import threading
import time
import zlib

from fetch_vanilla import ROOT, default_version

SEED = "ferrite-fixture"
# Flush, then flush again and compare: equal region files mean every chunk is
# generated, lit and stable. The timeout only guards against a stuck server.
GENERATE_POLL_SECONDS = 10
GENERATE_MAX_SECONDS = 300
# Chunk loading and light initialisation are asynchronous; give the relight pass time to
# finish before the first snapshot is written.
RELIGHT_SETTLE_SECONDS = 15
SPAWN = (0, 80, 112)  # asserted by ferrite-server's fixture tests
# Tests compare chunks 0..15 with a three-chunk halo, so -3..18 must all be generated.
# forceload allows at most 256 chunks per command, so the area is force-loaded in
# z-strips.
FORCELOAD_COMMANDS = (
    "forceload add -48 -48 303 79",  # chunks x -3..18, z -3..4
    "forceload add -48 80 303 207",  # chunks x -3..18, z 5..12
    "forceload add -48 208 303 303",  # chunks x -3..18, z 13..18
)
PROPERTIES = f"""level-name=world
level-seed={SEED}
online-mode=false
server-port=25599
enable-rcon=false
spawn-protection=0
view-distance=4
simulation-distance=4
"""
REGION_DIR = "world/dimensions/minecraft/overworld/region"
SECTOR = 4096
# Chunks the two passes force-load; -3..18 matches FORCELOAD_COMMANDS. Chunks outside it
# (the server also generates a spawn area) are never reloaded by the relight pass, so the
# fixture leaves them untouched.
FORCED_MIN = -3
FORCED_MAX = 18
# `isLightOn: 1b` as serialized in a chunk root: a byte tag named isLightOn. A byte tag
# keeps its value in the last byte, so flipping it marks the chunk as unlit without
# touching the rest of the chunk tree.
LIGHT_ON = b"\x01\x00\x09isLightOn\x01"
LIGHT_OFF = LIGHT_ON[:-1] + b"\x00"


def region_digest(out) -> str:
    """Hash the region files; a changing digest means chunks are still generating."""
    digest = hashlib.sha256()
    for path in sorted((out / REGION_DIR).glob("*.mca")):
        digest.update(path.name.encode())
        digest.update(path.read_bytes())
    return digest.hexdigest()


def decompress(kind: int, blob: bytes) -> bytes:
    if kind == 1:
        return gzip.decompress(blob)
    if kind == 2:
        return zlib.decompress(blob)
    if kind == 3:
        return blob
    sys.exit(f"unsupported chunk compression {kind}")


def read_region(path):
    """Return {chunk index: (timestamp, uncompressed NBT)} for one region file."""
    data = path.read_bytes()
    chunks = {}
    for index in range(1024):
        entry = data[index * 4 : index * 4 + 4]
        if entry[:3] == b"\0\0\0":
            continue
        if entry[3] == 255:
            sys.exit(f"{path}: external chunk payloads are unsupported")
        start = int.from_bytes(entry[:3], "big") * SECTOR
        length = int.from_bytes(data[start : start + 4], "big")
        timestamp = int.from_bytes(
            data[SECTOR + index * 4 : SECTOR + index * 4 + 4], "big"
        )
        blob = data[start + 5 : start + 4 + length]
        chunks[index] = (timestamp, decompress(data[start + 4], blob))
    return chunks


def write_region(path, chunks) -> None:
    """Rewrite a region file, recompressing the given chunks."""
    out = bytearray(2 * SECTOR)
    for index, (timestamp, raw) in sorted(chunks.items()):
        payload = zlib.compress(raw, 6)
        record = struct.pack(">I", len(payload) + 1) + b"\x02" + payload
        sectors = (len(record) + SECTOR - 1) // SECTOR
        if sectors >= 255:
            sys.exit(f"{path}: chunk {index} needs {sectors} sectors")
        offset = len(out) // SECTOR
        out[index * 4 : index * 4 + 3] = offset.to_bytes(3, "big")
        out[index * 4 + 3] = sectors
        out[SECTOR + index * 4 : SECTOR + index * 4 + 4] = timestamp.to_bytes(4, "big")
        out.extend(record)
        out.extend(b"\0" * (sectors * SECTOR - len(record)))
    path.write_bytes(out)


def region_chunks(path):
    """Yield (x, z, timestamp, uncompressed NBT) for one region file."""
    rx, rz = (int(part) for part in path.stem.removeprefix("r.").split("."))
    for index, (timestamp, raw) in read_region(path).items():
        yield rx * 32 + index % 32, rz * 32 + index // 32, timestamp, raw


def mark_chunks_unlit(out) -> int:
    """Clear `isLightOn` in the forced chunks so vanilla relights them on the next load."""
    marked = 0
    for path in sorted((out / REGION_DIR).glob("*.mca")):
        chunks = read_region(path)
        rx, rz = (int(part) for part in path.stem.removeprefix("r.").split("."))
        for index, (timestamp, raw) in chunks.items():
            x = rx * 32 + index % 32
            z = rz * 32 + index // 32
            if not (FORCED_MIN <= x <= FORCED_MAX and FORCED_MIN <= z <= FORCED_MAX):
                continue
            found = raw.count(LIGHT_ON)
            if found != 1:
                sys.exit(f"{path}: forced chunk {x},{z} has {found} isLightOn flags")
            chunks[index] = (timestamp, raw.replace(LIGHT_ON, LIGHT_OFF))
            marked += 1
        write_region(path, chunks)
    return marked


def unlit_chunks(out) -> int:
    """Count forced chunks whose light was not recomputed by the relight pass."""
    return sum(
        raw.count(LIGHT_OFF)
        for path in sorted((out / REGION_DIR).glob("*.mca"))
        for x, z, _, raw in region_chunks(path)
        if FORCED_MIN <= x <= FORCED_MAX and FORCED_MIN <= z <= FORCED_MAX
    )


def run_server(jar, out, *, pin_spawn: bool, settle_seconds: int) -> None:
    """Run the server until two consecutive saves produce identical region files."""
    proc = subprocess.Popen(
        ["java", "-Xmx2G", "-jar", str(jar), "nogui"],
        cwd=out,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
    )

    send_lock = threading.Lock()

    def send(command: str) -> None:
        with send_lock:
            proc.stdin.write(command + "\n")
            proc.stdin.flush()

    def flush() -> None:
        send("save-all flush")

    started = time.monotonic()
    last_digest = None
    saving = False
    for line in proc.stdout:
        if "Done (" in line:
            # Tests read chunk (0, 0) / r.0.0.mca, which spawn may not cover.
            # forceload allows at most 256 chunks per command.
            for command in FORCELOAD_COMMANDS:
                send(command)
            if pin_spawn:
                # Tests join at spawn and expect chunk (0, 7) first; pin it inside
                # the forced area.
                send(f"setworldspawn {SPAWN[0]} {SPAWN[1]} {SPAWN[2]}")
            saving = True
            threading.Timer(settle_seconds, flush).start()
        elif "Saved the game" in line and saving:
            digest = region_digest(out)
            elapsed = time.monotonic() - started
            if digest == last_digest:
                print(f"ok: world stable after {elapsed:.0f} s", flush=True)
                saving = False
                send("stop")
            elif elapsed >= GENERATE_MAX_SECONDS:
                print(
                    f"warning: world still changing after {elapsed:.0f} s; "
                    "fixture light may be stale",
                    file=sys.stderr,
                )
                saving = False
                send("stop")
            else:
                last_digest = digest
                threading.Timer(GENERATE_POLL_SECONDS, flush).start()
        elif "Unable to" in line or "Too many chunks" in line:
            send("stop")
            sys.exit(f"forceload failed: {line.strip()}")
    if proc.wait() != 0:
        sys.exit(f"vanilla server exited with {proc.returncode}")


def main() -> None:
    version = sys.argv[1] if len(sys.argv) > 1 else default_version()
    base = ROOT / "vanilla" / version
    jar = base / "server.jar"
    if not jar.exists():
        sys.exit(f"missing {jar}; run python tools/fetch_vanilla.py first")
    out = base / "fixture-world"
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    (out / "eula.txt").write_text("eula=true\n", encoding="utf-8")
    (out / "server.properties").write_text(PROPERTIES, encoding="utf-8")

    print("pass 1/2: generate", flush=True)
    run_server(jar, out, pin_spawn=True, settle_seconds=0)

    marked = mark_chunks_unlit(out)
    if marked == 0:
        sys.exit("generated world has no chunks to relight")
    print(f"pass 2/2: relight {marked} chunk(s)", flush=True)
    run_server(jar, out, pin_spawn=False, settle_seconds=RELIGHT_SETTLE_SECONDS)

    stale = unlit_chunks(out)
    if stale:
        sys.exit(f"{stale} chunk(s) are still marked unlit after the relight pass")
    print(f"ok: {out / 'world'} (seed {SEED!r})")


if __name__ == "__main__":
    main()
