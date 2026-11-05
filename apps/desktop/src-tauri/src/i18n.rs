//! UI language of the core: messages that reach the frontend (errors, store
//! status, dialog texts) follow `settings.general.language`. Log output stays
//! as it is.

use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
thread_local! {
    /// Per-test override, so parallel tests never see each other's language.
    static TEST_ENGLISH: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Applies the language from the settings (`"de"` or `"en"`; anything else is German).
pub fn set_language(language: &str) {
    ENGLISH.store(language == "en", Ordering::Relaxed);
}

/// Whether texts for the UI should be English.
pub fn is_english() -> bool {
    #[cfg(test)]
    if let Some(english) = TEST_ENGLISH.with(std::cell::Cell::get) {
        return english;
    }
    ENGLISH.load(Ordering::Relaxed)
}

/// The language for new installs: German on German systems, English elsewhere.
pub fn system_default() -> &'static str {
    match sys_locale::get_locale() {
        Some(locale) if locale.to_ascii_lowercase().starts_with("de") => "de",
        _ => "en",
    }
}

/// Sets (or with `None` clears) the language override of this thread (async tests).
#[cfg(test)]
pub fn set_thread_english(english: Option<bool>) {
    TEST_ENGLISH.with(|c| c.set(english));
}

/// Runs `f` with English texts on this thread only (tests).
#[cfg(test)]
pub fn in_english<T>(f: impl FnOnce() -> T) -> T {
    TEST_ENGLISH.with(|c| c.set(Some(true)));
    let result = f();
    TEST_ENGLISH.with(|c| c.set(None));
    result
}

/// A UI text in German and English, chosen at run time. Works like `format!`:
/// `tr!("Bei {name} anmelden", "Sign in to {name}")`.
#[macro_export]
macro_rules! tr {
    ($de:literal, $en:literal $(,)?) => {{
        #[allow(clippy::useless_format)]
        let text = if $crate::i18n::is_english() { format!($en) } else { format!($de) };
        text
    }};
    ($de:literal, $en:literal, $($arg:tt)+) => {
        if $crate::i18n::is_english() { format!($en, $($arg)+) } else { format!($de, $($arg)+) }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_text_of_the_current_language() {
        let store = "GOG";
        assert_eq!(
            tr!("Bei {store} anmelden", "Sign in to {store}"),
            "Bei GOG anmelden"
        );
        assert_eq!(in_english(|| tr!("{} Spiele", "{} games", 3)), "3 games");
        assert_eq!(
            tr!("Hallo", "Hello"),
            "Hallo",
            "back to German after the override"
        );
    }
}
