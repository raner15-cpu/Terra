# Terra: локальный запуск для разработки (Windows)

## Что потребуется

- Windows 10 22H2 или новее.
- Git, Node.js LTS и Rust stable (MSVC toolchain).
- Visual Studio Build Tools с workload **Desktop development with C++**.
- Microsoft Edge WebView2 Runtime.
- Ollama для локального чата и свободное место под выбранную модель.

Rust/Tauri и Node нужны для запуска из исходников.

## Ollama и модели

Установи и запусти Ollama. API должен отвечать на `http://127.0.0.1:11434`. Установленные модели можно проверить командой `ollama list`. Terra показывает модели, уже установленные в Ollama; чат не использует облачный API.

## Скачать и запустить Terra

Распакуй актуальный архив ветки `terra-skeleton`. Открой PowerShell в корне папки Terra и выполни один раз:

```powershell
Set-Location .\frontend
npm.cmd ci
Set-Location ..
cargo install tauri-cli --version "^2.0.0" --locked
```

Для dev-кнопки **«Запустить»** также собери голосовой runtime. Из корня репозитория выполни:

```powershell
cargo build -p jarvis-app
```

Это создаёт `jarvis-app.exe` рядом с `jarvis-gui.exe`; графическая кнопка запускает этот отдельный процесс. Без этого бинарника кнопка покажет ошибку, а не запустит ассистента.

Затем запусти графическую Terra:

```powershell
Set-Location .\crates\jarvis-gui
cargo tauri dev
```

Tauri сам запускает Vite на `127.0.0.1:1420`. Не запускай второй Vite отдельно. Не закрывай PowerShell во время работы dev-приложения. На первом запуске Cargo может долго скачивать и собирать Rust-зависимости.

Для повторного запуска, если исходники и зависимости не менялись, достаточно перейти в `crates\jarvis-gui` и выполнить `cargo tauri dev`. Если менялся `jarvis-app`, сначала повтори `cargo build -p jarvis-app`.

Чтобы собрать desktop-пакет, из `crates\jarvis-gui` выполни `cargo tauri build`.

## Модели для теста

- `qwen3:8b` — универсальная многоязычная модель; файл около 5.2 GB.
- `bambucha/saiga-llama3` — русскоязычная Saiga/Llama 3 8B; файл около 4.9 GB.

Размер файла — не полный объём RAM/VRAM при генерации. На RTX 4060 с 16 GB RAM начни с одной 8B-модели. Если памяти не хватает или ответы медленные, попробуй `qwen3:4b`.
