//! Application error type. Serialized to the frontend as `{ kind, message }`
//! (mirrors `AppErrorPayload` in `packages/shared/src/models.ts`).

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Datenbankfehler: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Datenbank-Migration fehlgeschlagen: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("Dateisystemfehler: {0}")]
    Io(#[from] std::io::Error),
    #[error("Ungültige Daten: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("„{0}“ ist nicht in deiner Bibliothek.")]
    NotInLibrary(String),
    #[error("„{0}“ ist bereits in deiner Bibliothek.")]
    AlreadyInLibrary(String),
    #[error("{0}")]
    InvalidInput(String),
    #[error("Das Spiel läuft bereits.")]
    AlreadyRunning(String),
    #[error("Das Spiel läuft nicht.")]
    NotRunning(String),
    #[error("Start fehlgeschlagen: {0}")]
    Launch(String),
    #[error("{0} wurde nicht gefunden. Ist es installiert?")]
    CompatToolNotFound(String),
    #[error("Noch nicht verfügbar: {0}")]
    NotSupported(String),
    #[error("Offline – {0}")]
    Offline(String),
    #[error("Der Katalog ist noch nicht eingerichtet. Trage unter Einstellungen → Katalog Zugangsdaten für IGDB oder RAWG ein.")]
    CatalogNotConfigured,
    #[error("Anmeldung beim Katalog fehlgeschlagen: {0}")]
    CatalogAuth(String),
    #[error("Katalog-Anfrage fehlgeschlagen: {0}")]
    CatalogProvider(String),
    #[error("Schlüsselbund nicht erreichbar: {0}")]
    Keyring(String),
}

impl AppError {
    /// Stable machine-readable kind (camelCase), mirrors `AppErrorKind` in TS.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Database(_) => "database",
            Self::Migration(_) => "migration",
            Self::Io(_) => "io",
            Self::Serialization(_) => "serialization",
            Self::NotInLibrary(_) => "notInLibrary",
            Self::AlreadyInLibrary(_) => "alreadyInLibrary",
            Self::InvalidInput(_) => "invalidInput",
            Self::AlreadyRunning(_) => "alreadyRunning",
            Self::NotRunning(_) => "notRunning",
            Self::Launch(_) => "launch",
            Self::CompatToolNotFound(_) => "compatToolNotFound",
            Self::NotSupported(_) => "notSupported",
            Self::Offline(_) => "offline",
            Self::CatalogNotConfigured => "catalogNotConfigured",
            Self::CatalogAuth(_) => "catalogAuth",
            Self::CatalogProvider(_) => "catalogProvider",
            Self::Keyring(_) => "keyring",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_kind_and_message() {
        let json = serde_json::to_value(AppError::NotInLibrary("Tidewater".into())).unwrap();
        assert_eq!(json["kind"], "notInLibrary");
        assert_eq!(
            json["message"],
            "„Tidewater“ ist nicht in deiner Bibliothek."
        );
    }
}
