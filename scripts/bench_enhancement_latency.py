#!/usr/bin/env python3
"""Benchmark local Ollama enhancement latency at Thoth's real entry-size distribution.

Thoth's AI-enhancement stage (off by default) sends the transcript to a local
Ollama model on the critical path: when enabled, text only reaches the cursor
after this round-trip. This harness measures that round-trip so the cost can be
compared over time as faster/better models arrive.

It measures, per (model, input size):
  - cold total latency  (model freshly loaded into memory, i.e. first dictation
    after Ollama idle-unloads it)
  - warm total latency   (model resident — the steady state if keep_alive holds it)
  - generation throughput (eval tokens/sec)
  - the one-time model load penalty (load_duration)

Input sizes are fixed synthetic dictation-style samples at Thoth's measured
word-count percentiles (p50=36, p90=123, p99=341 words as of 2026-06-24), so the
numbers map onto real usage. The prompt is Thoth's actual default enhancement
prompt (see src-tauri/src/pipeline.rs DEFAULT_ENHANCEMENT_PROMPT).

Results append to:
  docs/development/benchmarks/enhancement-latency.jsonl   (machine-readable, one row/run)
  docs/development/benchmarks/enhancement-latency.md       (human-readable, one section/run)

Usage:
  python3 scripts/bench_enhancement_latency.py [model ...]
Default models: llama3.2 llama3.2:1b qwen2.5:1.5b
Requires a running local Ollama (http://localhost:11434).
"""

from __future__ import annotations

import json
import subprocess
import sys
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from statistics import median

OLLAMA = "http://localhost:11434"
WARM_N = 5
DEFAULT_MODELS = ["llama3.2", "llama3.2:1b", "qwen2.5:1.5b"]

# Thoth's actual default enhancement prompt (pipeline.rs DEFAULT_ENHANCEMENT_PROMPT).
PROMPT_TEMPLATE = (
    "Fix grammar and punctuation in the following text.\n"
    "Keep the original meaning and tone. Output only the corrected text, nothing else.\n\n"
    "Text: {text}"
)

# One run-on dictation-style monologue (technical, lightly punctuated, no personal
# data), sliced to Thoth's measured word-count percentiles so each sample hits the
# size exactly and the labels stay honest.
BASE_TEXT = (
    "so the thing i keep coming back to is that the enhancement pass currently sits "
    "right on the critical path which means when it is switched on the transcribed "
    "text only reaches the cursor after the local model has finished its round trip "
    "and that round trip is the whole question because the deterministic filter we "
    "have today is fast and faithful but it cannot fix the spurious capitals that the "
    "model produces at the segment seams since it has no idea which words are genuine "
    "proper nouns whereas a language model actually understands the surrounding "
    "context and would get the casing right almost every single time the catch of "
    "course is latency because the model first has to be resident in memory and then "
    "it has to generate roughly the same number of tokens as the input so for a short "
    "dictation fired into a chat box or a search field it might feel basically instant "
    "provided the model is already warm but for a longer paragraph the generation time "
    "starts to dominate and you really begin to feel the lag which is exactly why i "
    "want hard numbers across a handful of fast models measured at the sizes we "
    "genuinely dictate rather than guessing so we can decide whether a small warm "
    "model is good enough to hide behind an optional polish mode that the user reaches "
    "for deliberately or whether we keep the model strictly off the cursor path and "
    "only ever enhance into the history view where the latency simply does not matter "
    "and the other consideration worth holding in mind is that a small local model can "
    "quietly drift on technical terms so if it lowercases a product name or rewrites a "
    "phrase we lose the faithfulness that makes the deterministic path trustworthy in "
    "the first place which is precisely why the default prompt is mechanical and asks "
    "the model to preserve meaning and tone instead of rephrasing but even a mechanical "
    "prompt occasionally changes a word so we have to weigh that risk against the casing "
    "wins and the whole point of saving this baseline today is that the local model "
    "landscape keeps moving so quickly that what looks far too slow right now might be "
    "perfectly snappy in six months on the very same hardware with a newer smaller "
    "distilled model and that is a comparison worth being able to make properly later"
)
_WORDS = BASE_TEXT.split()
# Thoth's measured percentiles (2026-06-24, n=19207): p50=36, p90=123, p99=341 words.
SAMPLES = {
    "p50_36w": " ".join(_WORDS[:36]),
    "p90_123w": " ".join(_WORDS[:123]),
    "p99_341w": " ".join(_WORDS[:341]),
}
assert len(_WORDS) >= 341, f"BASE_TEXT too short: {len(_WORDS)} words"


def wc(text: str) -> int:
    return len(text.split())


def ollama_version() -> str:
    try:
        out = subprocess.run(
            ["ollama", "--version"], capture_output=True, text=True, timeout=10
        ).stdout.strip()
        return out.replace("ollama version is ", "").strip() or "unknown"
    except Exception:
        return "unknown"


def chip() -> str:
    try:
        return subprocess.run(
            ["sysctl", "-n", "machdep.cpu.brand_string"],
            capture_output=True,
            text=True,
            timeout=10,
        ).stdout.strip()
    except Exception:
        return "unknown"


def installed_models() -> set[str]:
    try:
        with urllib.request.urlopen(f"{OLLAMA}/api/tags", timeout=10) as r:
            data = json.load(r)
        return {m["name"] for m in data.get("models", [])} | {
            m["name"].split(":")[0] for m in data.get("models", [])
        }
    except Exception:
        return set()


def stop_model(model: str) -> None:
    """Unload the model from memory so the next call measures a cold load."""
    subprocess.run(
        ["ollama", "stop", model], capture_output=True, text=True, timeout=30
    )
    time.sleep(0.5)


def generate(model: str, text: str) -> dict:
    """One /api/generate call. Returns Ollama's nanosecond timing breakdown."""
    body = json.dumps(
        {
            "model": model,
            "prompt": PROMPT_TEMPLATE.format(text=text),
            "stream": False,
            "keep_alive": "5m",
            "options": {"temperature": 0, "num_predict": 1024},
        }
    ).encode()
    req = urllib.request.Request(
        f"{OLLAMA}/api/generate",
        data=body,
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=300) as r:
        d = json.load(r)
    ns = 1_000_000  # ns -> ms
    return {
        "total_ms": d.get("total_duration", 0) / ns,
        "load_ms": d.get("load_duration", 0) / ns,
        "prompt_eval_ms": d.get("prompt_eval_duration", 0) / ns,
        "eval_ms": d.get("eval_duration", 0) / ns,
        "eval_count": d.get("eval_count", 0),
        "prompt_eval_count": d.get("prompt_eval_count", 0),
    }


def bench_model(model: str) -> list[dict]:
    rows = []
    print(f"\n### {model}", flush=True)
    stop_model(model)
    # Cold: first call after unload — total includes the model load penalty.
    first_size = "p50_36w"
    cold = generate(model, SAMPLES[first_size])
    print(
        f"  cold {first_size}: total={cold['total_ms']:.0f}ms load={cold['load_ms']:.0f}ms",
        flush=True,
    )
    for size, text in SAMPLES.items():
        warm = [generate(model, text) for _ in range(WARM_N)]
        warm_total = median(w["total_ms"] for w in warm)
        warm_eval = median(w["eval_ms"] for w in warm)
        eval_count = median(w["eval_count"] for w in warm)
        tok_s = (eval_count / (warm_eval / 1000)) if warm_eval > 0 else 0
        rows.append(
            {
                "model": model,
                "size": size,
                "input_words": wc(text),
                "cold_total_ms": round(cold["total_ms"])
                if size == first_size
                else None,
                "load_ms": round(cold["load_ms"]) if size == first_size else None,
                "warm_total_ms": round(warm_total),
                "warm_eval_tok_s": round(tok_s, 1),
                "out_tokens": int(eval_count),
            }
        )
        print(
            f"  warm {size} ({wc(text)}w): total={warm_total:.0f}ms  {tok_s:.0f} tok/s  out={int(eval_count)}tok",
            flush=True,
        )
    return rows


def main() -> int:
    models = sys.argv[1:] or DEFAULT_MODELS
    avail = installed_models()
    if not avail:
        print("ERROR: cannot reach Ollama at " + OLLAMA, file=sys.stderr)
        return 1
    todo = [m for m in models if m in avail]
    skipped = [m for m in models if m not in avail]
    if skipped:
        print(f"Skipping (not installed): {', '.join(skipped)}")
    if not todo:
        print("ERROR: none of the requested models are installed.", file=sys.stderr)
        return 1

    stamp = datetime.now(timezone.utc).astimezone()
    meta = {
        "date": stamp.strftime("%Y-%m-%d %H:%M %Z"),
        "chip": chip(),
        "ollama": ollama_version(),
        "warm_n": WARM_N,
    }
    print(f"Benchmark {meta['date']} | {meta['chip']} | {meta['ollama']}")

    all_rows = []
    for m in todo:
        all_rows.extend(bench_model(m))

    out_dir = Path(__file__).resolve().parent.parent / "docs/development/benchmarks"
    out_dir.mkdir(parents=True, exist_ok=True)
    jsonl = out_dir / "enhancement-latency.jsonl"
    with jsonl.open("a") as f:
        for row in all_rows:
            f.write(json.dumps({**meta, **row}) + "\n")

    md = out_dir / "enhancement-latency.md"
    write_markdown(md, meta, all_rows)
    print(f"\nSaved: {jsonl}\n       {md}")
    return 0


def write_markdown(path: Path, meta: dict, rows: list[dict]) -> None:
    header = (
        "# Enhancement Latency Benchmark\n\n"
        "Local Ollama enhancement round-trip latency at Thoth's real entry-size\n"
        "distribution. Enhancement (off by default) runs on the critical path: when\n"
        "enabled, text reaches the cursor only after this round-trip. For reference,\n"
        'the current transcription baseline (the "razor fast" feel) is p50 ~270ms,\n'
        "p90 ~720ms; entry word counts are p50=36, p90=123, p99=341.\n\n"
        "Regenerate / append a run: `python3 scripts/bench_enhancement_latency.py`\n"
    )
    section = [
        f"\n## Run {meta['date']}",
        f"\n- Hardware: {meta['chip']}",
        f"- Ollama: {meta['ollama']}  ·  warm runs/median: {meta['warm_n']}",
        "\n| model | input | cold total | load | warm total | tok/s | out tok |",
        "|---|---|---|---|---|---|---|",
    ]
    for r in rows:
        cold = f"{r['cold_total_ms']}ms" if r["cold_total_ms"] is not None else "—"
        load = f"{r['load_ms']}ms" if r["load_ms"] is not None else "—"
        section.append(
            f"| {r['model']} | {r['size']} ({r['input_words']}w) | {cold} | {load} "
            f"| {r['warm_total_ms']}ms | {r['warm_eval_tok_s']} | {r['out_tokens']} |"
        )
    existing = path.read_text() if path.exists() else header
    if not existing.startswith("# Enhancement Latency Benchmark"):
        existing = header + existing
    path.write_text(existing + "\n".join(section) + "\n")


if __name__ == "__main__":
    raise SystemExit(main())
