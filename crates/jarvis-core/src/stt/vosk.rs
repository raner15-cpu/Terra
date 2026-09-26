use once_cell::sync::OnceCell;
use vosk::{DecodingState, Recognizer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use parking_lot::Mutex;

use crate::{vosk_models, i18n, config, models};
use crate::models::vosk::VoskModel;
use crate::DB;

// the model Arc keeps the vosk::Model alive for the recognizers
static VOSK_MODEL: OnceCell<Arc<VoskModel>> = OnceCell::new();
static WAKE_RECOGNIZER: OnceCell<Mutex<Recognizer>> = OnceCell::new();
static SPEECH_RECOGNIZER: OnceCell<Mutex<Recognizer>> = OnceCell::new();
static SPEECH_ACCUMULATOR: OnceCell<Mutex<SpeechAccumulator>> = OnceCell::new();
static CONVERSATION_ENDPOINT: AtomicBool = AtomicBool::new(false);
static HINT_WARNED: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct SpeechAccumulator {
    segments: Vec<String>,
    silence_frames: u32,
    /// Frames since the current utterance started accumulating audio.
    utterance_frames: u32,
    /// Frames of the current utterance that contained speech.
    speech_frames: u32,
    noise_floor: f32,
    /// Loudness of actual speech, used to separate speech from a loud noise floor.
    speech_level: f32,
    frames_seen: u64,
    /// How often the application VAD claimed "voice". A VAD that always says
    /// "voice" (loud mic, high gain) carries no information and must be ignored.
    hint_frames: u64,
    hint_voice_frames: u64,
}

const SPEECH_SAMPLE_RATE: f32 = 16_000.0;
const SPEECH_FRAME_LENGTH: f32 = 512.0;
/// Quiet time that ends a command (short: commands are single phrases).
const COMMAND_END_SILENCE_SECONDS: f32 = 0.9;
/// Quiet time that ends a conversation turn. People pause while thinking, so this
/// has to be clearly longer than a natural mid-sentence pause.
const CONVERSATION_END_SILENCE_SECONDS: f32 = 2.2;
/// Safety cap: close the utterance even if silence is never detected (noisy mic).
/// Must stay clearly below config::CMS_WAIT_DELAY, otherwise the command loop
/// times out at the same moment and the recognized text is thrown away.
const COMMAND_MAX_UTTERANCE_SECONDS: f32 = 8.0;
const CONVERSATION_MAX_UTTERANCE_SECONDS: f32 = 60.0;
/// A turn shorter than this is ignored (door slams, coughs, mic pops).
const MIN_SPEECH_SECONDS: f32 = 0.35;
/// Absolute minimum RMS that may be treated as speech, regardless of noise floor.
const MIN_SPEECH_RMS: f32 = 45.0;
/// A frame counts as speech when it is this much louder than the measured noise floor.
const NOISE_FLOOR_SPEECH_FACTOR: f32 = 2.2;
/// Lowest noise floor estimate we allow (keeps the factor meaningful on clean mics).
const NOISE_FLOOR_MIN: f32 = 8.0;
/// Speech is expected to be at least this much louder than the noise floor for
/// the loudness-based split to be trusted.
const MIN_SPEECH_TO_NOISE_RATIO: f32 = 2.5;
/// The application VAD is ignored once it reports "voice" for this share of frames.
const HINT_USELESS_RATIO: f64 = 0.95;
/// ...but only after enough frames to judge it (about 10 seconds).
const HINT_MIN_FRAMES: u64 = 300;
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

/// Switch endpointing between short command phrases and long conversation turns.
pub fn set_conversation_endpointing(active: bool) {
    CONVERSATION_ENDPOINT.store(active, Ordering::Relaxed);
}

pub fn conversation_endpointing() -> bool {
    CONVERSATION_ENDPOINT.load(Ordering::Relaxed)
}

/// Feed one frame into the command/conversation recognizer.
///
/// `voice_hint` is the decision of the application VAD for the same frame. It is
/// only trusted while it actually discriminates: on a loud mic it reports "voice"
/// on every frame, and blindly trusting it kept every utterance open forever.
/// Endpointing itself is loudness-based (noise floor vs. speech level), so pauses
/// stay detectable no matter how loud the noise floor is.
///
/// An utterance is closed only by real silence (or by a long safety cap). It is
/// never closed just because Vosk stopped producing new words - that used to cut
/// people off mid-sentence on hard or unknown vocabulary.
///
/// In conversation mode the result may be an empty string: it signals "the turn is
/// over, the audio is yours", which lets Whisper transcribe speech that the Vosk
/// command model could not decode at all.
pub fn recognize_speech_with_vad(data: &[i16], voice_hint: Option<bool>) -> Option<String> {
    let mut recognizer = SPEECH_RECOGNIZER.get()?.lock();
    let mut accumulator = SPEECH_ACCUMULATOR.get()?.lock();

    let conversation = conversation_endpointing();
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

    // speech level: rises fast, decays slowly
    if accumulator.speech_level <= 0.0 {
        accumulator.speech_level = rms;
    } else if rms > accumulator.speech_level {
        accumulator.speech_level = accumulator.speech_level * 0.7 + rms * 0.3;
    } else {
        accumulator.speech_level = accumulator.speech_level * 0.995 + rms * 0.005;
    }

    // Once loud speech and quiet pauses are clearly separated, split them by
    // loudness instead of by an absolute threshold. This is what makes pauses
    // detectable on a mic whose noise floor is louder than any fixed threshold.
    let ratio = accumulator.speech_level / accumulator.noise_floor;
    let adaptive_threshold = if ratio >= MIN_SPEECH_TO_NOISE_RATIO {
        let midpoint = (accumulator.noise_floor * accumulator.speech_level).sqrt();
        let low = accumulator.noise_floor * 1.5;
        let high = accumulator.speech_level * 0.5;
        if low <= high {
            midpoint.max(low).min(high)
        } else {
            midpoint
        }
    } else {
        (accumulator.noise_floor * NOISE_FLOOR_SPEECH_FACTOR).max(MIN_SPEECH_RMS)
    };

    // Judge the application VAD before trusting it: a hint that is always "voice"
    // used to keep every utterance open forever, so Terra only ever heard the
    // wake word and never a command.
    accumulator.hint_frames += 1;
    if voice_hint.unwrap_or(false) {
        accumulator.hint_voice_frames += 1;
    }
    let hint_share = if accumulator.hint_frames > 0 {
        accumulator.hint_voice_frames as f64 / accumulator.hint_frames as f64
    } else {
        0.0
    };
    let hint_usable =
        accumulator.hint_frames < HINT_MIN_FRAMES || hint_share < HINT_USELESS_RATIO;

    if !hint_usable && !HINT_WARNED.swap(true, Ordering::Relaxed) {
        warn!(
            "Application VAD reports voice in {:.0}% of frames (mic is loud or gain is high). \
Ignoring it and using loudness-based endpointing (floor {:.0}, speech level {:.0}).",
            hint_share * 100.0,
            accumulator.noise_floor,
            accumulator.speech_level,
        );
    }

    let is_speech = rms >= adaptive_threshold || (hint_usable && voice_hint.unwrap_or(false));

    if is_speech {
        accumulator.silence_frames = 0;
        accumulator.speech_frames += 1;
    } else {
        accumulator.silence_frames += 1;
    }

    if is_speech || accumulator.speech_frames > 0 {
        accumulator.utterance_frames += 1;
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
            }
        }
    }

    let silence_threshold = seconds_to_frames(if conversation {
        CONVERSATION_END_SILENCE_SECONDS
    } else {
        COMMAND_END_SILENCE_SECONDS
    });
    let max_utterance_frames = seconds_to_frames(if conversation {
        CONVERSATION_MAX_UTTERANCE_SECONDS
    } else {
        COMMAND_MAX_UTTERANCE_SECONDS
    });
    let min_speech_frames = seconds_to_frames(MIN_SPEECH_SECONDS);

    if accumulator.frames_seen % DEBUG_LOG_EVERY_FRAMES == 0 {
        debug!(
            "STT frame: mode={} rms={:.0} floor={:.0} level={:.0} thr={:.0} speech={} silence={}/{} turn={}f speech_frames={} segments={} vad_hint={:.0}%{}",
            if conversation { "conversation" } else { "command" },
            rms,
            accumulator.noise_floor,
            accumulator.speech_level,
            adaptive_threshold,
            is_speech,
            accumulator.silence_frames,
            silence_threshold,
            accumulator.utterance_frames,
            accumulator.speech_frames,
            accumulator.segments.len(),
            hint_share * 100.0,
            if hint_usable { "" } else { " (ignored)" },
        );
    }

    let heard_enough = accumulator.speech_frames >= min_speech_frames;
    let has_text = !accumulator.segments.is_empty();
    // in conversation mode the audio alone is enough: Whisper will decode it
    let has_material = has_text || (conversation && heard_enough);

    let ended_by_silence = accumulator.silence_frames >= silence_threshold && has_material;
    let ended_by_cap = accumulator.utterance_frames >= max_utterance_frames && has_material;

    if ended_by_silence || ended_by_cap {
        let utterance = accumulator.segments.join(" ").trim().to_string();
        let turn_seconds =
            accumulator.utterance_frames as f32 * SPEECH_FRAME_LENGTH / SPEECH_SAMPLE_RATE;
        accumulator.segments.clear();
        accumulator.silence_frames = 0;
        accumulator.utterance_frames = 0;
        accumulator.speech_frames = 0;
        info!(
            "Utterance closed ({}, {:.1}s): '{}'",
            if ended_by_silence { "silence" } else { "length cap" },
            turn_seconds,
            utterance
        );

        if utterance.is_empty() && !conversation {
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
        accumulator.utterance_frames = 0;
        accumulator.speech_frames = 0;
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
