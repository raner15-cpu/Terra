# Whisper (conversation mode)

Terra uses Vosk for the wake word and for commands, and Whisper Small only inside a
voice conversation (`Терра` → `Да` → `Разговор`).

Expected content of this folder:

- `whisper-cli.exe` (or `main.exe`) from a whisper.cpp release, together with its DLLs
- `ggml-small.bin` — multilingual Whisper Small model

Automatic setup:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\setup-whisper.ps1
```

Manual setup:

1. Binary: https://github.com/ggml-org/whisper.cpp/releases (`whisper-bin-x64.zip`)
2. Model: https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin

Settings (optional, via the settings store):

- `whisper_enabled` — `true` / `false`, default `true`
- `whisper_model` — absolute path to a `.bin` model, overrides auto-detection
- `whisper_exe` — absolute path to the whisper.cpp binary

If the binary or model is missing, Terra logs the reason once when a conversation
starts and keeps using Vosk text, so the dialogue still works.
