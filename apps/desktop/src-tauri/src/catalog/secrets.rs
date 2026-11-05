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

impl SecretStore for KeyringStore {
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
    }
}
