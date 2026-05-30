# Recording State Machine (the simple model)

Two sounds, named so we can tell them apart:

- **bing** = the START sound (you pressed, recording is now capturing)
- **bong** = the STOP sound (you pressed, capture has ended)

## The intended model — one press toggles, sound matches the action

```text
                press (shift)                press (shift)
                  ┌──────┐                     ┌──────┐
                  ▼      │                     ▼      │
            ┌──────────┐ │  press → play BING  ┌──────────┐
            │          │ │  + arm capture      │          │
   start ──▶│   IDLE   │─┼────────────────────▶│ RECORDING│
            │          │ │                     │ (capturing)
            └──────────┘ │                     └────┬─────┘
                  ▲       │                          │ press → play BONG
                  │       │                          │ + stop capture
                  │       │                          ▼
                  │       │                    ┌──────────────┐
                  │       │   (background,     │  PROCESSING  │
                  │       └────  NOT blocking  │ transcribe → │
                  │            a new start) ◀──│ paste → save │
                  │                            └──────┬───────┘
                  └───────────────────────────────────┘
                         processing finishes
                         (silent — no sound)
```

Key idea: **PROCESSING is a background activity, not a state that blocks the user.**
From the user's point of view there are only two things that matter:

- Am I capturing audio right now? → RECORDING
- Am I not? → IDLE (even if a previous clip is still transcribing in the background)

So the **only** input is "press", and the rule is dead simple:

| You are…              | You press → | Sound | Action                       |
| --------------------- | ----------- | ----- | ---------------------------- |
| IDLE (not capturing)  | start       | BING  | arm capture                  |
| RECORDING (capturing) | stop        | BONG  | stop capture, hand off to bg |

PROCESSING in the background does **not** change what a press does. If you press
while a previous clip is still transcribing, you are still "not capturing" →
press = BING + start a new capture. The old clip keeps finishing on its own.

## Why we currently get the wrong sound ("double bing")

Today the **sound is chosen in the wrong place** by the wrong question.

- The START sound is played by the Rust keypress handler the instant you press,
  gated on a backend flag (`!is_pipeline_running && !is_recording`).
- But the actual decision "is this press a start or a stop?" is made later, in
  the frontend, using a _different_ notion of "busy".

These two disagree during the ~1–2s after you stop, while the background
transcribe+paste runs:

```text
press to STOP ─▶ capture ends ─▶ backend flag clears ─▶ (transcribe + paste, ~1–2s) ─▶ done
                                        │
                    if you press here ──┘
                    Rust sees "backend free" → plays BING (start sound!)
                    Frontend sees "still busy" → drops the start
                    Net: BING sound, but no recording → the false "double bing"
```

The press lands in a seam where the two halves of the app disagree about
whether you're busy. The sound says "started!", the recording says "nope".

## The fix (what makes it match the simple model)

The state machine above only has **one** authority for "am I capturing?": the
recorder's armed flag. The sound must be chosen from the _same_ authority and at
the _same_ moment as the start/stop decision — not from a separate flag in a
separate place that can disagree.

Two clean ways to honour the model:

1. **Sound follows the action, decided in one place.** Whoever decides
   "this press starts" plays BING; whoever decides "this press stops" plays
   BONG. One decision, one sound, no second opinion. PROCESSING never gates a
   start — pressing while a background clip finishes is a normal IDLE→RECORDING
   start (BING), exactly as the table says.

2. PROCESSING must not look like "busy" to the start decision. The only thing
   that blocks a _start_ is already capturing (RECORDING). Background work is
   invisible to the user's next press.

Both reduce to the same principle: **there is one question — "am I capturing
right now?" — and both the sound and the action must read the same answer to it
at the same instant.** Everything hard about this came from having two flags
answering two slightly different questions a beat apart.
