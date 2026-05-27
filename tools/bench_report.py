#!/usr/bin/env python3
"""Render Divan benchmark output as a small self-contained HTML report."""

from __future__ import annotations

import html
import json
import re
import sys
from dataclasses import asdict, dataclass
from pathlib import Path


UNIT_SCALE = {
    "ps": 1e-3,
    "ns": 1.0,
    "\u00b5s": 1e3,
    "us": 1e3,
    "ms": 1e6,
    "s": 1e9,
}

ROW_RE = re.compile(
    r"^(?P<prefix>(?:[│ ]  )*)(?P<branch>[├╰]─ )(?P<name>\S+)"
    r"(?P<rest>.*)$"
)
TIME_RE = re.compile(r"(?P<value>\d+(?:\.\d+)?)\s*(?P<unit>ps|ns|\u00b5s|us|ms|s)")


@dataclass
class BenchRow:
    path: list[str]
    fastest_ns: float
    slowest_ns: float
    median_ns: float
    mean_ns: float
    samples: int
    iters: int


def depth_of(prefix: str) -> int:
    return len(prefix) // 3


def parse_time(cell: str) -> float:
    match = TIME_RE.search(cell.strip())
    if match is None:
        raise ValueError(f"missing time in cell: {cell!r}")
    return float(match.group("value")) * UNIT_SCALE[match.group("unit")]


def parse_rows(text: str) -> list[BenchRow]:
    stack: list[str] = []
    rows: list[BenchRow] = []

    for line in text.splitlines():
        match = ROW_RE.match(line)
        if match is None:
            continue

        depth = depth_of(match.group("prefix"))
        name = match.group("name")
        rest = match.group("rest")
        cells = [cell.strip() for cell in rest.split("│")]
        has_measurements = len(cells) >= 6 and TIME_RE.search(cells[0]) is not None

        stack = stack[:depth]
        if has_measurements:
            rows.append(
                BenchRow(
                    path=stack + [name],
                    fastest_ns=parse_time(cells[0]),
                    slowest_ns=parse_time(cells[1]),
                    median_ns=parse_time(cells[2]),
                    mean_ns=parse_time(cells[3]),
                    samples=int(cells[4]),
                    iters=int(cells[5]),
                )
            )
        else:
            stack.append(name)

    return rows


def format_ns(value: float) -> str:
    if value >= 1e6:
        return f"{value / 1e6:.3g} ms"
    if value >= 1e3:
        return f"{value / 1e3:.3g} us"
    return f"{value:.3g} ns"


def render_html(rows: list[BenchRow]) -> str:
    max_mean = max((row.mean_ns for row in rows), default=1.0)
    table_rows = []
    for row in sorted(rows, key=lambda item: item.path):
        path = "/".join(row.path)
        width = max(1.0, row.mean_ns / max_mean * 100.0)
        table_rows.append(
            "<tr>"
            f"<td><code>{html.escape(path)}</code></td>"
            f"<td>{format_ns(row.fastest_ns)}</td>"
            f"<td>{format_ns(row.median_ns)}</td>"
            f"<td>{format_ns(row.mean_ns)}</td>"
            f"<td>{row.samples}</td>"
            f"<td><div class=\"bar\" style=\"width:{width:.2f}%\"></div></td>"
            "</tr>"
        )

    payload = json.dumps([asdict(row) for row in rows], indent=2)
    return f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>binette benchmark report</title>
  <style>
    :root {{
      color-scheme: light dark;
      font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }}
    body {{
      margin: 32px;
      line-height: 1.4;
    }}
    table {{
      border-collapse: collapse;
      width: 100%;
    }}
    th, td {{
      border-bottom: 1px solid color-mix(in srgb, CanvasText 16%, transparent);
      padding: 8px 10px;
      text-align: left;
      vertical-align: middle;
    }}
    th {{
      font-size: 13px;
      text-transform: uppercase;
    }}
    code {{
      font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
      font-size: 13px;
    }}
    .bar {{
      height: 10px;
      min-width: 2px;
      background: #3b82f6;
      border-radius: 2px;
    }}
    details {{
      margin-top: 24px;
    }}
    pre {{
      overflow: auto;
      padding: 12px;
      background: color-mix(in srgb, CanvasText 8%, transparent);
    }}
  </style>
</head>
<body>
  <h1>binette benchmark report</h1>
  <table>
    <thead>
      <tr>
        <th>Benchmark</th>
        <th>Fastest</th>
        <th>Median</th>
        <th>Mean</th>
        <th>Samples</th>
        <th>Mean bar</th>
      </tr>
    </thead>
    <tbody>
      {"".join(table_rows)}
    </tbody>
  </table>
  <details>
    <summary>Parsed data</summary>
    <pre>{html.escape(payload)}</pre>
  </details>
</body>
</html>
"""


def main() -> int:
    text = sys.stdin.read()
    rows = parse_rows(text)
    if not rows:
        print("no Divan benchmark rows found", file=sys.stderr)
        return 1
    output = Path(sys.argv[1]) if len(sys.argv) > 1 else None
    report = render_html(rows)
    if output is None:
        sys.stdout.write(report)
    else:
        output.write_text(report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
