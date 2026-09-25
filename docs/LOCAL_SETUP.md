# Terra: локальный запуск для разработки (Windows)

## Что потребуется

- Windows 10 22H2 или новее.
- Git, Node.js LTS и Rust stable (MSVC toolchain).
- Visual Studio Build Tools с workload **Desktop development with C++**.
- Microsoft Edge WebView2 Runtime.
- Ollama для локального чата и свободное место под выбранную модель.

Rust/Tauri и Node нужны только для запуска из исходников. Позже можно собирать обычный установщик.

## Установить Ollama и модель

1. Установи Ollama для Windows с https://ollama.com/download/windows и запусти приложение. Оно поднимает локальный API на `http://127.0.0.1:11434`.
2. В PowerShell скачай одну или обе модели:

```powershell
ollama pull qwen3:8b
ollama pull bambucha/saiga-llama3
```

3. Проверь одну из них прямо в терминале, например:

```powershell
ollama run qwen3:8b
```

Выйти из диалога Ollama можно командой `/bye`. Модели хранятся локально; скачать каждую нужно только один раз. Проверь API и установленные модели:

```powershell
(Invoke-RestMethod http://127.0.0.1:11434/api/tags).models | Select-Object name
```

В Terra появятся все локальные модели, которые видит Ollama; выбор не зашит в приложение. Установи только одну, если пока не хочешь занимать место.

## Скачать и запустить Terra

```powershell
git clone --branch terra-skeleton --single-branch https://github.com/raner15-cpu/Terra.git
cd Terra
cd frontend
npm ci
cd ..
cargo install tauri-cli --version "^2.0.0" --locked
cd crates/jarvis-gui
cargo tauri dev
```

Первый запуск Rust может занять несколько минут: Cargo загрузит и соберёт зависимости. Для чата Ollama должна быть запущена. В приложении открой **Текстовый диалог**, нажми **Обновить** и выбери модель.

Чтобы собрать desktop-пакет после установки зависимостей, из `crates/jarvis-gui` выполни `cargo tauri build`.

## Какую модель попробовать

- `qwen3:8b` — универсальная многоязычная модель, файл около 5.2 GB; хороший первый кандидат для проверки русского диалога.
- `bambucha/saiga-llama3` — русскоязычный Saiga/Llama 3 8B, файл около 4.9 GB; вариант для сравнения, но более старый и с указанным на странице Ollama контекстом 8K.

Размер файла — не полный объём оперативной/видеопамяти при генерации. На RTX 4060 с 16 GB RAM начни с одной 8B-модели и закрой тяжёлые программы. Если памяти не хватает или ответы слишком медленные, попробуй `qwen3:4b` (`ollama pull qwen3:4b`).

Весь запрос и ответ обрабатываются локально через Ollama; приложение не требует ключа облачного API.
