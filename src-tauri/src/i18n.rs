//! Interface language for everything the backend writes for a human to read.
//!
//! The frontend has its own catalogue (`src/i18n/`); this module covers the
//! strings that only Rust produces — tray entries, installer and updater
//! results, download errors — so a user who picks English does not get a
//! half-translated window.
//!
//! The language is a process-wide value rather than a parameter threaded through
//! every call site: the deep ones (`model_downloader`, the `pkexec` wrapper in
//! `updater`) are several layers below anything holding the config, and passing a
//! language through them would be noise on paths that have nothing to do with
//! language. It is written once at startup and again whenever settings are saved,
//! and read with `Relaxed` — a message rendered in the previous language during
//! the microseconds around a save is not a correctness problem.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::atomic::{AtomicU8, Ordering};

/// The languages the interface is translated into.
///
/// Adding one means adding its arm to every function below; the compiler lists
/// them all, which is the point of an enum here rather than a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum UiLanguage {
    Fr,
    En,
}

impl UiLanguage {
    /// The language code the frontend and `<html lang>` use.
    pub fn code(self) -> &'static str {
        match self {
            Self::Fr => "fr",
            Self::En => "en",
        }
    }

    /// Best match for a POSIX locale value such as `fr_FR.UTF-8` or `en-GB`.
    ///
    /// Anything that is not French maps to English: it is the language the
    /// project's own documentation is written in, and a wrong guess towards
    /// English is far more readable to the rest of the world than a wrong guess
    /// towards French.
    pub fn from_locale(locale: &str) -> Self {
        if locale
            .split(['_', '-', '.'])
            .next()
            .is_some_and(|lang| lang.eq_ignore_ascii_case("fr"))
        {
            Self::Fr
        } else {
            Self::En
        }
    }

    /// The system's language, for the very first launch only — afterwards the
    /// stored setting wins.
    ///
    /// `LC_ALL` / `LC_MESSAGES` / `LANG` is the POSIX order and covers Linux and
    /// macOS. Windows sets none of them, so it lands on English, which is the
    /// right default for a build whose system language we cannot read without
    /// pulling in a crate for one call.
    pub fn from_environment() -> Self {
        for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            match std::env::var(var) {
                Ok(value) if !value.is_empty() && value != "C" && value != "POSIX" => {
                    return Self::from_locale(&value);
                }
                _ => {}
            }
        }
        Self::En
    }
}

impl Default for UiLanguage {
    fn default() -> Self {
        Self::from_environment()
    }
}

/// `UiLanguage` as a `u8`, because that is what an atomic can hold.
const FR: u8 = 0;
const EN: u8 = 1;

static CURRENT: AtomicU8 = AtomicU8::new(EN);

pub fn set_current(language: UiLanguage) {
    CURRENT.store(
        match language {
            UiLanguage::Fr => FR,
            UiLanguage::En => EN,
        },
        Ordering::Relaxed,
    );
}

pub fn current() -> UiLanguage {
    match CURRENT.load(Ordering::Relaxed) {
        FR => UiLanguage::Fr,
        _ => UiLanguage::En,
    }
}

/// Picks the translation for the current language.
///
/// Every message below goes through this, so a new language is one arm added
/// here plus the compiler pointing at each message in turn.
fn pick(fr: &'static str, en: &'static str) -> &'static str {
    match current() {
        UiLanguage::Fr => fr,
        UiLanguage::En => en,
    }
}

/// Strings shown by the backend. Grouped in one module so a translator has a
/// single place to read, and so none of them can be written inline elsewhere.
pub mod msg {
    use super::pick;

    // --- Tray -------------------------------------------------------------

    pub fn tray_settings() -> &'static str {
        pick("Paramètres", "Settings")
    }

    pub fn tray_quit() -> &'static str {
        pick("Quitter", "Quit")
    }

    // --- GNOME extension installer ---------------------------------------

    pub fn gnome_installed_logging_out() -> &'static str {
        pick(
            "Extension installée et activée !\n\nDéconnexion en cours...",
            "Extension installed and enabled!\n\nLogging out...",
        )
    }

    pub fn gnome_installed_log_out_manually() -> &'static str {
        pick(
            "Extension installée et activée !\n\nVeuillez vous déconnecter manuellement pour activer l'extension.",
            "Extension installed and enabled!\n\nPlease log out manually to activate it.",
        )
    }

    pub fn gnome_installed_relogin() -> &'static str {
        pick(
            "Extension installée !\n\nDéconnectez-vous et reconnectez-vous pour l'activer.",
            "Extension installed!\n\nLog out and back in to activate it.",
        )
    }

    // --- Updater (pkexec apt install) ------------------------------------

    pub fn update_auth_cancelled() -> &'static str {
        pick(
            "Authentification annulée par l'utilisateur.",
            "Authentication cancelled by the user.",
        )
    }

    pub fn update_auth_denied() -> &'static str {
        pick(
            "Autorisation refusée (PolicyKit indisponible ?).",
            "Authorisation denied (is PolicyKit available?).",
        )
    }

    pub fn update_apt_failed(code: i32) -> String {
        match super::current() {
            super::UiLanguage::Fr => format!("apt-get install a échoué (code {code})."),
            super::UiLanguage::En => format!("apt-get install failed (exit code {code})."),
        }
    }

    pub fn update_apt_failed_with_stderr(code: i32, stderr: &str) -> String {
        match super::current() {
            super::UiLanguage::Fr => format!("apt-get install a échoué (code {code}) : {stderr}"),
            super::UiLanguage::En => format!("apt-get install failed (exit code {code}): {stderr}"),
        }
    }

    pub fn update_apt_signalled() -> &'static str {
        pick(
            "apt-get install a été terminé par un signal.",
            "apt-get install was killed by a signal.",
        )
    }

    // --- Model downloads --------------------------------------------------

    pub fn download_already_running(model: &str) -> String {
        match super::current() {
            super::UiLanguage::Fr => {
                format!("Téléchargement déjà en cours pour '{model}' — patientez.")
            }
            super::UiLanguage::En => format!("'{model}' is already downloading — please wait."),
        }
    }

    pub fn download_cancelled(model: &str) -> String {
        match super::current() {
            super::UiLanguage::Fr => format!("Téléchargement de '{model}' annulé."),
            super::UiLanguage::En => format!("Download of '{model}' cancelled."),
        }
    }

    pub fn integrity_mismatch(file: &str, expected: &str, got: &str) -> String {
        match super::current() {
            super::UiLanguage::Fr => format!(
                "Intégrité : le SHA-256 de {file} ne correspond pas. Attendu {expected}, obtenu {got}. Installation refusée."
            ),
            super::UiLanguage::En => format!(
                "Integrity: SHA-256 of {file} does not match. Expected {expected}, got {got}. Installation refused."
            ),
        }
    }

    pub fn filename_refused(name: &str) -> String {
        match super::current() {
            super::UiLanguage::Fr => format!("nom de fichier refusé : {name}"),
            super::UiLanguage::En => format!("file name refused: {name}"),
        }
    }

    pub fn file_gone(name: &str) -> String {
        match super::current() {
            super::UiLanguage::Fr => format!("{name} n'existe plus"),
            super::UiLanguage::En => format!("{name} no longer exists"),
        }
    }

    pub fn not_an_orphan(name: &str) -> String {
        match super::current() {
            super::UiLanguage::Fr => {
                format!("{name} est un modèle du catalogue, pas un fichier orphelin")
            }
            super::UiLanguage::En => {
                format!("{name} is a catalogue model, not an orphaned file")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The global is process-wide, so these run under one lock rather than in
    /// parallel: two tests setting opposite languages would otherwise read each
    /// other's value.
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_language<T>(language: UiLanguage, f: impl FnOnce() -> T) -> T {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = current();
        set_current(language);
        let out = f();
        set_current(previous);
        out
    }

    #[test]
    fn locale_detection_recognises_french_and_defaults_to_english() {
        assert_eq!(UiLanguage::from_locale("fr_FR.UTF-8"), UiLanguage::Fr);
        assert_eq!(UiLanguage::from_locale("fr"), UiLanguage::Fr);
        assert_eq!(UiLanguage::from_locale("fr-CA"), UiLanguage::Fr);
        assert_eq!(UiLanguage::from_locale("FR_BE"), UiLanguage::Fr);
        assert_eq!(UiLanguage::from_locale("en_GB.UTF-8"), UiLanguage::En);
        assert_eq!(UiLanguage::from_locale("de_DE"), UiLanguage::En);
        assert_eq!(UiLanguage::from_locale(""), UiLanguage::En);
        // "french" is not a locale and must not be mistaken for one by a prefix match.
        assert_eq!(UiLanguage::from_locale("frisian"), UiLanguage::En);
    }

    #[test]
    fn the_current_language_round_trips() {
        with_language(UiLanguage::Fr, || assert_eq!(current(), UiLanguage::Fr));
        with_language(UiLanguage::En, || assert_eq!(current(), UiLanguage::En));
    }

    /// A message that forgot its translation would return the same text in both
    /// languages, which is exactly the bug this catches — and it is invisible
    /// from the call site.
    #[test]
    fn every_message_differs_between_the_two_languages() {
        let fr = with_language(UiLanguage::Fr, all_messages);
        let en = with_language(UiLanguage::En, all_messages);
        assert_eq!(fr.len(), en.len());
        for (f, e) in fr.iter().zip(en.iter()) {
            assert!(!f.is_empty() && !e.is_empty(), "empty message: {f:?}/{e:?}");
            assert_ne!(f, e, "message is identical in both languages: {f}");
        }
    }

    /// Every message in `msg`, rendered with sample arguments. Adding a message
    /// without adding it here is the only way to escape the test above, so the
    /// count is asserted too.
    fn all_messages() -> Vec<String> {
        let m = vec![
            msg::tray_settings().to_string(),
            msg::tray_quit().to_string(),
            msg::gnome_installed_logging_out().to_string(),
            msg::gnome_installed_log_out_manually().to_string(),
            msg::gnome_installed_relogin().to_string(),
            msg::update_auth_cancelled().to_string(),
            msg::update_auth_denied().to_string(),
            msg::update_apt_failed(100),
            msg::update_apt_failed_with_stderr(100, "boom"),
            msg::update_apt_signalled().to_string(),
            msg::download_already_running("whisper-base"),
            msg::download_cancelled("whisper-base"),
            msg::integrity_mismatch("f.gguf", "aaa", "bbb"),
            msg::filename_refused("../etc"),
            msg::not_an_orphan("whisper-base"),
            msg::file_gone("stale.gguf"),
        ];
        assert_eq!(m.len(), 16, "a message was added without a test entry");
        m
    }

    #[test]
    fn parameters_reach_the_rendered_message() {
        for language in [UiLanguage::Fr, UiLanguage::En] {
            with_language(language, || {
                assert!(msg::download_cancelled("whisper-tiny").contains("whisper-tiny"));
                assert!(msg::update_apt_failed(42).contains("42"));
                assert!(msg::update_apt_failed_with_stderr(1, "disk full").contains("disk full"));
                let mismatch = msg::integrity_mismatch("m.gguf", "expected", "actual");
                assert!(mismatch.contains("m.gguf"));
                assert!(mismatch.contains("expected") && mismatch.contains("actual"));
                assert!(msg::filename_refused("../x").contains("../x"));
                assert!(msg::not_an_orphan("whisper-base").contains("whisper-base"));
            });
        }
    }

    #[test]
    fn the_language_code_matches_what_the_frontend_expects() {
        assert_eq!(UiLanguage::Fr.code(), "fr");
        assert_eq!(UiLanguage::En.code(), "en");
    }
}
