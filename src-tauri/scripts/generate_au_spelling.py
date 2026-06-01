#!/usr/bin/env python3
"""Generate the US->AU spelling map from the vendored VARCON data.

Reads ``data/varcon/varcon.txt`` (the canonical English Speller Database
variant-conversion file, the same data that generates the en_AU Hunspell
dictionary shipped in browsers and office suites) and emits a sorted Rust
``&[(&str, &str)]`` table to ``src/transcription/au_spelling_map.rs``.

This is run by hand when the vendored VARCON version changes; the generated
file is committed so there is no build-time codegen. Run from ``src-tauri/``:

    python3 scripts/generate_au_spelling.py

VARCON format (see data/varcon/README):
  - Each non-comment line is a cluster of variant spellings joined by ' / '.
  - Each variant is ``TAGS: word`` where TAGS are space-separated dialect tags.
  - Dialect categories: A=American, B=British "-ise", Z=British "-ize"/OED,
    C=Canadian, D=Australian. A bare tag letter is the *preferred* spelling for
    that dialect; a suffixed tag (Xv, XV, X-, Xx) marks a variant.
  - Fallback rule (README): "If there are no 'D' tags then 'B' implies 'D'."
    So the Australian spelling is the bare-D variant, or the bare-B variant when
    no D is present.
  - ``(level NN)`` on the ``#`` header line is the SCOWL frequency band; we keep
    clusters at level <= 60 (common, dictionary-grade words), dropping the rare
    and archaic long tail that is hazardous to auto-apply to dictated speech.
"""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VARCON = ROOT / "data" / "varcon" / "varcon.txt"
OUT = ROOT / "src" / "transcription" / "au_spelling_map.rs"

# SCOWL frequency cutoff. <=60 keeps common, dictionary-grade words and drops
# the rare/archaic long tail (level>60) that would mis-fire on dictated speech.
MAX_LEVEL = 60

# Homograph / sense / register hazards: the US spelling is itself a common word
# whose blind conversion is wrong more often than right in dictated English
# (noun/verb splits, verbs that collide with a noun-only AU respelling, register
# shifts). Excluded so the converter never makes a confident-but-wrong change.
DENYLIST = {
    "tire",
    "tires",
    "tired",
    "tiring",  # tire = fatigue verb (vs tyre)
    "curb",
    "curbs",
    "curbed",
    "curbing",  # curb = restrain verb (vs kerb)
    "sake",  # "for the sake of" (vs saki)
    "mat",
    "mats",  # a mat (vs matt finish)
    "prize",
    "prizes",
    "prized",
    "prizing",  # reward (vs prise = lever)
    "practice",
    "practices",  # noun stays; only the verb is practise
    "license",
    "licenses",  # noun is licence; verb license — ambiguous
    "draft",
    "drafts",
    "drafted",
    "drafting",  # draft doc/bank (vs draught beer/air)
    "story",
    "stories",  # tale (vs storey = floor)
    "check",
    "checks",
    "checked",
    "checking",  # check (vs cheque)
    "ass",
    "asses",  # register / meaning
    "mom",
    "moms",  # deliberate word
    "czar",
    "czars",  # "data czar" etc.
    "meow",
    "meows",
    "meowed",  # standard in AU
    "gibe",
    "gibes",
    "jibe",
    "jibes",  # rare / ambiguous
    "whiz",
    "whizz",  # rare
    "program",
    "programs",  # AU accepts "program" (esp. computing)
    # ── Second-pass hazards (found auditing the generated map) ──
    "meter",
    "meters",  # instrument sense (power/VU/parking meter) stays; only the unit is metre
    "checker",
    "checkers",
    "checkered",
    "checkering",
    "checkerboard",
    "checkbook",  # spell-checker / pattern / validator — not the money sense
    "dependent",
    "dependents",  # adjective is "dependent" in AU; only the noun is "dependant"
    "descendant",
    "descendants",  # "descendant" IS the correct AU spelling — map would degrade it
    "caster",
    "casters",  # caster sugar / caster wheel (vs castor oil)
    "racket",
    "rackets",  # noise / scam sense dominates (vs racquet)
    "matte",  # finish / matte painting (vs the name "Matt")
    "learned",  # adjective "a learned professor" must not become "learnt"
    # Orthographically broken VARCON generation artefacts (not real words):
    "perv",
    "costumier",
    "mynas",
    "colorrest",
}

# NB: VARCON also generates bogus comparatives/superlatives of non-gradable
# adjectives ("behaviouraler", "meagrer", "gonorrhoealer", "anaemicest"…). These
# are harmless — nobody dictates them, so they never fire — and any structural
# filter broad enough to catch them also drops legitimate agent-nouns
# (labourer, signaller, teetotaller). They are left in rather than risk a
# false-positive drop of a real word.

LEVEL_RE = re.compile(r"\(level (\d+)\)")
ALPHA_RE = re.compile(r"[a-z]+")


def parse_variants(line):
    """Split a cluster line into [(spelling, [tags])], dropping trailing usage."""
    core = line.split(" | ", 1)[0]
    variants = []
    for part in core.split(" / "):
        if ":" in part:
            tags, word = part.split(":", 1)
            variants.append((word.strip(), tags.split()))
    return variants


def bare_word(variants, category):
    """Return the spelling carrying the *bare* (preferred) tag for a category."""
    for word, tags in variants:
        if category in tags:  # exact match = preferred, not a variant
            return word
    return None


def build():
    pairs = {}
    level = None
    with open(VARCON, encoding="latin-1") as handle:
        for raw in handle:
            line = raw.rstrip("\n")
            if line.startswith("#"):
                m = LEVEL_RE.search(line)
                level = int(m.group(1)) if m else None
                continue
            if not line.strip():
                continue
            if level is not None and level > MAX_LEVEL:
                continue
            variants = parse_variants(line)
            us = bare_word(variants, "A")
            if us is None:
                continue
            au = bare_word(variants, "D") or bare_word(variants, "B")
            if au is None or au == us:
                continue
            if not ALPHA_RE.fullmatch(us) or not ALPHA_RE.fullmatch(au):
                continue
            if us in DENYLIST:
                continue
            pairs.setdefault(us, au)  # first (headword) mapping wins
    return pairs


def main():
    pairs = build()
    rows = sorted(pairs.items())

    lines = [
        "//! US -> Australian spelling map (GENERATED — do not edit by hand).",
        "//!",
        "//! Regenerate with `python3 scripts/generate_au_spelling.py` after",
        "//! updating `data/varcon/varcon.txt`. Source: the English Speller",
        "//! Database (VARCON/SCOWL), the same data behind the en_AU Hunspell",
        "//! dictionary. Licence/attribution: see `data/varcon/README` and the",
        "//! NOTICES entry. Filtered to SCOWL level <= 60 with a denylist of",
        "//! homograph hazards (see the generator).",
        "",
        "/// US -> AU spelling pairs, sorted by US spelling for binary search.",
        f"pub(crate) static AU_SPELLING_MAP: &[(&str, &str)] = &[",
    ]
    for us, au in rows:
        lines.append(f'    ("{us}", "{au}"),')
    lines.append("];")
    lines.append("")

    OUT.write_text("\n".join(lines), encoding="utf-8")
    print(f"wrote {OUT.relative_to(ROOT)}: {len(rows)} pairs")


if __name__ == "__main__":
    main()
