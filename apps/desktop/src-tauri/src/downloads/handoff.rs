//! Store games are never downloaded by Cove: the installation is handed to
//! the store's own client through its URL scheme.

use crate::models::StoreSource;

/// URL that asks the store client to install a game. On Linux, GOG and Epic
/// games go to Heroic (their Linux client).
pub fn install_url(store: StoreSource, store_game_id: &str, windows: bool) -> Option<String> {
    let id =
        percent_encoding::utf8_percent_encode(store_game_id, percent_encoding::NON_ALPHANUMERIC)
            .to_string();
    match store {
        StoreSource::Steam => Some(format!("steam://install/{id}")),
        StoreSource::Epic if windows => {
            Some(format!("com.epicgames.launcher://apps/{id}?action=install"))
        }
        StoreSource::Gog if windows => Some(format!("goggalaxy://openGameView/{id}")),
        StoreSource::Epic => Some(format!("heroic://launch?appName={id}&runner=legendary")),
        StoreSource::Gog => Some(format!("heroic://launch?appName={id}&runner=gog")),
        StoreSource::Manual => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_urls_per_store_and_platform() {
        assert_eq!(
            install_url(StoreSource::Steam, "620", false).as_deref(),
            Some("steam://install/620")
        );
        assert_eq!(
            install_url(StoreSource::Epic, "Sugar", true).as_deref(),
            Some("com.epicgames.launcher://apps/Sugar?action=install")
        );
        assert_eq!(
            install_url(StoreSource::Gog, "1207664663", true).as_deref(),
            Some("goggalaxy://openGameView/1207664663")
        );
        assert_eq!(
            install_url(StoreSource::Gog, "1207664663", false).as_deref(),
            Some("heroic://launch?appName=1207664663&runner=gog")
        );
        assert_eq!(
            install_url(StoreSource::Epic, "a&b", false).as_deref(),
            Some("heroic://launch?appName=a%26b&runner=legendary")
        );
        assert_eq!(install_url(StoreSource::Manual, "x", false), None);
    }
}
