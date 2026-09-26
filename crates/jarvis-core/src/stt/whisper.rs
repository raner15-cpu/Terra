//! Whisper transcription for conversation mode.
//!
//! Whisper is intentionally NOT part of the wake-word / command path: those need
//! sub-100 ms latency and a fixed grammar, which Vosk does well. Whisper is only
//! used after the user opened a dialogue ("Терра" -> "Да" -> "Разговор"), where
//! free-form speech quality matters much more than latency.
//!
//! The transcriber runs whisper.cpp as a side process (`whisper-cli`), so no extra
//! Rust/C++ build toolchain is required and everything stays fully offline.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use once_cell::sync::OnceCell;

use crate::{config, i18n, APP_DIR, DB};

#[derive(Debug, Clone)]
pub struct WhisperSetup {
    pub exe: PathBuf,
    pub model: PathBuf,
}

static SETUP: OnceCell<Option<WhisperSetup>> = OnceCell::new();

/// Resolve the whisper binary and model once. Safe to call repeatedly.
pub fn ensure_init() -> bool {
    SETUP.get_or_init(resolve_setup).is_some()
}

pub fn is_available() -> bool {
    matches!(SETUP.get(), Some(Some(_)))
}

pub fn setup() -> Option<&'static WhisperSetup> {
    SETUP.get().and_then(|setup| setup.as_ref())
}

/// Human readable reason why Whisper is unavailable (for logs / UI).
pub fn unavailable_reason() -> String {
    if !whisper_enabled() {
        return "Whisper выключен в настройках (whisper_enabled = false).".to_string();
    }

    format!(
        "Не найден whisper-cli или модель в '{}'. \
Перезапусти Start-Terra.bat: он скачивает Whisper автоматически. \
Папка не удаляется при обновлении проекта.",
        default_exe_dir().display(),
    )
}

fn whisper_enabled() -> bool {
    DB.get()
        .map(|db| db.read().whisper_enabled)
        .unwrap_or(config::WHISPER_ENABLED_BY_DEFAULT)
}

/// Where Whisper is expected to live: a persistent folder OUTSIDE the project, so
/// that deleting and re-downloading the project does not delete a 500 MB model.
/// `Start-Terra.bat` uses the same location and exports `TERRA_WHISPER_DIR`.
pub fn default_exe_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(config::WHISPER_DIR_ENV) {
        let dir = PathBuf::from(dir);
        if !dir.as_os_str().is_empty() {
            return dir;
        }
    }

    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local_app_data)
            .join(config::WHISPER_USER_DIR_NAME)
            .join("whisper");
    }

    if let Some(dirs) = crate::APP_DIRS.get() {
        return dirs.data_dir.join("whisper");
    }

    APP_DIR.join(config::WHISPER_PATH)
}

fn candidate_dirs() -> Vec<PathBuf> {
    let mut roots = vec![default_exe_dir()];

    if let Some(dirs) = crate::APP_DIRS.get() {
        roots.push(dirs.data_dir.join("whisper"));
    }

    // legacy / portable locations inside the project
    roots.push(APP_DIR.join(config::WHISPER_PATH));
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd.join(config::WHISPER_PATH));
    }

    // whisper.cpp archives sometimes keep the binary in a subfolder
    let mut dirs = Vec::with_capacity(roots.len() * 3);
    for root in roots {
        if dirs.contains(&root) {
            continue;
        }
        dirs.push(root.join("Release"));
        dirs.push(root.join("bin"));
        dirs.push(root);
    }
    dirs
}

fn resolve_setup() -> Option<WhisperSetup> {
    if !whisper_enabled() {
        info!("Whisper is disabled in settings, conversation mode will use Vosk text.");
        return None;
    }

    let configured_exe = setting("whisper_exe");
    let configured_model = setting("whisper_model");

    let exe = configured_exe
        .and_then(|value| {
            let path = PathBuf::from(&value);
            if path.is_file() {
                Some(path)
            } else {
                warn!("Configured whisper_exe '{}' not found.", value);
                None
            }
        })
        .or_else(find_exe)?;

    let model = configured_model
        .and_then(|value| {
            let path = PathBuf::from(&value);
            if path.is_file() {
                Some(path)
            } else {
                warn!("Configured whisper_model '{}' not found.", value);
                None
            }
        })
        .or_else(find_model)?;

    info!(
        "Whisper ready for conversation mode. exe: '{}', model: '{}'",
        exe.display(),
        model.display()
    );

    Some(WhisperSetup { exe, model })
}

fn setting(key: &str) -> Option<String> {
    let value = DB.get()?.read().get(key)?;
    let value = value.trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn find_exe() -> Option<PathBuf> {
    for dir in candidate_dirs() {
        for name in config::WHISPER_EXE_NAMES {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    // last resort: something on PATH
    for name in config::WHISPER_EXE_NAMES {
        if Command::new(name).arg("--help").output().is_ok() {
            return Some(PathBuf::from(name));
        }
    }

    warn!("Whisper binary not found. {}", "Expected resources/whisper/whisper-cli(.exe)");
    None
}

fn find_model() -> Option<PathBuf> {
    // an explicit model name from the environment wins (same value the launcher uses)
    let preferred: Vec<String> = std::env::var(config::WHISPER_MODEL_ENV)
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .into_iter()
        .chain(config::WHISPER_MODEL_NAMES.iter().map(|name| name.to_string()))
        .collect();

    for dir in candidate_dirs() {
        for name in &preferred {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }

        // any ggml-*.bin in the whisper folder
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_lowercase();
                if name.starts_with("ggml-") && name.ends_with(".bin") {
                    return Some(path);
                }
            }
        }
    }

    warn!("Whisper model not found in resources/whisper (expected ggml-small.bin).");
    None
}

/// Language passed to whisper.cpp. `auto` lets Whisper detect the language per
/// turn, which is what you want when Russian speech contains English words.
fn whisper_language() -> String {
    if let Some(configured) = setting("whisper_language") {
        return configured;
    }

    match i18n::get_language().as_str() {
        "ru" => "ru".to_string(),
        "ua" => "uk".to_string(),
        "en" => "en".to_string(),
        _ => "auto".to_string(),
    }
}

/// Optional vocabulary hint. Whisper conditions on it, which noticeably helps with
/// rare or domain words and keeps mixed-language speech from being over-normalized.
fn whisper_prompt() -> Option<String> {
    setting("whisper_prompt").or_else(|| {
        let default = config::WHISPER_DEFAULT_PROMPT.trim();
        if default.is_empty() {
            None
        } else {
            Some(default.to_string())
        }
    })
}

/// Transcribe one utterance of 16 kHz mono PCM.
pub fn transcribe(samples: &[i16]) -> Result<String, String> {
    let setup = setup().ok_or_else(unavailable_reason)?;

    if samples.len() < config::WHISPER_MIN_SAMPLES {
        return Err("Фрагмент слишком короткий для Whisper.".to_string());
    }

    let started = Instant::now();
    let wav_path = write_wav(samples)?;
    let result = run_whisper(&setup.exe, &setup.model, &wav_path);
    let _ = std::fs::remove_file(&wav_path);

    let text = result?;
    info!(
        "Whisper transcribed {:.1}s of audio in {} ms",
        samples.len() as f32 / 16_000.0,
        started.elapsed().as_millis()
    );

    Ok(text)
}

fn write_wav(samples: &[i16]) -> Result<PathBuf, String> {
    let path = std::env::temp_dir().join(format!(
        "terra-whisper-{}.wav",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_millis())
            .unwrap_or_default()
    ));

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = hound::WavWriter::create(&path, spec)
        .map_err(|error| format!("Не удалось создать WAV для Whisper: {error}"))?;
    for sample in samples {
        writer
            .write_sample(*sample)
            .map_err(|error| format!("Не удалось записать WAV для Whisper: {error}"))?;
    }
    writer
        .finalize()
        .map_err(|error| format!("Не удалось закрыть WAV для Whisper: {error}"))?;

    Ok(path)
}

fn run_whisper(exe: &Path, model: &Path, wav_path: &Path) -> Result<String, String> {
    let threads = config::WHISPER_THREADS.to_string();
    let beam_size = config::WHISPER_BEAM_SIZE.to_string();
    let language = whisper_language();

    let mut command = Command::new(exe);
    command
        .arg("-m")
        .arg(model)
        .arg("-f")
        .arg(wav_path)
        .arg("-l")
        .arg(&language)
        .arg("-t")
        .arg(&threads)
        .arg("-bs")
        .arg(&beam_size)
        .arg("-nt") // no timestamps
        .arg("-np"); // no progress prints

    if let Some(prompt) = whisper_prompt() {
        command.arg("--prompt").arg(prompt);
    }

    let output = command
        .output()
        .map_err(|error| format!("Не удалось запустить whisper-cli: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "whisper-cli вернул ошибку ({}): {}",
            output.status,
            stderr.trim()
        ));
    }

    let text = clean_transcript(&String::from_utf8_lossy(&output.stdout));
    if text.is_empty() {
        return Err("Whisper не распознал речь в этом фрагменте.".to_string());
    }

    Ok(text)
}

/// Drop whisper.cpp control markers and non-speech annotations.
fn clean_transcript(raw: &str) -> String {
    let mut parts: Vec<String> = Vec::new();

    for line in raw.lines() {
        let mut line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        for marker in config::WHISPER_NOISE_MARKERS {
            line = line.replace(marker, " ");
        }

        // strip "[...]" and "(...)" annotations like [BLANK_AUDIO] or (музыка)
        line = strip_annotations(&line);

        let line = line.trim();
        if !line.is_empty() {
            parts.push(line.to_string());
        }
    }

    parts
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_annotations(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut depth_square = 0usize;
    let mut depth_round = 0usize;

    for character in input.chars() {
        match character {
            '[' => depth_square += 1,
            ']' => depth_square = depth_square.saturating_sub(1),
            '(' => depth_round += 1,
            ')' => depth_round = depth_round.saturating_sub(1),
            _ if depth_square == 0 && depth_round == 0 => output.push(character),
            _ => {}
        }
    }

    output
}
