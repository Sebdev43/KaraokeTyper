use enigo::{Enigo, Keyboard, Settings};
use std::thread;
use std::time::Duration;

pub struct TextOutput {
    enigo: Enigo,
    key_delay_ms: u64,
}

impl TextOutput {
    pub fn new(key_delay_ms: u64) -> Result<Self, String> {
        let enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("Failed to initialize Enigo: {}", e))?;

        Ok(Self { enigo, key_delay_ms })
    }

    pub fn type_text(&mut self, text: &str) -> Result<(), String> {
        for c in text.chars() {
            self.enigo
                .text(&c.to_string())
                .map_err(|e| format!("Failed to type character: {}", e))?;

            if self.key_delay_ms > 0 {
                thread::sleep(Duration::from_millis(self.key_delay_ms));
            }
        }

        Ok(())
    }
}
