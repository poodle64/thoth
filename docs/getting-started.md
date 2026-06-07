# Getting started with Thoth

You have installed Thoth; this guide walks you from a fresh launch to your first dictation in about five to ten minutes, then points you at the extras worth turning on.

Thoth turns speech into text and inserts it wherever your cursor is. Everything runs on your own machine: the speech model, the audio, the history. Nothing is sent to the cloud, and you do not need an internet connection once the model is downloaded.

The setup is three quick steps; the app's Overview tab walks you through them as a checklist that ticks itself off as you go.

## Step 1: download a speech model

A "speech model" is the file that does the actual listening; it is the trained system that converts the sound of your voice into words. Thoth ships without one so you can choose where it lives and how big it is, then downloads it on first run.

1. Open Thoth. The window opens on the **Overview** tab.
2. Find the first checklist item, **Download speech model**, and click **Download Recommended Model**.
3. Wait for the download and a short "Preparing transcription engine" step to finish. The checklist item turns green when the model is ready.

The recommended model runs entirely on your machine and is around 500 MB. The download is one-off; after this Thoth works offline. If you would rather pick a smaller or larger model, click **Choose a different model** to open the **Models** tab, where each option lists its size and language support (the Whisper models there range from roughly 0.5 to 1.6 GB).

> On Apple Silicon Macs the recommended model uses the Neural Engine (Apple's dedicated machine-learning chip) for very fast transcription. On Linux, transcription is GPU-accelerated where a supported GPU is present, and falls back to the CPU otherwise.

## Step 2: grant permissions

Thoth needs permission to hear your microphone, and to start recording from a keyboard shortcut while you are working in another app. How you grant these differs by operating system.

### macOS

macOS gates microphone and global-shortcut access behind its privacy system, so you grant each one once. The Overview checklist drives this for you:

1. **Allow microphone access.** Click **Allow** on checklist step 2. macOS shows its own microphone dialogue; click Allow there too. This is the standard macOS permission under **System Settings > Privacy & Security > Microphone**.
2. **Allow global shortcut.** Click **Allow** on checklist step 3. This grants **Accessibility** access under **System Settings > Privacy & Security > Accessibility**, which is what lets the recording hotkey work from any app rather than only when Thoth is focused.

If a step does not turn green, open **System Settings > Privacy & Security**, find **Microphone** or **Accessibility**, and confirm Thoth's switch is on. After an app update or reinstall the Accessibility permission can look granted but stop working; if Thoth flags this, use the **Reset & Re-grant** button it offers and toggle the permission again in System Settings.

There is also an optional **Input Monitoring** permission in the checklist's "Optional settings" section. You only need it if you want to change the recording shortcut to a custom key; the default shortcut works without it.

### Linux

Linux has no per-app microphone dialogue like macOS. Audio capture goes through **PulseAudio** or **PipeWire** (the sound systems most modern Linux desktops use), and Thoth simply uses whatever capture device they expose. If your microphone already works in other apps, Thoth will find it; there is no permission to click.

The global shortcut behaves differently depending on your display server (the part of Linux that draws windows and handles input):

- **On X11**, Thoth registers the recording hotkey directly, much like macOS.
- **On Wayland**, applications are not allowed to grab a hotkey on their own. Instead Thoth asks the desktop (the "compositor") to bind the shortcut through a system service called the global-shortcuts portal. The important consequence: **the compositor, not Thoth, decides the actual key**, and it may show you its own dialogue to confirm or pick one. Thoth then displays the key the compositor assigned, so check the **Recording** settings to see what you have. KDE Plasma, Sway, Hyprland, and GNOME 48 and newer support this; on older or minimal desktops the portal may be unavailable, in which case Thoth tells you so you can fall back to a function-key shortcut.

If anything here does not behave as described, see [troubleshooting.md](troubleshooting.md).

## Step 3: set or confirm the record hotkey

The record hotkey is the key you press to start and stop dictation. The default is **F13**.

F13 is chosen deliberately: it is a key most keyboards expose (often through a remapping tool, or as a real key on larger keyboards) but almost nothing else uses, so it will not clash with shortcuts in your other apps. If your keyboard has no F13, or it conflicts with something, change it:

1. Open **Settings** and choose the **Recording** tab.
2. In the keyboard-shortcuts section, click the recording shortcut field and press the key (or key combination) you want.
3. The change saves immediately; Thoth re-registers the new shortcut.

Single function keys (F13 through F20) make the most reliable shortcuts because they work as a bare key press. On macOS, customising the shortcut is when the optional **Input Monitoring** permission from step 2 comes into play. On Wayland, remember the compositor may override your choice (see the Linux notes above).

## Step 4: your first dictation

Everything is ready. Try it:

1. Click into any text field; a note, a browser address bar, a chat box, anything with a cursor.
2. Press your record hotkey (**F13** by default). A small recording indicator appears near your cursor.
3. Speak a sentence or two, naturally.
4. Press the hotkey again to stop.

Thoth transcribes what you said and inserts the text at your cursor, in whatever app you are using. That is the whole loop: press, speak, press, text appears.

Behind the scenes Thoth inserts the text either by simulating typing or by a quick paste; if it pastes, it restores whatever was on your clipboard afterwards, so your copy-paste is not disturbed.

## Optional next steps

The basics work now. These extras are worth a look when you are ready.

### Australian spelling and output filters

Thoth can tidy up its output automatically: convert US spellings to Australian/British ones (for example "-or" endings become "-our", and "-ize" verbs become "-ise"), strip filler words like "um", normalise spacing and punctuation, apply sentence case, and turn spoken numbers into digits. Find these toggles under **Settings > Transcribe**, in the **Output Filtering** section. Turn on only the ones you want; they apply to every transcription.

### Personal dictionary

Speech models sometimes mishear names, jargon, and product names; "immich" becomes "image", a colleague's name becomes a common word. The personal dictionary fixes this with your own vocabulary and replacement rules, so the same correction happens every time. See [dictionary.md](dictionary.md) to set it up.

### AI enhancement

If you run [Ollama](https://ollama.com) (a tool for running language models locally), Thoth can post-process a transcription to fix grammar, reformat it, or change its tone, all still on your machine. You write the instructions as reusable prompts. The **AI Enhancement** tab in Settings shows whether Ollama is connected; the full walkthrough is in [custom-prompts-guide.md](custom-prompts-guide.md).

### Automating Thoth

Thoth has an opt-in local control interface and a bundled assistant connector, so an AI assistant or a script on the same machine can drive the dictionary, settings, and history, or transcribe audio files for you. See [automation.md](automation.md).

### Where your data lives

Everything Thoth stores sits in a single hidden folder in your home directory, **`~/.thoth/`** (the leading dot means the folder is hidden by default):

| Path                       | What it holds                                  |
| -------------------------- | ---------------------------------------------- |
| `~/.thoth/config.json`     | Your settings                                  |
| `~/.thoth/dictionary.json` | Your personal dictionary and canonical terms   |
| `~/.thoth/models/`         | Downloaded speech models                       |
| `~/.thoth/thoth.db`        | Your transcription history (a SQLite database) |
| `~/.thoth/Recordings/`     | Saved audio recordings                         |
| `~/.thoth/logs/`           | Diagnostic logs                                |

Because it is all local and in one place, you can back it up, inspect it, or remove it yourself. Deleting `~/.thoth/` resets Thoth to a fresh state.

---

Stuck on anything? Start with [troubleshooting.md](troubleshooting.md).
