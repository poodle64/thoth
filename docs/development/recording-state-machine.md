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

The recorder's armed flag is the single authority for whether capture is active; both the start/stop action and its cue sound read that same flag at the same instant.
