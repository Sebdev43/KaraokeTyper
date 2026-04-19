use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};

static START_SOUND: &[u8] = include_bytes!("../resources/sounds/start.wav");
static STOP_SOUND: &[u8] = include_bytes!("../resources/sounds/stop.wav");

// Guard: only one sound at a time to prevent thread accumulation
static PLAYING: AtomicBool = AtomicBool::new(false);

pub fn play_start_sound(volume: f32) {
    play_embedded_sound(START_SOUND, volume);
}

pub fn play_stop_sound(volume: f32) {
    play_embedded_sound(STOP_SOUND, volume);
}

fn play_embedded_sound(data: &'static [u8], volume: f32) {
    // Skip if another sound is already playing
    if PLAYING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    std::thread::spawn(move || {
        if let Err(e) = play_sound_blocking(data, volume) {
            crate::dev_log!("[AUDIO_FEEDBACK] Playback error: {}", e);
        }
        PLAYING.store(false, Ordering::SeqCst);
    });
}

fn play_sound_blocking(data: &'static [u8], volume: f32) -> Result<(), String> {
    use rodio::{Decoder, OutputStream, Sink};

    let (_stream, stream_handle) =
        OutputStream::try_default().map_err(|e| format!("No audio output: {}", e))?;

    let sink = Sink::try_new(&stream_handle).map_err(|e| format!("Sink error: {}", e))?;
    sink.set_volume(volume.clamp(0.0, 1.0));

    let cursor = Cursor::new(data);
    let source = Decoder::new(cursor).map_err(|e| format!("Decode error: {}", e))?;
    sink.append(source);
    sink.sleep_until_end();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_start_sound_embedded() {
        assert!(!START_SOUND.is_empty());
        assert!(START_SOUND.len() > 44); // WAV header minimum
    }

    #[test]
    fn test_stop_sound_embedded() {
        assert!(!STOP_SOUND.is_empty());
        assert!(STOP_SOUND.len() > 44);
    }
}
