---
name: Thoth
description: Privacy-first, offline-capable voice transcription application
version: 2026.7.1
status: "adopted @poodle64/design-tokens and @poodle64/ui (WP-125)"
---

## Current state

Thoth uses a dark-only theme called **Scribe Amber**, wired onto `@poodle64/design-tokens` + `@poodle64/ui` in `src/app.css`. The palette is built around a warm amber primary (`oklch(0.693 0.124 65.9)`), dark brown backgrounds (deepest at `oklch(0.223 0.002 67.7)`), and muted stone neutrals — all still expressed in OKLCH, but now as a **full surface-ladder override** of the shared `--ds-color-*` tokens rather than a hand-rolled shadcn alias layer (see the block comment at the top of `app.css` for the rationale and the one visible drift this introduced: `--accent` moved from the old muted tier to the card tier). The radius base now resolves to `var(--ds-radius-lg)` (0.625rem, up from the previous hand-rolled `0.5rem`) via `@poodle64/ui/styles.css`. Thoth has no light mode (`ModeWatcher` defaults to dark, no toggle exists), so `:root` and `.dark` carry the identical override. Fonts remain the system default; Fraunces, Hanken Grotesk, and JetBrains Mono from the shared design language are not yet applied — a follow-up, not part of this migration.

## Shared baseline

The household design language and binding constants live in two places:

- `docs/master/design/shared-design-language.md` -- canonical source for colour space (OKLCH), radius (`--ds-radius-lg: 0.625rem`), typography (Fraunces display, Hanken Grotesk body, JetBrains Mono code), status vocabulary (success / warning / error / info / neutral), spacing, and namespace (`--ds-*`).
- `@poodle64/design-tokens` (GitHub Packages, public) -- the compiled CSS custom-property package that projects consume. Provides the `--ds-*` token set and the wiring snippet for `app.css`.
- `@poodle64/ui` (GitHub Packages, public) -- the shared shadcn-svelte component layer. Provides the shadcn semantic surface registration (`--card`, `--popover`, `--muted`, …) plus the composed page-chrome components.

## Component adoption

Thoth's vendored `src/lib/components/ui/` primitives with a `@poodle64/ui` equivalent now import from the package. Three primitives stay local because the package genuinely does not ship them:

- **`form`** -- deliberately excluded upstream (a Formsnap/Superforms binding layer is an application-architecture choice, not a design one; see `@poodle64/ui`'s README).
- **`radio-group`**, **`context-menu`** -- not yet added to the shared package (no app has migrated them upstream yet); Thoth's local copies are not drift, per `20-sveltekit-frontend.md`'s "genuinely missing" carve-out.

`scroll-area` was vendored but had zero importers in Thoth's codebase and was deleted outright rather than migrated.
