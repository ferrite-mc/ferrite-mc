#!/usr/bin/env python3
"""Render the fresh before/after spawn-region TPS chart."""
OUT = "/home/ubuntu/orca/workspaces/ferrite-mc/parallel-4oy8j-1/docs/perf/2026-11-23/spawn-region-20tps.svg"
FONT = "ui-sans-serif,system-ui,sans-serif"
MONO = "ui-monospace,monospace"
W, H = 960, 430
BOTTOM, TOP = 334.0, 214.0


def y(v):
    return BOTTOM - (v / 20.0) * (BOTTOM - TOP)


def bar(x, v, color, opacity=1.0, width=38):
    yy = y(v)
    return [
        f'<rect x="{x}" y="{yy:.1f}" width="{width}" height="{BOTTOM-yy:.1f}" rx="4" fill="{color}" opacity="{opacity}"/>',
        f'<text x="{x+width/2:.1f}" y="{yy-5:.1f}" text-anchor="middle" fill="#e6edf3" font-family="{MONO}" font-size="9">{v:.1f}</text>',
    ]


def main():
    p = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}">',
        f'<rect width="{W}" height="{H}" rx="14" fill="#0d1117"/>',
        f'<text x="28" y="36" fill="#e6edf3" font-family="{FONT}" font-size="19" font-weight="800">Ferrite spawn region: 20 TPS at 50–400 bots</text>',
        f'<text x="28" y="57" fill="#8b949e" font-family="{FONT}" font-size="11.5">Fresh seed=0 · BOTS_STILL=1 · view=10 · 5 ms stagger · 10 s settle + 30 s walk · Region 0 TPS</text>',
        '<rect x="596" y="28" width="9" height="9" rx="2" fill="#8b949e"/>',
        f'<text x="610" y="36" fill="#c9d1d9" font-family="{FONT}" font-size="10">before (254c444d)</text>',
        '<rect x="738" y="28" width="9" height="9" rx="2" fill="#f97316"/>',
        f'<text x="752" y="36" fill="#c9d1d9" font-family="{FONT}" font-size="10">after (5e76edd2)</text>',
        '<rect x="28" y="82" width="904" height="300" rx="10" fill="#161b22" stroke="#30363d"/>',
        f'<text x="48" y="108" fill="#e6edf3" font-family="{FONT}" font-size="13.5" font-weight="700">Server-reported spawn-region TPS</text>',
        f'<text x="48" y="126" fill="#8b949e" font-family="{FONT}" font-size="10.5">Higher is better · 20 TPS target line</text>',
        '<line x1="70" y1="334" x2="920" y2="334" stroke="#30363d"/>',
        '<line x1="70" y1="214" x2="920" y2="214" stroke="#f97316" stroke-dasharray="4,3" opacity="0.55"/>',
        f'<text x="62" y="217" text-anchor="end" fill="#f97316" font-family="{MONO}" font-size="8.5">20</text>',
        '<line x1="70" y1="274" x2="920" y2="274" stroke="#21262d" stroke-dasharray="3,3"/>',
        f'<text x="62" y="277" text-anchor="end" fill="#6e7681" font-family="{MONO}" font-size="8.5">10</text>',
        f'<text x="62" y="337" text-anchor="end" fill="#6e7681" font-family="{MONO}" font-size="8.5">0</text>',
    ]
    groups = [
        (122, "50 bots", None, 20.0),
        (292, "100 bots", None, 20.0),
        (482, "200 bots", 16.6, 20.0),
        (672, "400 bots", 12.3, 20.0),
        (852, "400 bots · 90 s", 14.8, 20.0),
    ]
    for cx, label, before, after in groups:
        if before is None:
            p += bar(cx - 19, after, "#f97316")
        else:
            p += bar(cx - 40, before, "#8b949e", 0.7, width=34)
            p += bar(cx + 6, after, "#f97316", width=34)
        p.append(f'<text x="{cx}" y="354" text-anchor="middle" fill="#c9d1d9" font-family="{FONT}" font-size="10">{label}</text>')
    p += [
        f'<text x="48" y="376" fill="#8b949e" font-family="{FONT}" font-size="9">Before: earlier 254c444d runs. After: fresh 5e76edd2 runs, one observation each. The 90 s run profiles after 60 s of walking.</text>',
        "</svg>",
    ]
    with open(OUT, "w", encoding="utf-8") as f:
        f.write("\n".join(p) + "\n")
    print("wrote", OUT)


if __name__ == "__main__":
    main()
