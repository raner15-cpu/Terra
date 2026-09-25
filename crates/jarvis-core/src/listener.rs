mod rustpotter;
mod vosk;

use once_cell::sync::OnceCell;

use crate::config::structs::WakeWordEngine;

use crate::{APP_DIR, DB};

static WAKE_WORD_ENGINE: OnceCell<WakeWordEngine> = OnceCell::new();
const TERRA_RUSTPOTTER_MODEL: &str = "resources/rustpotter/terra.rpw";

pub fn init() -> Result<(), String> {
    if WAKE_WORD_ENGINE.get().is_some() {
        return Ok(());
    }

    let configured_engine = DB.get().unwrap().read().wake_word_engine;
    let engine = match configured_engine {
        WakeWordEngine::Rustpotter if !APP_DIR.join(TERRA_RUSTPOTTER_MODEL).is_file() => {
            warn!(
                "Terra Rustpotter profile is not installed; using Vosk Terra wake grammar until it is."
            );
            WakeWordEngine::Vosk
        }
        WakeWordEngine::Porcupine => {
            warn!("Porcupine is unsupported in this build; using Vosk Terra wake grammar.");
            WakeWordEngine::Vosk
        }
        other => other,
    };

    WAKE_WORD_ENGINE
        .set(engine)
        .map_err(|_| "Wake word engine already set".to_string())?;

    match engine {
        WakeWordEngine::Porcupine => unreachable!("unsupported engine is redirected above"),
        WakeWordEngine::Rustpotter => {
            info!("Initializing Rustpotter wake-word engine.");
            rustpotter::init().map_err(|_| "Failed to init Rustpotter".to_string())
        }
        WakeWordEngine::Vosk => {
            info!("Initializing Vosk as wake-word engine.");
            warn!("Using Vosk as wake-word engine is highly not recommended, because it's very slow for this task.");
            vosk::init().map_err(|_| "Failed to init Vosk wake-word".to_string())
        }
    }
}

pub fn data_callback(frame_buffer: &[i16]) -> Option<i32> {
    match WAKE_WORD_ENGINE.get()? {
        WakeWordEngine::Porcupine => None,
        WakeWordEngine::Rustpotter => rustpotter::data_callback(frame_buffer),
        WakeWordEngine::Vosk => vosk::data_callback(frame_buffer),
    }
}
