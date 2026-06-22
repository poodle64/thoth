#!/usr/bin/env python3
"""One-shot backfill of Thoth's historical operational logs into Grafana Loki.

Thoth's local debug log (``~/.thoth/logs/thoth-debug.log``) holds months of
human-readable lines. This tool ships ONLY a curated, content-free subset
(timings, speed factors, recording lifecycle, errors) to a Loki endpoint so a
power user gets historical observability without ever sending transcript text.

Privacy model: an explicit ALLOW-LIST of message patterns, plus a hard DENY
net that drops any line carrying the transcript-content marker. The verbatim
``Transcribed N characters: '<text>'`` line is excluded by construction.

Timestamps: the log changed format partway through its life, so both are
parsed — early ``2026-02-16T03:07:24.701243Z`` (UTC) and later
``2026-06-22 09:40:23.914`` (local). Both convert to epoch-nanoseconds.

Loki ingest: pushes oldest->newest per stream (Loki enforces a per-stream
out-of-order window). The file is already chronological, so file order is
preserved per stream. Relax ``reject_old_samples_max_age`` on the Loki
instance before running a months-old backfill.

Usage:
    # preview what would ship — needs no endpoint, sends nothing
    python3 scripts/loki_backfill.py --dry-run

    # real push (operator supplies endpoint + token via env)
    LOKI_PUSH_URL=https://loki.example/loki/api/v1/push \\
    LOKI_AUTH="Bearer <token>" \\
    python3 scripts/loki_backfill.py
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import time
import urllib.error
import urllib.request
from collections import Counter
from datetime import datetime

DEFAULT_LOG = os.path.expanduser("~/.thoth/logs/thoth-debug.log")

# Two timestamp formats seen in the log's lifetime.
ISO_RE = re.compile(
    r"^(?P<ts>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+Z)\s+"
    r"(?P<level>TRACE|DEBUG|INFO|WARN|ERROR)\s+(?P<rest>.*)$"
)
LOCAL_RE = re.compile(
    r"^(?P<ts>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d+)\s+"
    r"(?P<level>TRACE|DEBUG|INFO|WARN|ERROR)\s+(?P<rest>.*)$"
)

# Hard DENY: the verbatim-transcript marker. Any line containing this is
# dropped regardless of level. This is the structural privacy guarantee.
TRANSCRIPT_MARKERS = ("characters: '", 'characters: "')

# ALLOW-LIST: substrings of valuable, content-free observability/performance
# events. WARN/ERROR lines are allowed wholesale (minus the DENY net) because
# operational failures are the point of the export.
ALLOW_SUBSTRINGS = (
    "Transcription took",
    "transcribed",  # "FluidAudio transcribed Ns audio in Ns (RTFx: N)" — speed factor
    "RTFx",
    "Recording started",
    "Recording stopped",
    "Silent recording",
    "Filtered text to",  # char count, no text
    "Enhanced text to",  # char count, no text
    "Enhancement failed",
    "Using configured audio device",
    "Whisper transcription service initialised",
    "model loaded",
    "Model loaded",
    "Metal GPU",
    "Database initialised",
    "Thoth starting",
)

HOME_RE = re.compile(r"/Users/[^/\s]+/")


def to_epoch_ns(ts: str) -> int:
    """Convert a parsed timestamp string to epoch-nanoseconds (UTC)."""
    if ts.endswith("Z"):
        dt = datetime.fromisoformat(ts.replace("Z", "+00:00"))  # aware UTC
    else:
        dt = datetime.strptime(ts, "%Y-%m-%d %H:%M:%S.%f")  # naive -> local
    return int(dt.timestamp()) * 1_000_000_000 + dt.microsecond * 1000


def classify(rest: str, level: str) -> bool:
    """Return True if this line should ship (content-free + valuable)."""
    if any(m in rest for m in TRANSCRIPT_MARKERS):
        return False  # hard deny — carries transcript text
    if level in ("WARN", "ERROR"):
        return True
    return any(s in rest for s in ALLOW_SUBSTRINGS)


def redact(rest: str) -> str:
    """Tidy home-directory paths so the literal username is not shipped."""
    return HOME_RE.sub("~/", rest)


def iter_matches(log_path: str, max_lines: int | None):
    """Yield (epoch_ns, level, message) for each allowed line."""
    seen = 0
    with open(log_path, "r", errors="replace") as fh:
        for line in fh:
            line = line.rstrip("\n")
            m = ISO_RE.match(line) or LOCAL_RE.match(line)
            if not m:
                continue
            level, rest = m.group("level"), m.group("rest")
            if not classify(rest, level):
                continue
            try:
                ns = to_epoch_ns(m.group("ts"))
            except ValueError:
                continue
            yield ns, level, redact(rest)
            seen += 1
            if max_lines and seen >= max_lines:
                return


def push_batch(url: str, headers: dict, streams: dict) -> None:
    """POST one batch of streams to Loki, retrying on 429/5xx."""
    payload = {
        "streams": [{"stream": labels, "values": values} for labels, values in streams]
    }
    body = json.dumps(payload).encode("utf-8")
    backoff = 1.0
    for attempt in range(6):
        req = urllib.request.Request(url, data=body, headers=headers, method="POST")
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                if resp.status in (200, 204):
                    return
        except urllib.error.HTTPError as e:
            if e.code in (429, 500, 502, 503) and attempt < 5:
                time.sleep(backoff)
                backoff = min(backoff * 2, 30)
                continue
            detail = e.read().decode("utf-8", "replace")[:300]
            raise SystemExit(f"Loki push failed {e.code}: {detail}")
        except urllib.error.URLError as e:
            if attempt < 5:
                time.sleep(backoff)
                backoff = min(backoff * 2, 30)
                continue
            raise SystemExit(f"Loki unreachable: {e}")
    raise SystemExit("Loki push failed after retries")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--log-path", default=DEFAULT_LOG)
    ap.add_argument("--dry-run", action="store_true", help="report only; send nothing")
    ap.add_argument("--batch-size", type=int, default=2000, help="log lines per push")
    ap.add_argument(
        "--max-lines", type=int, default=None, help="cap matched lines (testing)"
    )
    ap.add_argument("--throttle-ms", type=int, default=150, help="pause between pushes")
    args = ap.parse_args()

    if not os.path.exists(args.log_path):
        raise SystemExit(f"Log not found: {args.log_path}")

    if args.dry_run:
        by_level = Counter()
        templates = Counter()
        first_ns = last_ns = None
        total = 0
        for ns, level, msg in iter_matches(args.log_path, args.max_lines):
            total += 1
            by_level[level] += 1
            first_ns = ns if first_ns is None else min(first_ns, ns)
            last_ns = ns if last_ns is None else max(last_ns, ns)
            # collapse numbers so templates group; first 60 chars only.
            templates[re.sub(r"\d+\.?\d*", "N", msg)[:60]] += 1
        print(f"matched {total} content-free lines from {args.log_path}")
        if first_ns:
            f = datetime.fromtimestamp(first_ns / 1e9)
            l = datetime.fromtimestamp(last_ns / 1e9)
            print(f"range: {f:%Y-%m-%d %H:%M} .. {l:%Y-%m-%d %H:%M}")
        print("by level:", dict(by_level))
        print("\ntop message templates that WOULD ship (numbers collapsed to N):")
        for tmpl, n in templates.most_common(25):
            print(f"  {n:>8}  {tmpl}")
        return 0

    url = os.environ.get("LOKI_PUSH_URL")
    if not url:
        raise SystemExit("Set LOKI_PUSH_URL (and optionally LOKI_AUTH, LOKI_TENANT)")
    headers = {"Content-Type": "application/json"}
    if os.environ.get("LOKI_AUTH"):
        headers["Authorization"] = os.environ["LOKI_AUTH"]
    if os.environ.get("LOKI_TENANT"):
        headers["X-Scope-OrgID"] = os.environ["LOKI_TENANT"]

    base_labels = {"job": "thoth-backfill", "app": "thoth"}
    # buffer per level so each Loki stream stays timestamp-ordered.
    buffers: dict[str, list] = {}
    pushed = 0
    pending = 0

    def flush():
        nonlocal pending
        streams = []
        for level, values in buffers.items():
            if values:
                labels = dict(base_labels, level=level.lower())
                streams.append((labels, values))
        if streams:
            push_batch(url, headers, streams)
            for v in buffers.values():
                v.clear()
            time.sleep(args.throttle_ms / 1000.0)
        pending = 0

    for ns, level, msg in iter_matches(args.log_path, args.max_lines):
        buffers.setdefault(level, []).append([str(ns), msg])
        pending += 1
        pushed += 1
        if pending >= args.batch_size:
            flush()
        if pushed % 20000 == 0:
            print(f"  pushed {pushed} lines...", file=sys.stderr)
    flush()
    print(f"done — pushed {pushed} content-free lines to Loki")
    return 0


if __name__ == "__main__":
    sys.exit(main())
