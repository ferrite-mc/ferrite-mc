#!/usr/bin/env python3
"""Parse the full benchmark results and render SVG charts."""
import json
import os
import re
import statistics

D = "/tmp/ferrite-full-2026-11-23/results"
SERVERS = ["ferrite", "folia", "leaf", "paper", "purpur"]
COLORS = {
    "ferrite": "#f97316",
    "folia": "#38bdf8",
    "leaf": "#34d399",
    "paper": "#a78bfa",
    "purpur": "#f472b6",
}
BOTS = [50, 100, 200, 400]


def read(path):
    try:
        with open(path, "r", encoding="utf-8", errors="replace") as f:
            return f.read()
    except OSError:
        return ""


def ferrite(label):
    t = read(os.path.join(D, f"ferrite-{label}.meta.txt"))
    m = re.search(
        r"FERRITE_RESULT label=(\S+) bots=(\d+) bot_status=(-?\d+) start_ms=(\d+) "
        r"end_ms=(\d+) elapsed_ms=(\d+) maxrss_kb=(\d+) cpu_ticks=(\d+)", t)
    if not m:
        return None
    join = re.search(
        r"external bot join time: n=(\d+), p50=([\d.]+) ms, p90=([\d.]+) ms, "
        r"p99=([\d.]+) ms, max=([\d.]+) ms", t)
    tps = re.search(
        r"Region 0 TPS from last 5s, 1m, 5m: ([\d.]+), ([\d.]+), ([\d.]+)", t)
    profile = {}
    for line in t.splitlines():
        p = re.search(
            r"([\d.]+) ms self\s+[\d.]+ ms total\s+(\d+) calls\s+([\d.]+) ms/call\s+(\S+)", line)
        if p:
            profile[p.group(4)] = float(p.group(3))
    elapsed = int(m.group(6))
    return {
        "server": "ferrite",
        "bots": int(m.group(2)),
        "status": int(m.group(3)),
        "join_p50": float(join.group(2)) if join else None,
        "join_p99": float(join.group(4)) if join else None,
        "join_max": float(join.group(5)) if join else None,
        "tps": float(tps.group(2)) if tps else None,
        "elapsed_ms": elapsed,
        "rss_mib": int(m.group(7)) / 1024.0,
        "cpu_pct": int(m.group(8)) * 1000.0 / elapsed,
        "profile": profile,
    }


def jvm(server, bots):
    t = read(os.path.join(D, f"{server}-{bots}.meta.txt"))
    m = re.search(
        r"JVM_RESULT variant=(\S+) bots=(\d+) bot_status=(-?\d+) start_ms=(\d+) "
        r"end_ms=(\d+) elapsed_ms=(\d+) maxrss_kb=(\d+) cpu_ticks=(\d+)", t)
    if not m:
        return None
    join = re.search(
        r"external bot join time: n=(\d+), p50=([\d.]+) ms, p90=([\d.]+) ms, "
        r"p99=([\d.]+) ms, max=([\d.]+) ms", t)
    rows = []
    csv = read(os.path.join(D, f"{server}-{bots}.ticks.csv"))
    for line in csv.splitlines()[1:]:
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
            continue
    live = [r for r in rows if r["players"] == bots]
    tail = live[-10:]
    elapsed = int(m.group(6))
    return {
        "server": server,
        "bots": int(m.group(2)),
        "status": int(m.group(3)),
        "join_p50": float(join.group(2)) if join else None,
        "join_p99": float(join.group(4)) if join else None,
        "join_max": float(join.group(5)) if join else None,
        "tps": live[-1]["tps"] if live else None,
        "tick_mean": statistics.mean(r["mean"] for r in tail) if tail else None,
        "tick_p99": statistics.mean(r["p99"] for r in tail) if tail else None,
        "tick_max": max((r["max"] for r in tail), default=None),
        "players_seen": max((r["players"] for r in rows), default=0),
        "elapsed_ms": elapsed,
        "rss_mib": int(m.group(7)) / 1024.0,
        "cpu_pct": int(m.group(8)) * 1000.0 / elapsed,
    }


def collect():
    data = {}
    for n in BOTS:
        data[f"ferrite-{n}"] = ferrite(str(n))
    data["ferrite-400-long"] = ferrite("400-long")
    for s in SERVERS[1:]:
        for n in BOTS:
            data[f"{s}-{n}"] = jvm(s, n)
    return {k: v for k, v in data.items() if v}


def fmt(v, nd=1):
    if v is None:
        return "—"
    if abs(v) >= 1000:
        return f"{v:,.0f}"
    return f"{v:.{nd}f}"


def esc(s):
    return (str(s).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;"))


def bar_panel(x, y, w, h, title, subtitle, values, scale="linear", unit="", ref=None,
              note=None, digits=1, max_override=None):
    out = [f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="10" fill="#161b22" stroke="#30363d"/>']
    out.append(f'<text x="{x+18}" y="{y+26}" fill="#e6edf3" font-family="ui-sans-serif,system-ui,sans-serif" font-size="13.5" font-weight="700">{esc(title)}</text>')
    out.append(f'<text x="{x+18}" y="{y+44}" fill="#8b949e" font-family="ui-sans-serif,system-ui,sans-serif" font-size="10.5">{esc(subtitle)}</text>')
    left, right = x + 54, x + w - 16
    top, bottom = y + 66, y + h - 48
    out.append(f'<line x1="{left}" y1="{bottom}" x2="{right}" y2="{bottom}" stroke="#30363d"/>')

    def sc(v):
        if scale == "log":
            import math
            v = max(v, 1.0)
            lo, hi = 10.0, max(max_override or 10000.0, v * 1.2)
            f = (math.log10(v) - math.log10(lo)) / (math.log10(hi) - math.log10(lo))
        else:
            hi = max_override or max((v for v in values.values() if v), default=1) * 1.15
            hi = max(hi, 1e-9)
            f = v / hi
        return bottom - min(f, 1.0) * (bottom - top)

    # grid lines
    if scale == "log":
        import math
        for p in range(1, 5):
            v = 10 ** p
            if v <= (max_override or 10000):
                yy = sc(v)
                out.append(f'<line x1="{left}" y1="{yy:.1f}" x2="{right}" y2="{yy:.1f}" stroke="#21262d" stroke-dasharray="3,3"/>')
                out.append(f'<text x="{left-7}" y="{yy+3:.1f}" text-anchor="end" fill="#6e7681" font-family="ui-monospace,monospace" font-size="8.5">{v:,}</text>')
    else:
        hi = max_override or max((v for v in values.values() if v), default=1) * 1.15
        for i in range(1, 4):
            v = hi * i / 4
            yy = sc(v)
            out.append(f'<line x1="{left}" y1="{yy:.1f}" x2="{right}" y2="{yy:.1f}" stroke="#21262d" stroke-dasharray="3,3"/>')
            out.append(f'<text x="{left-7}" y="{yy+3:.1f}" text-anchor="end" fill="#6e7681" font-family="ui-monospace,monospace" font-size="8.5">{fmt(v,0)}</text>')

    if ref is not None:
        yy = sc(ref)
        out.append(f'<line x1="{left}" y1="{yy:.1f}" x2="{right}" y2="{yy:.1f}" stroke="#f97316" stroke-dasharray="4,3" opacity="0.55"/>')
        out.append(f'<text x="{right}" y="{yy-4:.1f}" text-anchor="end" fill="#f97316" font-family="ui-monospace,monospace" font-size="8.5">{esc(str(ref))}{esc(unit)}</text>')

    groups = len(BOTS)
    span = (right - left) / groups
    bar_w = min(15.0, span / 7.2)
    for gi, n in enumerate(BOTS):
        cx = left + span * (gi + 0.5)
        out.append(f'<text x="{cx:.1f}" y="{bottom+22}" text-anchor="middle" fill="#c9d1d9" font-family="ui-sans-serif,system-ui,sans-serif" font-size="10">{n} bots</text>')
        for si, s in enumerate(SERVERS):
            v = values.get((s, n))
            bx = cx - (len(SERVERS) * bar_w + 2 * (len(SERVERS) - 1)) / 2 + si * (bar_w + 2)
            if v is None:
                if s == "ferrite":
                    continue
                out.append(f'<text x="{bx+bar_w/2:.1f}" y="{bottom-6}" text-anchor="middle" fill="#f87171" font-family="ui-sans-serif,system-ui,sans-serif" font-size="8" font-weight="700">x</text>')
                continue
            yy = sc(v)
            out.append(f'<rect x="{bx:.1f}" y="{yy:.1f}" width="{bar_w:.1f}" height="{bottom-yy:.1f}" rx="2.5" fill="{COLORS[s]}"/>')
            label = fmt(v, 0 if v >= 100 else digits)
            if scale == "log":
                out.append(f'<text x="{bx+bar_w/2:.1f}" y="{yy-3:.1f}" text-anchor="middle" fill="#e6edf3" font-family="ui-monospace,monospace" font-size="7.3">{label}</text>')
            else:
                out.append(f'<text x="{bx+bar_w/2:.1f}" y="{yy-3:.1f}" text-anchor="middle" fill="#e6edf3" font-family="ui-monospace,monospace" font-size="7.3">{label}</text>')
    if note:
        out.append(f'<text x="{x+18}" y="{y+h-12}" fill="#8b949e" font-family="ui-sans-serif,system-ui,sans-serif" font-size="8.6">{esc(note)}</text>')
    return out


def legend(y):
    out = []
    x = 640
    for s in SERVERS:
        out.append(f'<rect x="{x}" y="{y}" width="9" height="9" rx="2" fill="{COLORS[s]}"/>')
        out.append(f'<text x="{x+14}" y="{y+8}" fill="#c9d1d9" font-family="ui-sans-serif,system-ui,sans-serif" font-size="10">{s.capitalize()}</text>')
        x += 62
    return out


def svg_full(data, out_path):
    w, h = 1100, 820
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">',
        f'<rect width="{w}" height="{h}" rx="14" fill="#0d1117"/>',
        '<text x="28" y="34" fill="#e6edf3" font-family="ui-sans-serif,system-ui,sans-serif" font-size="20" font-weight="800">Ferrite vs Folia · Leaf · Paper · Purpur 26.3 — fresh seed=0 (2026-11-23)</text>',
        '<text x="28" y="55" fill="#8b949e" font-family="ui-sans-serif,system-ui,sans-serif" font-size="11.5">100 / 200 / 400 bots · BOTS_STILL=1 · view=10 · 5 ms stagger · 10 s settle + 30 s walk · sequential, one server at a time</text>',
    ]
    parts += legend(42)

    def col(s, field):
        return {(s, n): (data.get(f"{s}-{n}") or {}).get(field) for n in BOTS}

    parts += bar_panel(28, 78, 512, 344, "Server-reported TPS", "higher is better · 1-minute figure including join storm · target 20",
                       col("ferrite", "tps") | {(s, n): (data.get(f"{s}-{n}") or {}).get("tps") for s in SERVERS for n in BOTS},
                       max_override=20, ref=20, unit=" TPS", digits=1)
    parts += bar_panel(560, 78, 512, 344, "Peak RSS", "lower is better · server process only · MiB",
                       col("ferrite", "rss_mib") | {(s, n): (data.get(f"{s}-{n}") or {}).get("rss_mib") for s in SERVERS for n in BOTS},
                       digits=0, note="JVM: -Xms1G -Xmx4G · Ferrite: release, threads=0, lighting-pool")
    parts += bar_panel(28, 444, 512, 344, "Join latency, p50", "lower is better · log scale · ms",
                       col("ferrite", "join_p50") | {(s, n): (data.get(f"{s}-{n}") or {}).get("join_p50") for s in SERVERS for n in BOTS},
                       scale="log", max_override=100000, digits=0,
                       note="Folia 400: connection closed before all bots joined" if not data.get("folia-400") or (data.get("folia-400") or {}).get("players_seen", 0) < 400 else None)
    parts += bar_panel(560, 444, 512, 344, "CPU usage", "mean process CPU over the bot run · 1200% = 12 vCPU",
                       col("ferrite", "cpu_pct") | {(s, n): (data.get(f"{s}-{n}") or {}).get("cpu_pct") for s in SERVERS for n in BOTS},
                       max_override=1200, digits=0, note="Ferrite parallelizes worldgen, lighting and sessions")
    parts.append("</svg>")
    with open(out_path, "w", encoding="utf-8") as f:
        f.write("\n".join(parts) + "\n")


def svg_scaling(data, out_path):
    w, h = 980, 470
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">',
        f'<rect width="{w}" height="{h}" rx="14" fill="#0d1117"/>',
        '<text x="28" y="34" fill="#e6edf3" font-family="ui-sans-serif,system-ui,sans-serif" font-size="19" font-weight="800">TPS vs bot count — fresh seed=0 (2026-11-23)</text>',
        '<text x="28" y="55" fill="#8b949e" font-family="ui-sans-serif,system-ui,sans-serif" font-size="11.5">Server-reported 1-minute TPS · higher is better · 20 TPS target</text>',
    ]
    parts += legend(44)
    left, right = 80, 930
    top, bottom = 100, 380
    import math
    def yy(v):
        return bottom - (v / 20.0) * (bottom - top)
    for v in (0, 5, 10, 15, 20):
        y = yy(v)
        parts.append(f'<line x1="{left}" y1="{y:.1f}" x2="{right}" y2="{y:.1f}" stroke="#21262d" stroke-dasharray="3,3"/>')
        parts.append(f'<text x="{left-8}" y="{y+3:.1f}" text-anchor="end" fill="#6e7681" font-family="ui-monospace,monospace" font-size="9">{v}</text>')
    parts.append(f'<line x1="{left}" y1="{bottom}" x2="{right}" y2="{bottom}" stroke="#30363d"/>')
    xs = {n: left + (i + 0.5) * (right - left) / len(BOTS) for i, n in enumerate(BOTS)}
    for n, x in xs.items():
        parts.append(f'<text x="{x}" y="{bottom+22}" text-anchor="middle" fill="#c9d1d9" font-family="ui-sans-serif,system-ui,sans-serif" font-size="11">{n} bots</text>')
    for s in SERVERS:
        pts = []
        for n in BOTS:
            d = data.get(f"{s}-{n}") or {}
            v = d.get("tps")
            p = d.get("players_seen", n)
            if v is None:
                continue
            pts.append((xs[n], yy(v), v, p))
        if len(pts) >= 2:
            parts.append(f'<polyline fill="none" stroke="{COLORS[s]}" stroke-width="2.4" points="' + " ".join(f"{x:.1f},{y:.1f}" for x, y, _, _ in pts) + '"/>')
        for x, y, v, p in pts:
            parts.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="4" fill="{COLORS[s]}" stroke="#0d1117" stroke-width="1.5"/>')
            parts.append(f'<text x="{x:.1f}" y="{y-9:.1f}" text-anchor="middle" fill="#e6edf3" font-family="ui-monospace,monospace" font-size="8.8">{v:.1f}</text>')
        if s == "folia":
            d = data.get("folia-400") or {}
            if d.get("players_seen", 0) < 400:
                x = xs[400]
                parts.append(f'<text x="{x:.1f}" y="{bottom+40}" text-anchor="middle" fill="#f87171" font-family="ui-sans-serif,system-ui,sans-serif" font-size="9" font-weight="700">Folia 400: closed at {d.get("players_seen", 0)}/400</text>')
    parts.append("</svg>")
    with open(out_path, "w", encoding="utf-8") as f:
        f.write("\n".join(parts) + "\n")


def markdown(data):
    print("| Server | Bots | Joined | Join p50 / p99 | TPS | Tick mean / p99 | CPU | Peak RSS |")
    print("|---|---:|---:|---:|---:|---:|---:|---:|")
    for s in SERVERS:
        for n in BOTS:
            d = data.get(f"{s}-{n}")
            if not d:
                print(f"| {s} | {n} | — | — | — | — | — | — |")
                continue
            print(f"| {s} | {n} | {d.get('players_seen', n)} | {fmt(d['join_p50'],0)} / {fmt(d['join_p99'],0)} ms | "
                  f"{fmt(d['tps'],1)} | {(fmt(d.get('tick_mean'),1)+' / '+fmt(d.get('tick_p99'),1)+' ms') if d.get('tick_mean') else '—'} | "
                  f"{fmt(d['cpu_pct'],0)}% | {fmt(d['rss_mib'],0)} MiB |")
    long = data.get("ferrite-400-long")
    if long:
        print("| ferrite-long | 400 | 400 | %s / %s ms | %s | — | %s%% | %s MiB |" % (
            fmt(long['join_p50'],0), fmt(long['join_p99'],0), fmt(long['tps'],1),
            fmt(long['cpu_pct'],0), fmt(long['rss_mib'],0)))


def main():
    data = collect()
    with open(os.path.join(D, "parsed.json"), "w", encoding="utf-8") as f:
        json.dump(data, f, indent=1)
    markdown(data)
    outdir = "/home/ubuntu/orca/workspaces/ferrite-mc/parallel-4oy8j-1/docs/perf/2026-11-23"
    if os.path.isdir(outdir):
        svg_full(data, os.path.join(outdir, "full-comparison.svg"))
        svg_scaling(data, os.path.join(outdir, "full-tps-scaling.svg"))
    print("\nparsed keys:", ", ".join(sorted(data)))


if __name__ == "__main__":
    main()
