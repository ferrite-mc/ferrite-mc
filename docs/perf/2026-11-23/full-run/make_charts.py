#!/usr/bin/env python3
"""Render the full-comparison and TPS-scaling SVGs from parsed.json."""
import json
import math
import os

D = "/tmp/ferrite-full-2026-11-23/results"
OUT = "/home/ubuntu/orca/workspaces/ferrite-mc/parallel-4oy8j-1/docs/perf/2026-11-23"
SERVERS = ["ferrite", "folia", "leaf", "paper", "purpur"]
COLORS = {
    "ferrite": "#f97316",
    "folia": "#38bdf8",
    "leaf": "#34d399",
    "paper": "#a78bfa",
    "purpur": "#f472b6",
}
NAMES = {"ferrite": "Ferrite", "folia": "Folia", "leaf": "Leaf", "paper": "Paper", "purpur": "Purpur"}
BOTS = [50, 100, 200, 400]
FONT = "ui-sans-serif,system-ui,sans-serif"
MONO = "ui-monospace,monospace"


def esc(s):
    return str(s).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def fmt(v, nd=1):
    if v is None:
        return "—"
    if abs(v) >= 1000:
        return f"{v:,.0f}"
    return f"{v:.{nd}f}"


def header(w, title, subtitle):
    return [
        f'<rect width="{w}" height="100%" rx="14" fill="#0d1117"/>',
        f'<text x="28" y="34" fill="#e6edf3" font-family="{FONT}" font-size="20" font-weight="800">{esc(title)}</text>',
        f'<text x="28" y="55" fill="#8b949e" font-family="{FONT}" font-size="11.5">{esc(subtitle)}</text>',
    ]


def server_legend(y):
    out, x = [], 560
    for s in SERVERS:
        out.append(f'<rect x="{x}" y="{y}" width="9" height="9" rx="2" fill="{COLORS[s]}"/>')
        out.append(f'<text x="{x+14}" y="{y+8}" fill="#c9d1d9" font-family="{FONT}" font-size="10">{NAMES[s]}</text>')
        x += 76
    out.append(f'<text x="{x}" y="{y+8}" fill="#8b949e" font-family="{FONT}" font-size="9.5">bots</text>')
    for i, n in enumerate(BOTS):
        out.append(f'<rect x="{x+34+i*30}" y="{y}" width="8" height="9" rx="2" fill="#e6edf3" opacity="{0.35+0.65*i/(len(BOTS)-1)}"/>')
        out.append(f'<text x="{x+46+i*30}" y="{y+8}" fill="#8b949e" font-family="{FONT}" font-size="9">{n}</text>')
    return out


def panel(x, y, w, h, title, subtitle, getter, scale, vmin, vmax, unit, note=None,
          ref=None, digits=0):
    out = [f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="10" fill="#161b22" stroke="#30363d"/>',
           f'<text x="{x+18}" y="{y+26}" fill="#e6edf3" font-family="{FONT}" font-size="13.5" font-weight="700">{esc(title)}</text>',
           f'<text x="{x+18}" y="{y+44}" fill="#8b949e" font-family="{FONT}" font-size="10.5">{esc(subtitle)}</text>']
    left = x + 92
    right = x + w - 58
    top = y + 62
    bottom = y + h - 34
    out.append(f'<line x1="{left}" y1="{bottom}" x2="{right}" y2="{bottom}" stroke="#30363d"/>')

    def sx(v):
        if scale == "log":
            v = max(v, vmin)
            f = (math.log10(v) - math.log10(vmin)) / (math.log10(vmax) - math.log10(vmin))
        else:
            f = (v - vmin) / (vmax - vmin)
        return left + min(max(f, 0.0), 1.0) * (right - left)

    if scale == "log":
        for v in (10, 100, 1000, 10000):
            if vmin <= v <= vmax:
                xx = sx(v)
                out.append(f'<line x1="{xx:.1f}" y1="{top}" x2="{xx:.1f}" y2="{bottom}" stroke="#21262d" stroke-dasharray="3,3"/>')
                out.append(f'<text x="{xx:.1f}" y="{bottom+14}" text-anchor="middle" fill="#6e7681" font-family="{MONO}" font-size="8.5">{v:,}</text>')
    else:
        for i in range(0, 5):
            v = vmin + (vmax - vmin) * i / 4
            xx = sx(v)
            out.append(f'<line x1="{xx:.1f}" y1="{top}" x2="{xx:.1f}" y2="{bottom}" stroke="#21262d" stroke-dasharray="3,3"/>')
            out.append(f'<text x="{xx:.1f}" y="{bottom+14}" text-anchor="middle" fill="#6e7681" font-family="{MONO}" font-size="8.5">{fmt(v,0)}</text>')
    if ref is not None:
        xx = sx(ref)
        out.append(f'<line x1="{xx:.1f}" y1="{top}" x2="{xx:.1f}" y2="{bottom}" stroke="#f97316" stroke-dasharray="4,3" opacity="0.6"/>')

    group_h = (bottom - top) / len(SERVERS)
    bar_h, bar_gap = 10, 1.5
    for gi, s in enumerate(SERVERS):
        cy = top + group_h * (gi + 0.5)
        out.append(f'<text x="{left-8}" y="{cy+3:.1f}" text-anchor="end" fill="#c9d1d9" font-family="{FONT}" font-size="10">{NAMES[s]}</text>')
        block_h = len(BOTS) * bar_h + (len(BOTS) - 1) * bar_gap
        by = cy - block_h / 2
        for bi, n in enumerate(BOTS):
            d = getter(s, n)
            yy = by + bi * (bar_h + bar_gap)
            if d["value"] is None:
                out.append(f'<text x="{left+4}" y="{yy+8:.1f}" fill="#f87171" font-family="{FONT}" font-size="8.5">{esc(d.get("label", "—"))}</text>')
                continue
            xx = sx(d["value"])
            out.append(f'<rect x="{left}" y="{yy:.1f}" width="{max(xx-left,1):.1f}" height="{bar_h}" rx="2" fill="{COLORS[s]}" opacity="{0.35+0.65*bi/(len(BOTS)-1)}"/>')
            label = fmt(d["value"], digits)
            out.append(f'<text x="{xx+5:.1f}" y="{yy+8:.1f}" fill="#c9d1d9" font-family="{MONO}" font-size="8.3">{label}</text>')
    if note:
        out.append(f'<text x="{x+18}" y="{y+h-7}" fill="#8b949e" font-family="{FONT}" font-size="8.6">{esc(note)}</text>')
    return out


def svg_full(data, out_path):
    w, h = 1100, 830

    def val(field, missing=None):
        def get(s, n):
            d = data.get(f"{s}-{n}")
            if not d:
                return {"value": None, "label": (missing or {}).get(s, "—")}
            v = d.get(field)
            if v is None:
                if s == "folia" and n == 400:
                    return {"value": None, "label": "closed at 275/400"}
                return {"value": None, "label": "—"}
            return {"value": v}
        return get

    parts = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">']
    parts += header(w, "Ferrite vs Folia · Leaf · Paper · Purpur 26.3 — full fresh-world run", "50 / 100 / 200 / 400 bots · empty seed=0 · BOTS_STILL=1 · view=10 · 5 ms stagger · 10 s settle + 30 s walk · sequential, one server at a time")
    parts += server_legend(66)
    parts += panel(28, 88, 512, 342, "Server-reported TPS (1 min)", "higher is better · includes the join storm · target 20",
                   val("tps"), "linear", 0, 20, " TPS", ref=20, digits=1)
    parts += panel(560, 88, 512, 342, "Peak RSS", "lower is better · server process only · MiB",
                   val("rss_mib"), "linear", 0, 5000, " MiB", digits=0,
                   note="* Folia 400 is a partial run at 275 players")
    parts += panel(28, 444, 512, 342, "Join latency, p50", "lower is better · log scale · ms",
                   val("join_p50"), "log", 10, 30000, " ms", digits=0,
                   note="Folia 400 closed connections at 275/400")
    parts += panel(560, 444, 512, 342, "Mean process CPU", "12 vCPU host · 1200% = all cores · informational",
                   val("cpu_pct"), "linear", 0, 1200, "%", digits=0,
                   note="Ferrite parallelizes worldgen, lighting and sessions")
    parts.append(f'<text x="28" y="{h-10}" fill="#6e7681" font-family="{FONT}" font-size="8.6">Single observation per point · peak RSS sampled 1 s from /proc · CPU from utime+stime over the bot run · JVM: -Xms1G -Xmx4G, no plugins except the probe</text>')
    parts.append("</svg>")
    with open(out_path, "w", encoding="utf-8") as f:
        f.write("\n".join(parts) + "\n")


def svg_scaling(data, out_path):
    w, h = 980, 470
    parts = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">']
    parts += header(w, "TPS vs bot count — full fresh-world run", "server-reported 1-minute TPS · higher is better · 20 TPS target")
    parts += server_legend(64)
    left, right, top, bottom = 80, 900, 100, 380
    ref_y = bottom - (20 / 22) * (bottom - top)

    def yy(v):
        return bottom - (v / 22.0) * (bottom - top)

    for v in (0, 5, 10, 15, 20):
        out_y = yy(v)
        parts.append(f'<line x1="{left}" y1="{out_y:.1f}" x2="{right}" y2="{out_y:.1f}" stroke="#21262d" stroke-dasharray="3,3"/>')
        parts.append(f'<text x="{left-8}" y="{out_y+3:.1f}" text-anchor="end" fill="#6e7681" font-family="{MONO}" font-size="9">{v}</text>')
    parts.append(f'<line x1="{left}" y1="{bottom}" x2="{right}" y2="{bottom}" stroke="#30363d"/>')
    xs = {n: left + (i + 0.5) * (right - left) / len(BOTS) for i, n in enumerate(BOTS)}
    for n, x in xs.items():
        parts.append(f'<text x="{x:.1f}" y="{bottom+22}" text-anchor="middle" fill="#c9d1d9" font-family="{FONT}" font-size="11">{n} bots</text>')
    for s in SERVERS:
        pts = []
        for n in BOTS:
            d = data.get(f"{s}-{n}") or {}
            if d.get("tps") is None:
                continue
            pts.append((xs[n], yy(d["tps"]), d["tps"]))
        if len(pts) >= 2:
            parts.append(f'<polyline fill="none" stroke="{COLORS[s]}" stroke-width="2.4" points="' + " ".join(f"{x:.1f},{y:.1f}" for x, y, _ in pts) + '"/>')
        for x, y, v in pts:
            parts.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="4" fill="{COLORS[s]}" stroke="#0d1117" stroke-width="1.5"/>')
            parts.append(f'<text x="{x:.1f}" y="{y-10:.1f}" text-anchor="middle" fill="#e6edf3" font-family="{MONO}" font-size="8.8">{v:.1f}</text>')
    d = data.get("folia-400") or {}
    if d.get("players_seen", 0) < 400:
        x = xs[400]
        y = yy((data.get("folia-200") or {}).get("tps") or 0)
        parts.append(f'<line x1="{xs[200]:.1f}" y1="{y:.1f}" x2="{x:.1f}" y2="{y:.1f}" stroke="{COLORS["folia"]}" stroke-width="1.4" stroke-dasharray="4,3" opacity="0.6"/>')
        parts.append(f'<text x="{x:.1f}" y="{y-8:.1f}" text-anchor="middle" fill="#f87171" font-family="{FONT}" font-size="9" font-weight="700">closed at {d.get("players_seen", 0)}/400</text>')
    parts.append(f'<text x="{left}" y="{bottom+44}" fill="#8b949e" font-family="{FONT}" font-size="9">Folia did not complete the 400-bot run; its dashed segment is not a 400-bot measurement.</text>')
    parts.append("</svg>")
    with open(out_path, "w", encoding="utf-8") as f:
        f.write("\n".join(parts) + "\n")


def main():
    with open(os.path.join(D, "parsed.json"), encoding="utf-8") as f:
        data = json.load(f)
    svg_full(data, os.path.join(OUT, "full-comparison.svg"))
    svg_scaling(data, os.path.join(OUT, "full-tps-scaling.svg"))
    print("wrote", os.path.join(OUT, "full-comparison.svg"))
    print("wrote", os.path.join(OUT, "full-tps-scaling.svg"))


if __name__ == "__main__":
    main()
