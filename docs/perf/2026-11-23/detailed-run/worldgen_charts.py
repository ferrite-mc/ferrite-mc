#!/usr/bin/env python3
"""Render the detailed worldgen SVG from worldgen_bench CSV output."""
from __future__ import annotations

import csv
import math
import statistics
from pathlib import Path

HERE = Path(__file__).resolve().parent
CSV_PATH = HERE / "raw" / "worldgen_scaling.csv"
OUT_PATH = HERE.parent / "worldgen-scaling.svg"

BG = "#0d1117"
PANEL = "#161b22"
BORDER = "#30363d"
TEXT = "#e6edf3"
MUTED = "#8b949e"
GRID = "#21262d"
ORANGE = "#f97316"
BLUE = "#38bdf8"
GREEN = "#34d399"
PINK = "#f472b6"
PURPLE = "#a78bfa"
FONT = "ui-sans-serif,system-ui,sans-serif"
MONO = "ui-monospace,SFMono-Regular,monospace"


def esc(value) -> str:
    return str(value).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def fmt(value: float, digits: int = 1) -> str:
    if abs(value) >= 1000:
        return f"{value:,.0f}"
    return f"{value:,.{digits}f}"


def rect(x, y, w, h, fill=PANEL, stroke=BORDER, rx=10):
    return f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{rx}" fill="{fill}" stroke="{stroke}"/>'


def text(x, y, value, size=10, fill=TEXT, anchor="start", weight="400", family=FONT):
    return (
        f'<text x="{x:.1f}" y="{y:.1f}" fill="{fill}" font-family="{family}" '
        f'font-size="{size}" text-anchor="{anchor}" font-weight="{weight}">{esc(value)}</text>'
    )


def panel(x, y, w, h, title, subtitle):
    return [
        rect(x, y, w, h),
        text(x + 16, y + 24, title, 13.5, TEXT, weight="700"),
        text(x + 16, y + 41, subtitle, 10.2, MUTED),
    ]


def linear(value, lo, hi, top, bottom):
    if hi <= lo:
        return (top + bottom) / 2
    return bottom - (value - lo) / (hi - lo) * (bottom - top)


def nice_max(value: float) -> float:
    if value <= 0:
        return 1.0
    magnitude = 10 ** len(str(int(value))) / 10
    for step in (1, 2, 2.5, 5, 10):
        candidate = step * magnitude
        if candidate >= value:
            return candidate
    return value


def median(values):
    return statistics.median(values) if values else 0.0


def load_rows():
    rows = []
    with CSV_PATH.open(newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle):
            rows.append(
                {
                    "section": row["section"],
                    "dimension": row["dimension"],
                    "threads": int(row["threads"]),
                    "rep": int(row["rep"]),
                    "chunks": int(row["chunks"]),
                    "cps": float(row["cps"]),
                    "p50": float(row["p50_ms"]),
                    "p95": float(row["p95_ms"]),
                    "p99": float(row["p99_ms"]),
                }
            )
    return rows


def aggregate(rows):
    scaling = {}
    for row in rows:
        if row["section"] != "scaling":
            continue
        bucket = scaling.setdefault(row["threads"], {"cps": [], "p50": [], "p95": [], "p99": []})
        bucket["cps"].append(row["cps"])
        bucket["p50"].append(row["p50"])
        bucket["p95"].append(row["p95"])
        bucket["p99"].append(row["p99"])
    threads = sorted(scaling)
    series = {
        "threads": threads,
        "cps": [median(scaling[t]["cps"]) for t in threads],
        "cps_min": [min(scaling[t]["cps"]) for t in threads],
        "cps_max": [max(scaling[t]["cps"]) for t in threads],
        "p50": [median(scaling[t]["p50"]) for t in threads],
        "p95": [median(scaling[t]["p95"]) for t in threads],
        "p99": [median(scaling[t]["p99"]) for t in threads],
        "reps": len(scaling[threads[0]]["cps"]),
        "chunks": max(
            (row["chunks"] for row in rows if row["section"] == "scaling"), default=0
        ),
    }

    dimensions = {}
    for row in rows:
        if row["section"] != "dimension":
            continue
        dimensions.setdefault(row["dimension"], {}).setdefault(row["threads"], []).append(row["cps"])
    dims = {name: {t: median(v) for t, v in by_thread.items()} for name, by_thread in dimensions.items()}
    return series, dims


def x_axis(threads, left, right):
    lo, hi = min(threads), max(threads)
    return {t: left + (t - lo) / max(hi - lo, 1) * (right - left) for t in threads}


def draw_axes(lines, x0, x1, top, bottom, y_max, ticks, label_fmt=fmt, y_label=""):
    for i in range(ticks + 1):
        value = y_max * i / ticks
        y = linear(value, 0, y_max, top, bottom)
        lines.append(
            f'<line x1="{x0}" y1="{y:.1f}" x2="{x1}" y2="{y:.1f}" stroke="{GRID}" stroke-width="1"/>'
        )
        lines.append(text(x0 - 8, y + 3.5, label_fmt(value), 9.2, MUTED, anchor="end", family=MONO))
    if y_label:
        lines.append(
            text(x0 - 44, top - 8, y_label, 9.5, MUTED, anchor="start")
        )


def draw_x_ticks(lines, mapping, bottom, values=None):
    for t in values or sorted(mapping):
        lines.append(text(mapping[t], bottom + 17, str(t), 9.5, MUTED, anchor="middle", family=MONO))


def polyline(points, color, width=2.0, dashed=False):
    path = " ".join(f"{x:.1f},{y:.1f}" for x, y in points)
    dash = ' stroke-dasharray="5 4"' if dashed else ""
    return (
        f'<polyline points="{path}" fill="none" stroke="{color}" stroke-width="{width}" '
        f'stroke-linejoin="round" stroke-linecap="round"{dash}/>'
    )


def dot(x, y, color, r=3.2):
    return f'<circle cx="{x:.1f}" cy="{y:.1f}" r="{r}" fill="{color}" stroke="{BG}" stroke-width="1"/>'


def main():
    rows = load_rows()
    series, dims = aggregate(rows)
    threads = series["threads"]
    base_cps = series["cps"][0]
    final_cps = series["cps"][-1]
    final_speedup = final_cps / base_cps
    final_efficiency = final_speedup / threads[-1] * 100.0
    max_threads = threads[-1]
    reps = series["reps"]
    chunks = series["chunks"]

    width, height = 1240, 1140
    lines = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" '
        f'viewBox="0 0 {width} {height}">',
        f'<rect width="{width}" height="{height}" fill="{BG}"/>',
        text(24, 34, "Ferrite pure-Rust worldgen scaling — detailed run (2026-11-23)", 20, TEXT, weight="700"),
        text(
            24,
            55,
            "Full RustGenerator::generate (terrain, structure starts/jigsaw, decoration); "
            "lighting, serialization and networking excluded.",
            11,
            MUTED,
        ),
        text(
            24,
            72,
            f"Xeon Platinum 8358 · {max_threads} logical CPUs (6 cores × 2 SMT) · seed 12345 · "
            f"{chunks} Overworld chunks × {reps} trials per thread count · one process at a time",
            10,
            MUTED,
        ),
    ]

    # --- Panel A: throughput by thread count ------------------------------------------------
    ax, ay, aw, ah = 24, 92, 760, 340
    lines += panel(ax, ay, aw, ah, "Overworld throughput by thread count",
                   f"line = median of {reps} trials; band = trial min–max; dashed = linear scaling")
    x0, x1, top, bottom = ax + 66, ax + aw - 20, ay + 66, ay + ah - 44
    y_max = math.ceil(base_cps * max_threads * 1.02 / 400) * 400
    draw_axes(lines, x0, x1, top, bottom, y_max, 4, label_fmt=lambda v: f"{v:,.0f}", y_label="chunks/s")
    mapping = x_axis(threads, x0, x1)
    draw_x_ticks(lines, mapping, bottom)
    ideal = [(mapping[t], linear(base_cps * t, 0, y_max, top, bottom)) for t in threads]
    lines.append(polyline(ideal, MUTED, 1.4, dashed=True))
    upper = [
        (mapping[t], linear(series["cps_max"][i], 0, y_max, top, bottom))
        for i, t in enumerate(threads)
    ]
    lower = [
        (mapping[t], linear(series["cps_min"][i], 0, y_max, top, bottom))
        for i, t in enumerate(threads)
    ][::-1]
    band = " ".join(f"{x:.1f},{y:.1f}" for x, y in upper + lower)
    lines.append(f'<polygon points="{band}" fill="{ORANGE}" opacity="0.14"/>')
    actual = [
        (mapping[t], linear(series["cps"][i], 0, y_max, top, bottom))
        for i, t in enumerate(threads)
    ]
    lines.append(polyline(actual, ORANGE, 2.4))
    for x, y in actual:
        lines.append(dot(x, y, ORANGE))
    lines.append(
        text(mapping[threads[-1]] - 4, actual[-1][1] - 10, f"{final_cps:,.0f}", 11, ORANGE,
             anchor="end", weight="700", family=MONO)
    )
    lines.append(text(x1 - 4, top + 12, f"linear ideal at {max_threads}T: {base_cps * max_threads:,.0f}",
                      9.5, MUTED, anchor="end", family=MONO))

    # --- Panel B: speedup -------------------------------------------------------------------
    bx, by, bw, bh = 800, 92, 416, 340
    lines += panel(bx, by, bw, bh, "Speedup vs one thread", "dashed = perfect scaling; label = parallel efficiency")
    x0, x1, top, bottom = bx + 58, bx + bw - 20, by + 66, by + bh - 44
    draw_axes(lines, x0, x1, top, bottom, max_threads, 3, label_fmt=lambda v: f"{v:.0f}x")
    mapping = x_axis(threads, x0, x1)
    draw_x_ticks(lines, mapping, bottom)
    ideal = [(mapping[t], linear(t, 0, max_threads, top, bottom)) for t in threads]
    lines.append(polyline(ideal, MUTED, 1.4, dashed=True))
    speed = [(mapping[t], linear(series["cps"][i] / base_cps, 0, max_threads, top, bottom))
             for i, t in enumerate(threads)]
    lines.append(polyline(speed, BLUE, 2.4))
    for x, y in speed:
        lines.append(dot(x, y, BLUE))
    lines.append(text(speed[-1][0] - 4, speed[-1][1] - 10,
                      f"{final_speedup:.2f}x · {final_efficiency:.0f}%", 10.5, BLUE,
                      anchor="end", weight="700", family=MONO))

    # --- Panel C: latency -------------------------------------------------------------------
    cx, cy, cw, ch = 24, 448, 584, 340
    lines += panel(cx, cy, cw, ch, "Per-chunk latency",
                   f"worker-side Instant per chunk over {chunks} chunks; median across trials")
    x0, x1, top, bottom = cx + 62, cx + cw - 40, cy + 66, cy + ch - 44
    latency_max = math.ceil(max(series["p99"]) * 1.08 / 6) * 6
    draw_axes(lines, x0, x1, top, bottom, latency_max, 4, label_fmt=lambda v: f"{v:.0f}", y_label="ms")
    mapping = x_axis(threads, x0, x1)
    draw_x_ticks(lines, mapping, bottom)
    for key, color, label in (("p50", GREEN, "p50"), ("p95", BLUE, "p95"), ("p99", PINK, "p99")):
        points = [(mapping[t], linear(series[key][i], 0, latency_max, top, bottom))
                  for i, t in enumerate(threads)]
        lines.append(polyline(points, color, 2.0))
        for x, y in points:
            lines.append(dot(x, y, color, 2.4))
    for i, (label, color) in enumerate((("p50", GREEN), ("p95", BLUE), ("p99", PINK))):
        ly = bottom - 46 + i * 15
        lines.append(
            f'<line x1="{x0 + 10}" y1="{ly}" x2="{x0 + 26}" y2="{ly}" '
            f'stroke="{color}" stroke-width="2" stroke-linecap="round"/>'
        )
        lines.append(text(x0 + 32, ly + 3.5, label, 9.5, color, weight="700", family=MONO))
    lines.append(text(x0 + 2, top + 12, f"1T p50 {series['p50'][0]:.2f} ms · p99 {series['p99'][0]:.2f} ms",
                      9.5, MUTED, family=MONO))
    lines.append(text(x1 - 4, top + 12,
                      f"{max_threads}T p50 {series['p50'][-1]:.2f} ms · p99 {series['p99'][-1]:.2f} ms",
                      9.5, MUTED, anchor="end", family=MONO))

    # --- Panel D: efficiency ----------------------------------------------------------------
    dx, dy, dw, dh = 624, 448, 592, 340
    lines += panel(dx, dy, dw, dh, "Parallel efficiency",
                   "speedup ÷ threads; 12 logical CPUs are 6 physical cores × 2 SMT")
    x0, x1, top, bottom = dx + 58, dx + dw - 20, dy + 66, dy + dh - 44
    draw_axes(lines, x0, x1, top, bottom, 100, 4, label_fmt=lambda v: f"{v:.0f}%")
    mapping = x_axis(threads, x0, x1)
    draw_x_ticks(lines, mapping, bottom)
    bar_w = (x1 - x0) / len(threads) * 0.58
    for i, t in enumerate(threads):
        efficiency = series["cps"][i] / base_cps / t * 100.0
        y = linear(efficiency, 0, 100, top, bottom)
        color = ORANGE if t in (1, max_threads) else "#7c5a3a"
        lines.append(
            f'<rect x="{mapping[t] - bar_w / 2:.1f}" y="{y:.1f}" width="{bar_w:.1f}" '
            f'height="{bottom - y:.1f}" rx="2.5" fill="{color}"/>'
        )
        if t in (1, 2, 4, 8, max_threads):
            lines.append(text(mapping[t], y - 6, f"{efficiency:.0f}%", 9, TEXT,
                              anchor="middle", family=MONO))

    # --- Panel E: dimensions ----------------------------------------------------------------
    ex, ey, ew, eh = 24, 804, 1192, 280
    lines += panel(ex, ey, ew, eh, "Dimension throughput",
                   f"median of 3 trials × 128 chunks at 1 / 4 / 12 threads; column heights differ by dimension")
    x0, x1, top, bottom = ex + 70, ex + ew - 20, ey + 66, ey + eh - 44
    dim_names = [name for name in ("Overworld", "Nether", "End") if name in dims]
    all_values = [value for name in dim_names for value in dims[name].values()]
    y_max = math.ceil(max(all_values) * 1.12 / 3000) * 3000
    draw_axes(lines, x0, x1, top, bottom, y_max, 4, label_fmt=lambda v: f"{v:,.0f}", y_label="chunks/s")
    group_w = (x1 - x0) / len(dim_names)
    colors = {1: ORANGE, 4: BLUE, 12: GREEN}
    dim_threads = [1, 4, 12]
    for gi, name in enumerate(dim_names):
        center = x0 + group_w * (gi + 0.5)
        for ti, t in enumerate(dim_threads):
            value = dims[name].get(t, 0.0)
            bar_w = group_w * 0.17
            x = center + (ti - 1) * bar_w * 1.25 - bar_w / 2
            y = linear(value, 0, y_max, top, bottom)
            lines.append(
                f'<rect x="{x:.1f}" y="{y:.1f}" width="{bar_w:.1f}" height="{bottom - y:.1f}" '
                f'rx="2.5" fill="{colors[t]}"/>'
            )
            lines.append(text(x + bar_w / 2, y - 6, fmt(value), 9.2, TEXT,
                              anchor="middle", family=MONO))
        lines.append(text(center, bottom + 18, name, 10.5, TEXT, anchor="middle", weight="700"))
    legend_x = x1 - 250
    lines.append(text(legend_x - 12, ey + 30, "threads", 9.5, MUTED, anchor="end"))
    for i, t in enumerate([1, 4, 12]):
        lx = legend_x + i * 78
        lines.append(f'<rect x="{lx}" y="{ey + 21}" width="9" height="9" rx="2" fill="{colors[t]}"/>')
        lines.append(text(lx + 14, ey + 29, f"{t}T", 9.5, TEXT))

    six_index = threads.index(6) if 6 in threads else 0
    six_speedup = series["cps"][six_index] / base_cps
    smt_gain = (final_cps / series["cps"][six_index] - 1) * 100.0
    latency_gain = (series["p50"][-1] / series["p50"][six_index] - 1) * 100.0
    lines.append(text(
        24, 1104,
        f"6T: {six_speedup:.2f}x at {series['cps'][six_index]:,.0f} chunks/s (p50 {series['p50'][six_index]:.2f} ms); "
        f"12T: {final_speedup:.2f}x at {final_cps:,.0f} chunks/s (p50 {series['p50'][-1]:.2f} ms). "
        f"SMT adds {smt_gain:.0f}% throughput for {latency_gain:.0f}% higher p50.",
        9.4, TEXT,
    ))
    lines.append(text(24, 1122, "Source: detailed-run/raw/worldgen_scaling.csv · "
                                 "generated by detailed-run/worldgen_charts.py", 9.2, MUTED))
    lines.append("</svg>")
    OUT_PATH.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"wrote {OUT_PATH}")
    print(
        f"{max_threads}T median {fmt(final_cps)} chunks/s; speedup {final_speedup:.2f}x; "
        f"efficiency {final_efficiency:.1f}%; {reps} trials"
    )


if __name__ == "__main__":
    main()
