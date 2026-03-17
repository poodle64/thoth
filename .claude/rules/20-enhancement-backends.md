# Enhancement Backend Rules

Rules for the AI text enhancement subsystem (OpenAI-compatible, Anthropic, Ollama).

## API Key Handling

- API keys MUST be stored via config.rs `save_to_disk()` which enforces 0600 permissions
- API keys MUST NOT appear in log output, error messages, or frontend console
- Frontend API key inputs MUST use `type="password"` with optional show/hide toggle
- Anthropic API key auto-detection from `ANTHROPIC_API_KEY` env var is permitted

## URL Validation

- OpenAI-compatible `base_url` MUST be validated: only `http://` and `https://` schemes accepted
- Anthropic `base_url` MUST warn if not HTTPS (unless localhost) — API key transmitted in headers
- No scheme validation bypass — `file://`, `ftp://`, `javascript:` etc. are always rejected

## Backend Selection

- Pipeline MUST use the correct model for the active backend:
  - `backend == "anthropic"` → use `anthropicModel` from config
  - `backend == "ollama" | "openai_compat"` → use `model` from config
- Tray menu MUST display the active backend and model (e.g. "Cloud: claude-haiku-4-5-20251001")
- Tray MUST refresh after backend switch

## Timeouts and Retries

- Default timeout: 30 seconds for all backends
- OpenAI-compatible: retry with exponential backoff (3 attempts)
- Anthropic: single attempt (cloud service, retries add cost)
- Ollama: single attempt (local, fast failure preferred)

## Error Handling

- Error messages MUST include attempt count and generic error description
- Error messages MUST NOT include request bodies, headers, or API keys
- Failed enhancement MUST NOT block the transcription pipeline — return original text
