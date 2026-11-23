#!/usr/bin/env python3
"""Aggregate detailed benchmark JSON and render the SVG charts."""
from __future__ import annotations

import argparse
import json
import math
import statistics
from pathlib import Path

SERVERS = ["ferrite", "folia", "leaf", "paper", "purpur"]
COLORS = {
    "ferrite": "#f97316",
    "folia": "#38bdf8",
    "leaf": "#34d399",
    "paper": "#a78bfa",
    "purpur": "#f472b6",
}
LABEL = {
    "ferrite": "Ferrite",
    "folia": "Folia",
    "leaf": "Leaf",
    "paper": "Paper",
    "purpur": "Purpur",
}
VIEWS = [6, 10, 16]
BG = "#0d1117"
PANEL = "#161b22"
BORDER = "#30363d"
TEXT = "#e6edf3"
MUTED = "#8b949e"
GRID = "#21262d"
FONT = "ui-sans-serif,system-ui,sans-serif"
MONO = "ui-monospace,SFMono-Regular,monospace"


def esc(value) -> str:
    return str(value).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def fmt(value, digits=1) -> str:
    if value is None:
        return "—"
    if abs(value) >= 1000:
        return f"{value:,.0f}"
    return f"{value:.{digits}f}"


def mean(values):
    return statistics.mean(values) if values else None


def pct(values, q):
    if not values:
        return None
    s = sorted(values)
    return s[min(len(s) - 1, int((len(s) - 1) * q))]


def load_runs(directory: Path) -> list[dict]:
    runs = []
    for path in sorted((directory / "runs").glob("*/meta.json")):
        try:
            runs.append(json.loads(path.read_text(encoding="utf-8")))
        except (OSError, json.JSONDecodeError):
            pass
    return runs


def aggregate(runs: list[dict]) -> dict:
    fresh = {}
    fixture = {}
    loads = {}
    eight = {}
    for r in runs:
        if r.get("error"):
            continue
        if r.get("kind") == "chunk_stream":
            key = (r["variant"], r["view"])
            target = fresh if r.get("world") == "fresh" else fixture
            target.setdefault(key, []).append(r)
        elif r.get("kind") == "load" and r.get("bots") == 200 and r.get("world") == "fixture":
            loads[r["variant"]] = r
        elif r.get("kind") == "load" and r.get("bots") == 800:
            eight[(r["variant"], r["world"])] = r
    out = {"fresh": {}, "fixture": {}, "loads": loads, "eight": eight}
    for key, rows in fresh.items():
        rates = [x["rate"] for x in rows if x.get("rate") is not None]
        out["fresh"][key] = {
            "n": len(rows),
            "rate": mean(rates),
            "rate_min": min(rates) if rates else None,
            "rate_max": max(rates) if rates else None,
            "count": [x.get("count") for x in rows],
            "boot": mean([x["boot_ms"] for x in rows if x.get("boot_ms") is not None]),
            "idle_rss": mean([x["idle_rss_mib"] for x in rows if x.get("idle_rss_mib") is not None]),
            "cpu": mean([x["cpu_pct"] for x in rows if x.get("cpu_pct") is not None]),
        }
    for key, rows in fixture.items():
        rates = [x["rate"] for x in rows if x.get("rate") is not None]
        out["fixture"][key] = {
            "n": len(rows),
            "rate": mean(rates),
            "rate_min": min(rates) if rates else None,
            "rate_max": max(rates) if rates else None,
            "count": [x.get("count") for x in rows],
            "boot": mean([x["boot_ms"] for x in rows if x.get("boot_ms") is not None]),
            "idle_rss": mean([x["idle_rss_mib"] for x in rows if x.get("idle_rss_mib") is not None]),
            "cpu": mean([x["cpu_pct"] for x in rows if x.get("cpu_pct") is not None]),
        }
    return out


def rect(x, y, w, h, fill=PANEL, stroke=BORDER, rx=10):
    return f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{rx}" fill="{fill}" stroke="{stroke}"/>'


def text(x, y, value, size=10, fill=TEXT, anchor="start", weight="400", family=FONT):
    return (f'<text x="{x:.1f}" y="{y:.1f}" fill="{fill}" font-family="{family}" '
            f'font-size="{size}" text-anchor="{anchor}" font-weight="{weight}">{esc(value)}</text>')


def panel(x, y, w, h, title, subtitle):
    return [
        rect(x, y, w, h),
        text(x + 16, y + 24, title, 13.5, TEXT, weight="700"),
        text(x + 16, y + 41, subtitle, 10.2, MUTED),
    ]


def legend(x, y, entries):
    out = []
    for i, (name, color) in enumerate(entries):
        xx = x + i * 112
        out.append(f'<rect x="{xx}" y="{y-8}" width="9" height="9" rx="2" fill="{color}"/>')
        out.append(text(xx + 14, y, name, 10.5, TEXT))
    return out


def linear(v, lo, hi, top, bottom):
    if hi <= lo:
        hi = lo + 1
    f = (v - lo) / (hi - lo)
    return bottom - max(0.0, min(1.0, f)) * (bottom - top)


def log(v, lo, hi, top, bottom):
    v = max(v, lo)
    f = (math.log10(v) - math.log10(lo)) / (math.log10(hi) - math.log10(lo))
    return bottom - max(0.0, min(1.0, f)) * (bottom - top)


def axes(out, x, y, w, h, top, bottom, left, right, grid_values, scale_fn, labels, title):
    out.append(f'<line x1="{left}" y1="{bottom}" x2="{right}" y2="{bottom}" stroke="{BORDER}"/>')
    for value in grid_values:
        yy = scale_fn(value)
        out.append(f'<line x1="{left}" y1="{yy:.1f}" x2="{right}" y2="{yy:.1f}" stroke="{GRID}" stroke-dasharray="3,3"/>')
        out.append(text(left - 7, yy + 3, labels(value), 8.5, MUTED, "end", family=MONO))
    out.append(text(left - 7, top - 8, title, 8.5, MUTED, "end", family=MONO))


def grouped(out, x, y, w, h, groups, series, values, scale="linear", lo=0.0, hi=None,
            grid=None, fmt_value=fmt, log_labels=None, label_values=False):
    left = x + 56
    right = x + w - 18
    top = y + 72
    bottom = y + h - 55
    all_values = [v[0] if isinstance(v, tuple) else v for v in values.values() if v is not None]
    if hi is None:
        hi = max(all_values, default=1) * 1.15
        if scale == "linear":
            hi = max(hi, 1e-9)
    if grid is None:
        if scale == "log":
            grid = [10, 100, 1000, 10000]
        else:
            grid = [hi * i / 4 for i in range(1, 5)]
    if scale == "log":
        scale_fn = lambda v: log(v, lo, hi, top, bottom)
        labels = lambda v: f"{v:,.0f}"
    else:
        scale_fn = lambda v: linear(v, lo, hi, top, bottom)
        labels = lambda v: fmt(v, 0)
    axes(out, x, y, w, h, top, bottom, left, right, grid, scale_fn, labels, "")
    span = (right - left) / len(groups)
    bar_w = min(19.0, (span - 18) / len(series))
    for gi, group in enumerate(groups):
        cx = left + span * (gi + 0.5)
        out.append(text(cx, bottom + 20, group, 10, TEXT, "middle"))
        total = bar_w * len(series) + 3 * (len(series) - 1)
        sx = cx - total / 2
        for si, (name, color) in enumerate(series):
            raw = values.get((group, name))
            if raw is None:
                continue
            val, vmin, vmax = raw if isinstance(raw, tuple) else (raw, None, None)
            bx = sx + si * (bar_w + 3)
            yy = scale_fn(val)
            out.append(f'<rect x="{bx:.1f}" y="{yy:.1f}" width="{bar_w:.1f}" height="{max(1, bottom-yy):.1f}" rx="2" fill="{color}" opacity="0.92"/>')
            if vmin is not None and vmax is not None and vmax > vmin:
                a = scale_fn(vmin)
                b = scale_fn(vmax)
                out.append(f'<line x1="{bx+bar_w/2:.1f}" y1="{a:.1f}" x2="{bx+bar_w/2:.1f}" y2="{b:.1f}" stroke="#e6edf3" stroke-width="1" opacity="0.6"/>')
                out.append(f'<line x1="{bx+bar_w/2-3:.1f}" y1="{a:.1f}" x2="{bx+bar_w/2+3:.1f}" y2="{a:.1f}" stroke="#e6edf3" stroke-width="1" opacity="0.6"/>')
                out.append(f'<line x1="{bx+bar_w/2-3:.1f}" y1="{b:.1f}" x2="{bx+bar_w/2+3:.1f}" y2="{b:.1f}" stroke="#e6edf3" stroke-width="1" opacity="0.6"/>')
            if label_values:
                out.append(text(bx + bar_w / 2, yy - 5, fmt_value(val), 8.2, TEXT, "middle", family=MONO))


def header(width=1200):
    return [f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="1" viewBox="0 0 {width} 1">',
            f'<rect width="{width}" height="1" fill="{BG}"/>']


def finish(parts, width, height):
    body = "\n".join(parts)
    body = body.replace(f'height="1" viewBox="0 0 {width} 1"', f'height="{height}" viewBox="0 0 {width} {height}"', 1)
    return body + "\n</svg>\n"


def chart_chunks(agg, out_dir: Path):
    width, height = 1200, 620
    p = header(width)
    p.append(rect(0, 0, width, height, BG, BG, 0))
    p.append(text(28, 34, "Chunk streaming: fresh world vs pre-generated fixture", 20, TEXT, weight="700"))
    p.append(text(28, 57, "One bot, connect-to-last-chunk, 8 s quiet tail; bars are the mean of two fresh-world runs (whiskers min–max).", 11.5, MUTED))
    p += legend(660, 30, [(LABEL[s], COLORS[s]) for s in SERVERS])
    p += panel(24, 86, 560, 500, "Fresh world (seed=0)", "chunks/s; higher is better")
    groups = [f"view {v}" for v in VIEWS]
    values = {}
    for v in VIEWS:
        for s in SERVERS:
            a = agg["fresh"].get((s, v))
            if a:
                values[(f"view {v}", s)] = (a["rate"], a["rate_min"], a["rate_max"])
    grouped(p, 24, 86, 560, 500, groups, [(s, COLORS[s]) for s in SERVERS], values,
            scale="linear", lo=0, hi=190, grid=[50, 100, 150], label_values=True)
    p += panel(608, 86, 568, 500, "Fixture world (view 10)", "chunks/s; log scale")
    fvalues = {}
    for s in SERVERS:
        a = agg["fixture"].get((s, 10))
        if a:
            fvalues[("fixture", s)] = (a["rate"], a["rate_min"], a["rate_max"])
    grouped(p, 608, 86, 568, 500, ["fixture"], [(s, COLORS[s]) for s in SERVERS], fvalues,
            scale="log", lo=40, hi=2000, grid=[40, 100, 300, 1000], label_values=True)
    p.append(text(630, 562, "Ferrite uses Anvil for the fixture world; JVM servers load the same copied world.",
                  9.5, MUTED))
    (out_dir / "chunk-stream.svg").write_text(finish(p, width, height), encoding="utf-8")


def chart_fixture(agg, out_dir: Path):
    width, height = 1200, 760
    p = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
         rect(0, 0, width, height, BG, BG, 0),
         text(28, 34, "Pre-generated fixture world: 200 bots and forceload latency", 20, TEXT, weight="700"),
         text(28, 57, "Same host, view-distance 10, 10 s settle + 40 s walk; one run per server.", 11.5, MUTED)]
    p += legend(660, 30, [(LABEL[s], COLORS[s]) for s in SERVERS])
    # TPS
    p += panel(24, 84, 560, 300, "Server TPS (1 min)", "higher is better")
    vals = {("200 bots", s): agg["loads"][s].get("tps_1m") for s in SERVERS if s in agg["loads"]}
    grouped(p, 24, 84, 560, 300, ["200 bots"], [(s, COLORS[s]) for s in SERVERS], vals,
            scale="linear", lo=0, hi=21, grid=[5, 10, 15, 20], label_values=True)
    # Join p50/p99
    p += panel(608, 84, 568, 300, "Join latency", "ms; log scale; p50 / p99")
    jvals = {}
    for s in SERVERS:
        r = agg["loads"].get(s)
        if r:
            jvals[("200 bots", f"{s}-p50")] = r.get("join_p50")
            jvals[("200 bots", f"{s}-p99")] = r.get("join_p99")
    jseries = []
    for s in SERVERS:
        jseries.append((f"{s}-p50", COLORS[s]))
        jseries.append((f"{s}-p99", COLORS[s]))
    grouped(p, 608, 84, 568, 300, ["200 bots"], jseries, jvals,
            scale="log", lo=20, hi=40000, grid=[20, 100, 1000, 10000])
    p.append(text(630, 358, "first bar: p50   second bar: p99", 9.5, MUTED))
    # RSS
    p += panel(24, 404, 560, 300, "Peak RSS", "MiB; lower is better")
    rvals = {("200 bots", s): agg["loads"][s].get("peak_rss_mib") for s in SERVERS if s in agg["loads"]}
    grouped(p, 24, 404, 560, 300, ["200 bots"], [(s, COLORS[s]) for s in SERVERS], rvals,
            scale="linear", lo=0, hi=5200, grid=[1000, 2000, 3000, 4000, 5000], label_values=True)
    # Forceload
    p += panel(608, 404, 568, 300, "Forceload command RTT", "20 cycles, 256 chunks; mean ms; lower is better")
    fvals = {}
    for s in SERVERS:
        r = agg["loads"].get(s)
        if not r:
            continue
        add = r.get("forceload_add_ms") or []
        rem = r.get("forceload_remove_ms") or []
        if add:
            fvals[("20 cycles", f"{s}-add")] = (mean(add), min(add), max(add))
        if rem:
            fvals[("20 cycles", f"{s}-remove")] = (mean(rem), min(rem), max(rem))
    fseries = []
    for s in SERVERS:
        fseries.append((f"{s}-add", COLORS[s]))
        fseries.append((f"{s}-remove", COLORS[s]))
    grouped(p, 608, 404, 568, 300, ["20 cycles"], fseries, fvals,
            scale="linear", lo=0, hi=220, grid=[50, 100, 150, 200])
    p.append(text(630, 678, "add (first bar) / remove all (second bar); whiskers are min–max.",
                  9.5, MUTED))
    p.append("</svg>")
    (out_dir / "fixture-load.svg").write_text("\n".join(p), encoding="utf-8")


def chart_800(agg, out_dir: Path):
    width, height = 1200, 360
    p = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
         rect(0, 0, width, height, BG, BG, 0),
         text(28, 34, "Ferrite at 200 and 800 bots", 20, TEXT, weight="700"),
         text(28, 57, "200 fixture from the load run; 800 fixture/fresh are separate single runs, view 10, 40 s walk.", 11.5, MUTED)]
    groups = ["200 fixture", "800 fixture", "800 fresh"]
    rows = {
        "200 fixture": agg["loads"].get("ferrite"),
        "800 fixture": agg["eight"].get(("ferrite", "fixture")),
        "800 fresh": agg["eight"].get(("ferrite", "fresh")),
    }
    # TPS
    p += panel(24, 84, 276, 250, "TPS (1 min)", "higher is better")
    vals = {("g", g): (rows[g].get("tps_1m") if rows[g] else None) for g in groups}
    grouped(p, 24, 84, 276, 250, groups, [("g", COLORS["ferrite"])], vals,
            scale="linear", lo=0, hi=21, grid=[5, 10, 15, 20], label_values=True)
    # Join
    p += panel(312, 84, 276, 250, "Join p50", "ms; log scale")
    vals = {("g", g): (rows[g].get("join_p50") if rows[g] else None) for g in groups}
    grouped(p, 312, 84, 276, 250, groups, [("g", COLORS["ferrite"])], vals,
            scale="log", lo=20, hi=20000, grid=[20, 100, 1000, 10000], label_values=True)
    # RSS
    p += panel(600, 84, 276, 250, "Peak RSS", "MiB")
    vals = {("g", g): (rows[g].get("peak_rss_mib") if rows[g] else None) for g in groups}
    grouped(p, 600, 84, 276, 250, groups, [("g", COLORS["ferrite"])], vals,
            scale="linear", lo=0, hi=3000, grid=[500, 1000, 1500, 2000, 2500], label_values=True)
    # CPU
    p += panel(888, 84, 288, 250, "CPU", "% of 12 CPUs")
    vals = {("g", g): (rows[g].get("cpu_pct") if rows[g] else None) for g in groups}
    grouped(p, 888, 84, 288, 250, groups, [("g", COLORS["ferrite"])], vals,
            scale="linear", lo=0, hi=1200, grid=[300, 600, 900, 1200], label_values=True)
    p.append("</svg>")
    (out_dir / "ferrite-800.svg").write_text("\n".join(p), encoding="utf-8")


def json_safe(value):
    if isinstance(value, dict):
        out = {}
        for key, item in value.items():
            if isinstance(key, tuple):
                key = "|".join(str(part) for part in key)
            out[str(key)] = json_safe(item)
        return out
    if isinstance(value, list):
        return [json_safe(item) for item in value]
    return value


def summary_md(agg) -> str:
    lines = ["# Detailed benchmark aggregates (generated)", ""]
    lines += ["## Fresh-world chunk stream", "",
              "| Server | View | Runs | Chunks/s mean | Min–max | Chunks | Boot ms | Idle RSS MiB | CPU % |",
              "|---|---:|---:|---:|---:|---:|---:|---:|---:|"]
    for v in VIEWS:
        for s in SERVERS:
            a = agg["fresh"].get((s, v))
            if not a:
                continue
            lines.append(f"| {LABEL[s]} | {v} | {a['n']} | {fmt(a['rate'])} | {fmt(a['rate_min'])}–{fmt(a['rate_max'])} | "
                         f"{'/'.join(str(x) for x in a['count'])} | {fmt(a['boot'], 0)} | {fmt(a['idle_rss'])} | {fmt(a['cpu'], 0)} |")
    lines += ["", "## Fixture-world chunk stream (view 10)", "",
              "| Server | Runs | Chunks/s mean | Min–max | Chunks | Boot ms | Idle RSS MiB | CPU % |",
              "|---|---:|---:|---:|---:|---:|---:|---:|"]
    for s in SERVERS:
        a = agg["fixture"].get((s, 10))
        if a:
            lines.append(f"| {LABEL[s]} | {a['n']} | {fmt(a['rate'])} | {fmt(a['rate_min'])}–{fmt(a['rate_max'])} | "
                         f"{'/'.join(str(x) for x in a['count'])} | {fmt(a['boot'], 0)} | {fmt(a['idle_rss'])} | {fmt(a['cpu'], 0)} |")
    lines += ["", "## Fixture world, 200 bots", "",
              "| Server | TPS 1m | Join p50 / p99 ms | Peak RSS MiB | CPU % | Forceload add mean / p99 ms | Remove mean / p99 ms |",
              "|---|---:|---:|---:|---:|---:|---:|"]
    for s in SERVERS:
        r = agg["loads"].get(s)
        if not r:
            continue
        add = r.get("forceload_add_ms") or []
        rem = r.get("forceload_remove_ms") or []
        lines.append(f"| {LABEL[s]} | {fmt(r.get('tps_1m'), 2)} | {fmt(r.get('join_p50'))} / {fmt(r.get('join_p99'))} | "
                     f"{fmt(r.get('peak_rss_mib'))} | {fmt(r.get('cpu_pct'), 0)} | "
                     f"{fmt(mean(add))} / {fmt(pct(add, 0.99))} | {fmt(mean(rem))} / {fmt(pct(rem, 0.99))} |")
    lines += ["", "## Ferrite 800 bots", "",
              "| World | Bots | TPS 1m | Join p50 / p99 ms | Peak RSS MiB | CPU % | Wall s |",
              "|---|---:|---:|---:|---:|---:|---:|"]
    for world in ("fixture", "fresh"):
        r = agg["eight"].get(("ferrite", world))
        if r:
            lines.append(f"| {world} | {r.get('bots')} | {fmt(r.get('tps_1m'), 2)} | {fmt(r.get('join_p50'))} / {fmt(r.get('join_p99'))} | "
                         f"{fmt(r.get('peak_rss_mib'))} | {fmt(r.get('cpu_pct'), 0)} | {fmt(r.get('wall_s'))} |")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dir", type=Path, default=Path("/tmp/ferrite-detailed-2026-11-23"))
    args = parser.parse_args()
    runs = load_runs(args.dir)
    agg = aggregate(runs)
    args.dir.mkdir(parents=True, exist_ok=True)
    (args.dir / "summary.json").write_text(json.dumps(json_safe(agg), indent=2) + "\n", encoding="utf-8")
    (args.dir / "summary.md").write_text(summary_md(agg), encoding="utf-8")
    chart_chunks(agg, args.dir)
    chart_fixture(agg, args.dir)
    chart_800(agg, args.dir)
    print(json.dumps({
        "runs": len(runs),
        "fresh_points": len(agg["fresh"]),
        "fixture_points": len(agg["fixture"]),
        "loads": sorted(agg["loads"]),
        "eight": [f"{k[0]}-{k[1]}" for k in agg["eight"]],
    }, indent=2))


if __name__ == "__main__":
    main()
