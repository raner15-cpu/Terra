use std::sync::Mutex;

use once_cell::sync::OnceCell;
use rustpotter::Rustpotter;

use crate::{config, APP_DIR};

// store rustpotter instance
static RUSTPOTTER: OnceCell<Mutex<Rustpotter>> = OnceCell::new();

pub fn init() -> Result<(), ()> {
    let rustpotter_config = config::RUSTPOTTER_DEFAULT_CONFIG;

    // create rustpotter instance
    match Rustpotter::new(&rustpotter_config) {
        Ok(mut rinstance) => {
            // success
            let model_path = APP_DIR.join("resources/rustpotter/terra.rpw");
            if !model_path.is_file() {
                return Err(());
            }
            let model_path = model_path.to_string_lossy().into_owned();
            if let Err(e) = rinstance.add_wakeword_from_file(&model_path, "terra") {
                error!("Failed to load Terra wakeword file '{}': {}", model_path, e);
                return Err(());
            }

            // store
            let _ = RUSTPOTTER.set(Mutex::new(rinstance));
        }
        Err(msg) => {
            error!("Rustpotter failed to initialize.\nError details: {}", msg);

            return Err(());
        }
    }

    Ok(())
}

pub fn data_callback(frame_buffer: &[i16]) -> Option<i32> {
    let mut lock = RUSTPOTTER.get().unwrap().lock();
    let rustpotter = lock.as_mut().unwrap();
    // let detection = rustpotter.process_samples(frame_buffer.to_vec()); // @TODO. Temp crutch. Fix optimization issue, frame_buffer should not be copied to a new vector!
    let detection = rustpotter.process_samples(frame_buffer);

    // info!("Ruspotter data callback");

    if let Some(detection) = detection {
        if detection.score > config::RUSPOTTER_MIN_SCORE {
            info!("Rustpotter detection info:\n{:?}", detection);

            return Some(0);
        } else {
            info!("Rustpotter detection info:\n{:?}", detection)
        }
    }

    None
}
