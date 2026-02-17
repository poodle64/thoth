<div align="center">

[![Tauri](https://img.shields.io/badge/Tauri-2.0-24C8D8?style=flat-square&logo=tauri&logoColor=white)](https://tauri.app/)
[![Rust](https://img.shields.io/badge/Rust-1.75+-DEA584?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Svelte](https://img.shields.io/badge/Svelte-5-FF3E00?style=flat-square&logo=svelte&logoColor=white)](https://svelte.dev/)
[![Licence](https://img.shields.io/badge/Licence-MIT-blue?style=flat-square)](LICENCE)

# 𓅝 Thoth

### Scribe to the gods. Typist to you.

**Press a key. Speak. Text appears.**

[Download](#download) · [Features](#features) · [Build](#building-from-source) · [Architecture](docs/architecture/) · [Product Docs](docs/product/)

---

</div>

## The Problem

Voice input on macOS is either cloud-dependent or requires complex setup. Apple
Dictation sends your audio to Apple. Third-party tools want subscriptions. And
nothing handles technical jargon without mangling it.

## The Solution

Thoth runs speech-to-text locally using whisper.cpp with Metal GPU acceleration.
Nothing leaves the machine. Press a hotkey, speak, and text appears at your
cursor. No windows. No configuration. No cloud.

---

## Download

**[Download the latest release](https://github.com/poodle64/thoth/releases/latest)** for macOS (Apple Silicon).

> After installing, Thoth checks for updates automatically and installs them in-app.

---

## Features

<table>
<tr>
<td width="50%">

**Offline Transcription**

- whisper.cpp with Metal GPU acceleration
- Nothing leaves your machine
- Works without internet
- Real-time voice activity detection

</td>
<td width="50%">

**AI Enhancement**

- Post-process with Ollama (local)
- Grammar, formatting, and tone correction
- Clipboard context awareness
- Custom prompts with templates

</td>
</tr>
<tr>
<td width="50%">

**Personal Dictionary**

- Custom vocabulary for domain terms
- Text replacement rules
- Prevents "dev" becoming "Dave"
- Import/export support

</td>
<td width="50%">

**Cross-Platform**

- macOS native (menu bar app)
- Linux support planned
- Global keyboard shortcuts
- Recording indicator near cursor

</td>
</tr>
<tr>
<td width="50%">

**Recording Options**

- Push-to-talk or hands-free mode
- VAD silence detection
- Configurable audio device
- Sound feedback (optional)

</td>
<td width="50%">

**History & Export**

- Searchable transcription history
- JSON/CSV/TXT export
- SQLite database
- Configurable retention

</td>
</tr>
</table>

---

## Building from Source

```bash
pnpm install
pnpm tauri dev    # Development build
pnpm tauri build  # Production build
```

<details>
<summary><strong>Requirements</strong></summary>

- macOS 14.0+ or Linux
- Rust 1.75+
- Node.js 20+
- pnpm

</details>

---

## Tech Stack

| Layer         | Choice     | Why                                                          |
| ------------- | ---------- | ------------------------------------------------------------ |
| Framework     | Tauri 2.0  | Native performance, cross-platform                           |
| Backend       | Rust       | Memory safety, audio performance                             |
| Frontend      | Svelte 5   | Reactive UI with runes                                       |
| Audio         | cpal       | Cross-platform audio capture                                 |
| Transcription | whisper-rs | whisper.cpp with Metal GPU acceleration (sherpa-rs fallback) |
| Database      | SQLite     | Local persistence with migrations                            |
| AI            | Ollama     | Local LLM enhancement                                        |

---

## Documentation

- **Product docs:** [docs/product/](docs/product/). Intent, workflows, design principles
- **Architecture:** [docs/architecture/](docs/architecture/). Audio pipeline, data model

---

## Acknowledgements

<details>
<summary><strong>Core Technology</strong></summary>

- [whisper.cpp](https://github.com/ggerganov/whisper.cpp). High-performance speech recognition with Metal GPU acceleration
- [whisper-rs](https://github.com/tazz4843/whisper-rs). Rust bindings for whisper.cpp
- [Sherpa-ONNX](https://github.com/k2-fsa/sherpa-onnx). Fallback speech recognition inference
- [NVIDIA Parakeet](https://catalog.ngc.nvidia.com/orgs/nvidia/teams/nemo/models/parakeet-tdt-1.1b). Speech-to-text models

</details>

<details>
<summary><strong>Dependencies</strong></summary>

- [Tauri](https://tauri.app/). Desktop application framework
- [cpal](https://github.com/RustAudio/cpal). Cross-platform audio
- [rubato](https://github.com/HEnquist/rubato). Audio resampling
- [enigo](https://github.com/enigo-rs/enigo). Cross-platform input simulation

</details>

---

<div align="center">

**Your voice. Your machine. Nothing else.**

_Named after the Egyptian god of writing and wisdom, the scribe who faithfully records all that is spoken._

</div>
