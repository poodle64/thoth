---
name: Thoth
description: Privacy-first, offline-capable voice transcription application
version: 2026.7.0
status: "stub - not yet wired onto @poodle64/design-tokens"
---

## Current state

Thoth uses a hand-rolled dark-only theme called **Scribe Amber**, defined entirely in `src/app.css`. The palette is built around a warm amber primary (`oklch(0.693 0.124 65.9)`), dark brown backgrounds (deepest at `oklch(0.223 0.002 67.7)`), and muted stone neutrals. All colour tokens are expressed in OKLCH and mapped to the standard shadcn-svelte token slots (`--primary`, `--background`, `--muted`, etc.), then re-exported into Tailwind v4 via `@theme inline`. The radius base is `0.5rem` (`--radius`). Fonts are the system default; Fraunces, Hanken Grotesk, and JetBrains Mono from the shared design language are not yet applied. There is no `:root` / `.dark` divergence in Thoth; both selectors carry identical values.

**Open item:** The shared baseline specifies a radius base of `0.625rem` (`--ds-radius-lg`). Thoth currently uses `0.5rem`. Adoption of the shared tokens will close this drift.

## Shared baseline

The household design language and binding constants live in two places:

- `docs/master/design/shared-design-language.md` -- canonical source for colour space (OKLCH), radius (`--ds-radius-lg: 0.625rem`), typography (Fraunces display, Hanken Grotesk body, JetBrains Mono code), status vocabulary (success / warning / error / info / neutral), spacing, and namespace (`--ds-*`).
- `@poodle64/design-tokens` (GitHub Packages, public) -- the compiled CSS custom-property package that projects consume. Provides the `--ds-*` token set and the wiring snippet for `app.css`.

## Adoption path

1. Copy `templates/DESIGN.md.template` from the `@poodle64/design-tokens` package into the repo root and fill in the project-specific fields.
2. Install the package: `pnpm add -D @poodle64/design-tokens`.
3. Wire `app.css` per the package's §8 snippet, replacing the hand-rolled Scribe Amber values with `var(--ds-*)` references.
4. Migrate `--radius` from `0.5rem` to `var(--ds-radius-lg)` (0.625rem) and verify component sizing across all windows.
5. Apply Fraunces / Hanken Grotesk / JetBrains Mono via the font wiring in the §8 snippet.
