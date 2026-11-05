//! Anti-CSRF `state` values for login flows.
//!
//! No PKCE: Steam uses `OpenID` 2.0 (verified via `check_authentication`) and
//! the GOG/Epic launcher clients do not accept a code challenge. Their codes
//! arrive in Cove's own login window, never through a redirect another app
//! could receive.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;

use crate::error::{AppError, AppResult};

fn random_token(bytes: usize) -> AppResult<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| {
        AppError::Launch(tr!(
            "Zufallsquelle nicht verfügbar: {e}",
            "No source of randomness available: {e}"
        ))
    })?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// Unguessable `state` for one login attempt (256 bit).
pub fn new_state() -> AppResult<String> {
    random_token(32)
}

/// Compares two secrets without leaking the position of the first difference.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_are_random_and_url_safe() {
        let a = new_state().unwrap();
        assert_eq!(a.len(), 43);
        assert!(a
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_ne!(a, new_state().unwrap());
    }

    #[test]
    fn constant_time_comparison() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
    }
}
