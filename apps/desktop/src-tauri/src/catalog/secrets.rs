//! Catalogue credentials in the OS keyring (Secret Service / macOS Keychain /
//! Windows Credential Manager). Secrets never leave the core.

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::{Mutex, PoisonError};

use crate::error::{AppError, AppResult};
use crate::models::{CatalogCredentials, CatalogProviderId, ProvidersConfigured};

pub const KEYRING_SERVICE: &str = "app.cove.desktop";

const IGDB_ID: &str = "igdb-client-id";
const IGDB_SECRET: &str = "igdb-client-secret";
const RAWG_KEY: &str = "rawg-api-key";

/// Blocking key/value secret storage.
pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> AppResult<Option<String>>;
    fn set(&self, key: &str, value: &str) -> AppResult<()>;
    fn delete(&self, key: &str) -> AppResult<()>;
}

/// The real OS keyring, entries under one service name.
pub struct KeyringStore(&'static str);

impl KeyringStore {
    /// Cove's own entries (`app.cove.desktop`).
    pub const fn app() -> Self {
        Self(KEYRING_SERVICE)
    }

    /// Separate entries for tests against the real keyring.
    #[cfg(test)]
    pub const fn testing() -> Self {
        Self("app.cove.desktop.test")
    }
}

/// One OS keyring entry per key, without size handling.
struct RawKeyring(&'static str);

impl RawKeyring {
    fn entry(&self, key: &str) -> AppResult<keyring::Entry> {
        keyring::Entry::new(self.0, key).map_err(|e| AppError::Keyring(e.to_string()))
    }
}

/// Runs a keyring call on a plain thread without a tokio context.
///
/// The Secret Service backend (zbus with the `tokio` feature) drives every call
/// with its own `Runtime::block_on`, which panics on a thread that already runs
/// tokio tasks ("Cannot start a runtime from within a runtime"). Commands call
/// the keyring from async code, so it must never run on the caller's thread.
fn off_runtime<T: Send>(f: impl FnOnce() -> AppResult<T> + Send) -> AppResult<T> {
    std::thread::scope(|scope| scope.spawn(f).join())
        .map_err(|_| AppError::Keyring("keyring call panicked".into()))?
}

impl SecretStore for RawKeyring {
    fn get(&self, key: &str) -> AppResult<Option<String>> {
        off_runtime(|| match self.entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::Keyring(e.to_string())),
        })
    }

    fn set(&self, key: &str, value: &str) -> AppResult<()> {
        off_runtime(|| {
            self.entry(key)?
                .set_password(value)
                .map_err(|e| AppError::Keyring(e.to_string()))
        })
    }

    fn delete(&self, key: &str) -> AppResult<()> {
        off_runtime(|| match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::Keyring(e.to_string())),
        })
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, key: &str) -> AppResult<Option<String>> {
        chunked::get(&RawKeyring(self.0), key)
    }

    fn set(&self, key: &str, value: &str) -> AppResult<()> {
        chunked::set(&RawKeyring(self.0), key, value)
    }

    fn delete(&self, key: &str) -> AppResult<()> {
        chunked::delete(&RawKeyring(self.0), key)
    }
}

/// Long secrets split over several entries.
///
/// Windows' Credential Manager holds at most 2560 bytes per entry, stored as
/// UTF-16, so about 1280 characters; an Epic session (access + refresh token)
/// is longer. A long value is written as parts `<key>#0`, `<key>#1`, … and
/// `<key>` itself only holds the marker with the part count.
mod chunked {
    use super::SecretStore;
    use crate::error::{AppError, AppResult};

    /// Characters per entry: even 4-byte characters stay within 2560 bytes.
    pub const PART: usize = 600;
    const MARKER: &str = "cove-parts:";

    fn part_key(key: &str, index: usize) -> String {
        format!("{key}#{index}")
    }

    fn part_count(stored: Option<&str>) -> usize {
        stored
            .and_then(|v| v.strip_prefix(MARKER))
            .and_then(|n| n.parse().ok())
            .unwrap_or(0)
    }

    pub fn get(raw: &dyn SecretStore, key: &str) -> AppResult<Option<String>> {
        let Some(stored) = raw.get(key)? else {
            return Ok(None);
        };
        let count = part_count(Some(&stored));
        if count == 0 {
            return Ok(Some(stored));
        }
        let mut value = String::new();
        for index in 0..count {
            let part = raw
                .get(&part_key(key, index))?
                .ok_or_else(|| AppError::Keyring(format!("{key}: part {index} missing")))?;
            value.push_str(&part);
        }
        Ok(Some(value))
    }

    pub fn set(raw: &dyn SecretStore, key: &str, value: &str) -> AppResult<()> {
        let old = part_count(raw.get(key)?.as_deref());
        let chars: Vec<char> = value.chars().collect();
        let new = if chars.len() <= PART {
            raw.set(key, value)?;
            0
        } else {
            let parts: Vec<String> = chars.chunks(PART).map(|c| c.iter().collect()).collect();
            // Parts first: the marker only appears once all of them are there.
            for (index, part) in parts.iter().enumerate() {
                raw.set(&part_key(key, index), part)?;
            }
            raw.set(key, &format!("{MARKER}{}", parts.len()))?;
            parts.len()
        };
        for index in new..old {
            raw.delete(&part_key(key, index))?;
        }
        Ok(())
    }

    pub fn delete(raw: &dyn SecretStore, key: &str) -> AppResult<()> {
        let count = part_count(raw.get(key)?.as_deref());
        raw.delete(key)?;
        for index in 0..count {
            raw.delete(&part_key(key, index))?;
        }
        Ok(())
    }
}

/// In-memory store for tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> AppResult<Option<String>> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .cloned())
    }

    fn set(&self, key: &str, value: &str) -> AppResult<()> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, key: &str) -> AppResult<()> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(key);
        Ok(())
    }
}

/// Reads the stored credentials of one provider (None if incomplete).
pub fn load(
    store: &dyn SecretStore,
    provider: CatalogProviderId,
) -> AppResult<Option<CatalogCredentials>> {
    Ok(match provider {
        CatalogProviderId::Igdb => match (store.get(IGDB_ID)?, store.get(IGDB_SECRET)?) {
            (Some(client_id), Some(client_secret)) => Some(CatalogCredentials::Igdb {
                client_id,
                client_secret,
            }),
            _ => None,
        },
        CatalogProviderId::Rawg => store
            .get(RAWG_KEY)?
            .map(|api_key| CatalogCredentials::Rawg { api_key }),
    })
}

pub fn save(store: &dyn SecretStore, credentials: &CatalogCredentials) -> AppResult<()> {
    match credentials {
        CatalogCredentials::Igdb {
            client_id,
            client_secret,
        } => {
            store.set(IGDB_ID, client_id.trim())?;
            store.set(IGDB_SECRET, client_secret.trim())
        }
        CatalogCredentials::Rawg { api_key } => store.set(RAWG_KEY, api_key.trim()),
    }
}

pub fn clear(store: &dyn SecretStore, provider: CatalogProviderId) -> AppResult<()> {
    match provider {
        CatalogProviderId::Igdb => {
            store.delete(IGDB_ID)?;
            store.delete(IGDB_SECRET)
        }
        CatalogProviderId::Rawg => store.delete(RAWG_KEY),
    }
}

pub fn configured(store: &dyn SecretStore) -> AppResult<ProvidersConfigured> {
    Ok(ProvidersConfigured {
        igdb: load(store, CatalogProviderId::Igdb)?.is_some(),
        rawg: load(store, CatalogProviderId::Rawg)?.is_some(),
    })
}

/// Rejects obviously incomplete input before contacting the provider.
pub fn validate(credentials: &CatalogCredentials) -> AppResult<()> {
    let empty = match credentials {
        CatalogCredentials::Igdb {
            client_id,
            client_secret,
        } => client_id.trim().is_empty() || client_secret.trim().is_empty(),
        CatalogCredentials::Rawg { api_key } => api_key.trim().is_empty(),
    };
    if empty {
        return Err(AppError::InvalidInput(tr!(
            "Bitte fülle alle Felder aus.",
            "Please fill in all fields."
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_clear() {
        let store = MemoryStore::default();
        assert_eq!(
            configured(&store).unwrap(),
            ProvidersConfigured {
                igdb: false,
                rawg: false
            }
        );
        save(
            &store,
            &CatalogCredentials::Igdb {
                client_id: " id ".into(),
                client_secret: "secret".into(),
            },
        )
        .unwrap();
        assert_eq!(
            load(&store, CatalogProviderId::Igdb).unwrap(),
            Some(CatalogCredentials::Igdb {
                client_id: "id".into(),
                client_secret: "secret".into()
            })
        );
        assert!(configured(&store).unwrap().igdb);
        clear(&store, CatalogProviderId::Igdb).unwrap();
        assert!(load(&store, CatalogProviderId::Igdb).unwrap().is_none());
        assert!(validate(&CatalogCredentials::Rawg {
            api_key: "  ".into()
        })
        .is_err());
    }

    /// Regression: the Secret Service backend starts its own runtime per call,
    /// which used to panic inside async commands.
    #[tokio::test(flavor = "multi_thread")]
    async fn keyring_calls_may_start_their_own_runtime() {
        let value = off_runtime(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .map_err(|e| AppError::Keyring(e.to_string()))?;
            Ok(runtime.block_on(async { 42 }))
        })
        .unwrap();
        assert_eq!(value, 42);
    }

    /// Like Windows' Credential Manager: refuses entries over 1280 characters.
    #[derive(Default)]
    struct LimitedStore(MemoryStore);

    impl SecretStore for LimitedStore {
        fn get(&self, key: &str) -> AppResult<Option<String>> {
            self.0.get(key)
        }
        fn set(&self, key: &str, value: &str) -> AppResult<()> {
            if value.encode_utf16().count() * 2 > 2560 {
                return Err(AppError::Keyring("TooLong".into()));
            }
            self.0.set(key, value)
        }
        fn delete(&self, key: &str) -> AppResult<()> {
            self.0.delete(key)
        }
    }

    fn entries(store: &LimitedStore) -> usize {
        store.0 .0.lock().unwrap().len()
    }

    #[test]
    fn long_secrets_are_split_into_parts() {
        let raw = LimitedStore::default();
        let long: String = "EG1~ä😀".repeat(1000); // about 7000 chars, with 4-byte characters
        assert!(raw.set("k", &long).is_err(), "too long for one entry");

        chunked::set(&raw, "k", &long).unwrap();
        assert_eq!(
            chunked::get(&raw, "k").unwrap().as_deref(),
            Some(long.as_str())
        );
        let parts = long.chars().count().div_ceil(chunked::PART);
        assert_eq!(entries(&raw), parts + 1);

        // Shorter again: one plain entry, the old parts are gone.
        chunked::set(&raw, "k", "short").unwrap();
        assert_eq!(chunked::get(&raw, "k").unwrap().as_deref(), Some("short"));
        assert_eq!(entries(&raw), 1);

        chunked::set(&raw, "k", &long).unwrap();
        chunked::delete(&raw, "k").unwrap();
        assert_eq!(chunked::get(&raw, "k").unwrap(), None);
        assert_eq!(entries(&raw), 0);
    }

    /// Talks to the real OS keyring. Run manually: `cargo test real_keyring -- --ignored`.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs a desktop session with a keyring"]
    async fn real_keyring_roundtrip() {
        let key = "cove-test-entry";
        let store = KeyringStore::testing();
        store.set(key, "value").unwrap();
        assert_eq!(store.get(key).unwrap().as_deref(), Some("value"));
        store.delete(key).unwrap();
        assert_eq!(store.get(key).unwrap(), None);

        // An Epic-sized session goes over several entries.
        let session = format!("{{\"accessToken\":\"{}\"}}", "x".repeat(5000));
        store.set(key, &session).unwrap();
        assert_eq!(store.get(key).unwrap().as_deref(), Some(session.as_str()));
        store.delete(key).unwrap();
        assert_eq!(store.get(key).unwrap(), None);
    }
}
