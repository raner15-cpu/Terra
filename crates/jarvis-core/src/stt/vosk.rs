use once_cell::sync::OnceCell;
use vosk::{DecodingState, Recognizer};
use std::sync::Arc;
use parking_lot::Mutex;

use crate::{vosk_models, i18n, config, models};
use crate::models::vosk::VoskModel;
use crate::DB;

// the model Arc keeps the vosk::Model alive for the recognizers
static VOSK_MODEL: OnceCell<Arc<VoskModel>> = OnceCell::new();
static WAKE_RECOGNIZER: OnceCell<Mutex<Recognizer>> = OnceCell::new();
static SPEECH_RECOGNIZER: OnceCell<Mutex<Recognizer>> = OnceCell::new();
static SPEECH_ACCUMULATOR: OnceCell<Mutex<SpeechAccumulator>> = OnceCell::new();

#[derive(Default)]
struct SpeechAccumulator {
    segments: Vec<String>,
    silence_frames: u32,
    frames_since_segment: u32,
    noise_floor: f32,
    frames_seen: u64,
}

const SPEECH_SAMPLE_RATE: f32 = 16_000.0;
const SPEECH_FRAME_LENGTH: f32 = 512.0;
/// How long the user has to stay quiet before we treat the utterance as finished.
const UTTERANCE_END_SILENCE_SECONDS: f32 = 0.9;
/// Hard flush: Vosk already finalized text and nothing new arrived for this long.
const UTTERANCE_FLUSH_SECONDS: f32 = 1.8;
/// Absolute minimum RMS that may be treated as speech, regardless of noise floor.
const MIN_SPEECH_RMS: f32 = 45.0;
/// A frame counts as speech when it is this much louder than the measured noise floor.
const NOISE_FLOOR_SPEECH_FACTOR: f32 = 2.2;
/// Lowest noise floor estimate we allow (keeps the factor meaningful on clean mics).
const NOISE_FLOOR_MIN: f32 = 8.0;
/// Debug throttling: log recognizer state roughly once per second.
const DEBUG_LOG_EVERY_FRAMES: u64 = 31;

pub fn init_vosk() -> Result<(), String> {
    if VOSK_MODEL.get().is_some() {
        return Ok(());
    }

    let model_path = get_configured_model_path()?;
    let model_id = format!("vosk:{}", model_path.display());

    // load through registry (shared if anything else needs the same model)
    let vosk = models::vosk::load(
        models::registry(),
        &model_id,
        model_path.to_str().unwrap(),
    )?;

    // language-specific wake grammar
    let lang = i18n::get_language();
    let wake_grammar = config::get_wake_grammar(&lang);
    info!("Wake grammar for '{}': {:?}", lang, wake_grammar);

    let mut wake_recognizer = Recognizer::new_with_grammar(&vosk.model, 16000.0, wake_grammar)
        .ok_or("Failed to create wake word recognizer")?;

    wake_recognizer.set_max_alternatives(1);

    let mut speech_recognizer = Recognizer::new(&vosk.model, 16000.0)
        .ok_or("Failed to create speech recognizer")?;

    speech_recognizer.set_max_alternatives(config::VOSK_SPEECH_RECOGNIZER_MAX_ALTERNATIVES);
    speech_recognizer.set_words(config::VOSK_SPEECH_RECOGNIZER_WORDS);
    speech_recognizer.set_partial_words(config::VOSK_SPEECH_PARTIAL_WORDS);

    VOSK_MODEL.set(vosk).map_err(|_| "Model already set")?;
    WAKE_RECOGNIZER.set(Mutex::new(wake_recognizer)).map_err(|_| "Wake recognizer already set")?;
    SPEECH_RECOGNIZER.set(Mutex::new(speech_recognizer)).map_err(|_| "Speech recognizer already set")?;
    SPEECH_ACCUMULATOR
        .set(Mutex::new(SpeechAccumulator::default()))
        .map_err(|_| "Speech accumulator already set")?;

    Ok(())
}


pub fn recognize_wake_word(data: &[i16]) -> Option<(String, f32)> {
    let mut recognizer = WAKE_RECOGNIZER.get()?.lock();
    
    match recognizer.accept_waveform(data) {
        Ok(DecodingState::Running) => {
            None
        }
        Ok(DecodingState::Finalized) => {
            let result = recognizer.result();
            
            if let Some(alternatives) = result.multiple() {
                if let Some(best) = alternatives.alternatives.first() {
                    if !best.text.is_empty() {
                        return Some((best.text.to_string(), best.confidence));
                    }
                }
            }
            
            None
        }
        _ => None,
    }
}


pub fn recognize_speech(data: &[i16]) -> Option<String> {
    recognize_speech_with_vad(data, None)
}

/// Feed one frame into the command/conversation recognizer.
///
/// `voice_hint` is the decision of the application VAD for the same frame. It is
/// combined (logical OR) with an adaptive, noise-floor based detector, so a noisy
/// microphone can no longer keep the utterance open forever. That bug made Terra
/// react to the wake word but never return a command.
pub fn recognize_speech_with_vad(data: &[i16], voice_hint: Option<bool>) -> Option<String> {
    let mut recognizer = SPEECH_RECOGNIZER.get()?.lock();
    let mut accumulator = SPEECH_ACCUMULATOR.get()?.lock();

    let rms = frame_rms(data);
    accumulator.frames_seen += 1;

    // adaptive noise floor: falls fast, rises slowly
    if accumulator.noise_floor <= 0.0 {
        accumulator.noise_floor = rms.max(NOISE_FLOOR_MIN);
    } else if rms < accumulator.noise_floor {
        accumulator.noise_floor = accumulator.noise_floor * 0.9 + rms * 0.1;
    } else {
        accumulator.noise_floor = accumulator.noise_floor * 0.995 + rms * 0.005;
    }
    accumulator.noise_floor = accumulator.noise_floor.max(NOISE_FLOOR_MIN);

    let adaptive_threshold =
        (accumulator.noise_floor * NOISE_FLOOR_SPEECH_FACTOR).max(MIN_SPEECH_RMS);
    let is_speech = rms >= adaptive_threshold || voice_hint.unwrap_or(false);

    if is_speech {
        accumulator.silence_frames = 0;
    } else {
        accumulator.silence_frames += 1;
    }

    if !accumulator.segments.is_empty() {
        accumulator.frames_since_segment += 1;
    }

    if let Ok(DecodingState::Finalized) = recognizer.accept_waveform(data) {
        if let Some(text) = recognizer.result().multiple().and_then(|multiple| {
            multiple
                .alternatives
                .first()
                .map(|alternative| alternative.text.trim().to_string())
        }) {
            if !text.is_empty() && text != "[unk]" {
                debug!("STT segment finalized: '{}'", text);
                accumulator.segments.push(text);
                accumulator.frames_since_segment = 0;
            }
        }
    }

    let silence_threshold = seconds_to_frames(UTTERANCE_END_SILENCE_SECONDS);
    let flush_threshold = seconds_to_frames(UTTERANCE_FLUSH_SECONDS);

    if accumulator.frames_seen % DEBUG_LOG_EVERY_FRAMES == 0 {
        debug!(
            "STT frame: rms={:.0} floor={:.0} thr={:.0} speech={} silence={}/{} pending_segments={} since_segment={}",
            rms,
            accumulator.noise_floor,
            adaptive_threshold,
            is_speech,
            accumulator.silence_frames,
            silence_threshold,
            accumulator.segments.len(),
            accumulator.frames_since_segment,
        );
    }

    let ended_by_silence =
        accumulator.silence_frames >= silence_threshold && !accumulator.segments.is_empty();
    let ended_by_timeout =
        accumulator.frames_since_segment >= flush_threshold && !accumulator.segments.is_empty();

    if ended_by_silence || ended_by_timeout {
        let utterance = accumulator.segments.join(" ").trim().to_string();
        accumulator.segments.clear();
        accumulator.silence_frames = 0;
        accumulator.frames_since_segment = 0;
        info!(
            "Utterance closed ({}): '{}'",
            if ended_by_silence { "silence" } else { "flush timeout" },
            utterance
        );
        if utterance.is_empty() {
            return None;
        }
        return Some(utterance);
    }

    None
}

fn seconds_to_frames(seconds: f32) -> u32 {
    (((seconds * SPEECH_SAMPLE_RATE) / SPEECH_FRAME_LENGTH).max(1.0)) as u32
}

fn frame_rms(data: &[i16]) -> f32 {
    if data.is_empty() {
        return 0.0;
    }

    let energy = data
        .iter()
        .map(|sample| {
            let value = *sample as f64;
            value * value
        })
        .sum::<f64>()
        / data.len() as f64;

    energy.sqrt() as f32
}

pub fn reset_speech_recognizer() {
    if let Some(recognizer) = SPEECH_RECOGNIZER.get() {
        recognizer.lock().reset();
    }
    if let Some(accumulator) = SPEECH_ACCUMULATOR.get() {
        let mut accumulator = accumulator.lock();
        accumulator.segments.clear();
        accumulator.silence_frames = 0;
        accumulator.frames_since_segment = 0;
    }
}

pub fn reset_wake_recognizer() {
    if let Some(recognizer) = WAKE_RECOGNIZER.get() {
        recognizer.lock().reset();
    }
}

fn get_configured_model_path() -> Result<std::path::PathBuf, String> {
    // try to get from settings
    if let Some(db) = DB.get() {
        let settings = db.read();
        if !settings.vosk_model.is_empty() {
            if let Some(path) = vosk_models::get_model_path(&settings.vosk_model) {
                return Ok(path);
            }
            warn!("Configured Vosk model '{}' not found, falling back to auto-detect", settings.vosk_model);
        }
    }
    
    // auto-detect: prefer model matching current language
    let available = vosk_models::scan_vosk_models();
    let language = i18n::get_language();

    let lang_code = match language.as_str() {
        "ru" => "ru",
        "en" => "us",
        "ua" => "uk",
        other => other,
    };

    if let Some(matched) = available.iter().find(|m| m.language == lang_code) {
        info!("Auto-detected Vosk model for '{}': {}", language, matched.name);
        return Ok(matched.path.clone());
    }

    if let Some(first) = available.first() {
        info!("Auto-detected Vosk model (no language match): {}", first.name);
        return Ok(first.path.clone());
    }
    
    // fallback to legacy path
    let legacy_path = std::path::Path::new(config::VOSK_MODEL_PATH);
    if legacy_path.exists() {
        return Ok(legacy_path.to_path_buf());
    }
    
    Err("No Vosk models found".into())
}
