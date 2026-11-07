//! Minimal translations for text that Rust displays itself (tray menu).
//! The UI is translated in the frontend (`src/lib/i18n`).

use std::sync::OnceLock;

fn is_german() -> bool {
    static DE: OnceLock<bool> = OnceLock::new();
    *DE.get_or_init(|| sys_locale::get_locale().is_some_and(|l| l.to_lowercase().starts_with("de")))
}

pub fn t(key: &str) -> &'static str {
    let de = is_german();
    match key {
        "tray.screenshot" => "Screenshot",
        "save.done" => {
            if de {
                "Screenshot gespeichert:"
            } else {
                "Screenshot saved:"
            }
        }
        "save.recordingDone" => {
            if de {
                "Aufnahme gespeichert:"
            } else {
                "Recording saved:"
            }
        }
        "save.title" => {
            if de {
                "Screenshot speichern"
            } else {
                "Save screenshot"
            }
        }
        "capture.failed" => {
            if de {
                "Der Screenshot konnte nicht aufgenommen werden."
            } else {
                "The screenshot could not be taken."
            }
        }
        "tray.record" => {
            if de {
                "Aufnahme"
            } else {
                "Record"
            }
        }
        "tray.editor" => {
            if de {
                "Editor öffnen"
            } else {
                "Open editor"
            }
        }
        "tray.settings" => {
            if de {
                "Einstellungen"
            } else {
                "Settings"
            }
        }
        "tray.quit" => {
            if de {
                "Beenden"
            } else {
                "Quit"
            }
        }
        _ => "?",
    }
}
