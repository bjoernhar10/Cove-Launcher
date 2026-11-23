//! Library sync between the user's computers through their own cloud (the
//! save sync's connection, `savesync::cloud`): favourites, notes, collections,
//! the wishlist and the playtime of Cove's own sessions. Launch options,
//! folders and logins stay on each computer.
//!
//! The cloud holds one file, `cove-library/v1/library.json`. Its `entries` are
//! small keyed values (`fav|<game>`, `note|<game>`, `col|<name>`,
//! `colgame|<game>|<name>`, `wish|<catalogue id>`), merged three ways: against
//! the entries this computer last agreed on (`base`), a side that changed wins,
//! and when both changed this computer wins. So a favourite removed on one
//! computer is removed on the others too. Sessions are only ever added (one per
//! game and start). Games are known by store and store id, manual games by
//! their normalized title; values of games this computer doesn't have are kept
//! as they are. The first sync with a cloud, or a cloud file that is gone,
//! merges both sides without removing anything.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::db;
use crate::error::AppResult;
use crate::models::{now_iso, AppSettings, Game, LibraryItem, LibrarySyncStatus, StoreSource};
use crate::savesync::SaveSync;

const FILE: &str = "cove-library/v1/library.json";
const FORMAT: &str = "cove-library";
const VERSION: u32 = 1;
/// Settings table keys: the entries last agreed on, and when.
pub const BASE_KEY: &str = "librarySyncBase";
pub const AT_KEY: &str = "librarySyncAt";
/// Changes wait this long, so a burst of clicks is one sync.
const SETTLE: Duration = Duration::from_secs(10);
/// Without changes here, the cloud is asked this often.
const PERIOD: Duration = Duration::from_secs(30 * 60);

type Entries = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Doc {
    format: String,
    version: u32,
    device: String,
    saved_at: String,
    #[serde(default)]
    entries: Entries,
    /// `<game>|<start>` → session.
    #[serde(default)]
    sessions: BTreeMap<String, SyncedSession>,
    /// Wishlist games' catalogue data, so another computer can add them.
    #[serde(default)]
    games: BTreeMap<String, Game>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncedSession {
    game: String,
    started_at: String,
    ended_at: Option<String>,
    duration_seconds: i64,
}

/// How a game is known on every computer: store and store id, else the title.
pub fn game_key(source: StoreSource, store_game_id: Option<&str>, title: &str) -> String {
    match store_game_id.filter(|id| source != StoreSource::Manual && !id.is_empty()) {
        Some(id) => format!("{}:{}", source.as_str(), id.replace('|', "_")),
        None => format!("title:{}", crate::catalog::matching::normalize(title)),
    }
}

/// The game a key names (`fav|g`, `note|g`, `colgame|g|name`); None = no game.
fn key_game(key: &str) -> Option<&str> {
    let mut parts = key.splitn(3, '|');
    match (parts.next(), parts.next()) {
        (Some("fav" | "note" | "colgame"), Some(game)) => Some(game),
        _ => None,
    }
}

/// Three-way merge: a side that changed since `base` wins, both changed = `local`.
fn merge(base: &Entries, local: &Entries, remote: &Entries) -> Entries {
    let keys: HashSet<&String> = base
        .keys()
        .chain(local.keys())
        .chain(remote.keys())
        .collect();
    let mut merged = Entries::new();
    for key in keys {
        let (b, l, r) = (base.get(key), local.get(key), remote.get(key));
        let value = if l == b { r } else { l };
        if let Some(value) = value {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged
}

/// This computer's side.
struct Local {
    entries: Entries,
    sessions: BTreeMap<String, SyncedSession>,
    games: BTreeMap<String, Game>,
    /// Game key → local game id (library games only).
    ids: HashMap<String, String>,
    /// Lower-case collection name → (id, name).
    collections: HashMap<String, (String, String)>,
}

async fn local(pool: &SqlitePool, base: &Entries) -> AppResult<Local> {
    let library = db::library::list(pool).await?;
    let mut ids: HashMap<String, String> = HashMap::new();
    let mut keys: HashMap<String, String> = HashMap::new();
    let mut entries = Entries::new();
    for item in &library {
        let key = key_of(item);
        if ids.contains_key(&key) {
            continue;
        }
        ids.insert(key.clone(), item.game.id.clone());
        keys.insert(item.game.id.clone(), key.clone());
        if item.entry.favorite {
            entries.insert(format!("fav|{key}"), Value::Bool(true));
        }
    }
    for (game_id, note) in db::notes::all(pool).await? {
        if let Some(key) = keys.get(&game_id) {
            entries.insert(
                format!("note|{key}"),
                json!({ "text": note.text, "updatedAt": note.updated_at }),
            );
        }
    }
    let mut collections = HashMap::new();
    for collection in db::collections::list(pool).await? {
        let lower = collection.name.to_lowercase();
        entries.insert(
            format!("col|{lower}"),
            Value::String(collection.name.clone()),
        );
        for game_id in &collection.game_ids {
            if let Some(key) = keys.get(game_id) {
                entries.insert(format!("colgame|{key}|{lower}"), Value::Bool(true));
            }
        }
        collections.insert(lower, (collection.id, collection.name));
    }
    let mut games = BTreeMap::new();
    for wish in db::wishlist::list(pool).await? {
        entries.insert(
            format!("wish|{}", wish.game.id),
            json!({ "targetPrice": wish.target_price, "notifyOnSale": wish.notify_on_sale }),
        );
        games.insert(wish.game.id.clone(), wish.game);
    }
    // What concerns games this computer doesn't have is as the base says.
    for (key, value) in base {
        if key_game(key).is_some_and(|game| !ids.contains_key(game)) {
            entries.insert(key.clone(), value.clone());
        }
    }
    let rows: Vec<(String, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT s.game_id, s.started_at, s.ended_at, s.duration_seconds FROM play_sessions s \
         JOIN library_entries e ON e.game_id = s.game_id WHERE s.duration_seconds > 0",
    )
    .fetch_all(pool)
    .await?;
    let mut sessions = BTreeMap::new();
    for (game_id, started_at, ended_at, duration_seconds) in rows {
        if let Some(key) = keys.get(&game_id) {
            sessions.insert(
                format!("{key}|{started_at}"),
                SyncedSession {
                    game: key.clone(),
                    started_at,
                    ended_at,
                    duration_seconds,
                },
            );
        }
    }
    Ok(Local {
        entries,
        sessions,
        games,
        ids,
        collections,
    })
}

fn key_of(item: &LibraryItem) -> String {
    game_key(
        item.entry.source,
        item.entry.store_game_id.as_deref(),
        &item.game.title,
    )
}

/// Makes this computer match the merged entries and sessions.
#[allow(clippy::too_many_lines)] // one pass per kind of entry
async fn apply(pool: &SqlitePool, local: &Local, merged: &Doc) -> AppResult<bool> {
    let mut changed = false;
    let wanted = &merged.entries;
    let log = |what: &str, error: &crate::error::AppError| {
        eprintln!("[library-sync] {what}: {error}");
    };
    for (key, game_id) in &local.ids {
        let fav = format!("fav|{key}");
        let want = wanted.contains_key(&fav);
        if want != local.entries.contains_key(&fav) {
            sqlx::query("UPDATE library_entries SET favorite = ? WHERE game_id = ?")
                .bind(want)
                .bind(game_id)
                .execute(pool)
                .await?;
            changed = true;
        }
        let note = format!("note|{key}");
        if wanted.get(&note) != local.entries.get(&note) {
            let text = wanted.get(&note).and_then(|n| n["text"].as_str());
            let at = wanted
                .get(&note)
                .and_then(|n| n["updatedAt"].as_str())
                .map_or_else(now_iso, str::to_owned);
            match text.filter(|t| !t.trim().is_empty() && t.chars().count() <= db::notes::MAX_CHARS)
            {
                Some(text) => db::notes::write(pool, game_id, text, &at).await?,
                None => {
                    db::notes::set(pool, game_id, "").await?;
                }
            }
            changed = true;
        }
    }

    // Collections: the merged names, then their games.
    let mut collections = local.collections.clone();
    for (lower, (id, _)) in &local.collections {
        if !wanted.contains_key(&format!("col|{lower}")) {
            match db::collections::delete(pool, id).await {
                Ok(()) => changed = true,
                Err(error) => log("collection", &error),
            }
            collections.remove(lower);
        }
    }
    for (key, value) in wanted.range("col|".to_owned()..) {
        let Some(lower) = key.strip_prefix("col|") else {
            break;
        };
        let Some(name) = value.as_str() else { continue };
        match collections.get(lower) {
            Some((id, current)) if current != name => {
                match db::collections::rename(pool, id, name).await {
                    Ok(_) => changed = true,
                    Err(error) => log("collection", &error),
                }
            }
            Some(_) => {}
            None => match db::collections::create(pool, name).await {
                Ok(created) => {
                    changed = true;
                    collections.insert(lower.to_owned(), (created.id, created.name));
                }
                Err(error) => log("collection", &error),
            },
        }
    }
    for (lower, (id, _)) in &collections {
        for (key, game_id) in &local.ids {
            let member = format!("colgame|{key}|{lower}");
            let want = wanted.contains_key(&member);
            if want != local.entries.contains_key(&member) {
                match db::collections::set_game(pool, id, game_id, want).await {
                    Ok(()) => changed = true,
                    Err(error) => log("collection", &error),
                }
            }
        }
    }

    // Wishlist.
    for key in local.entries.keys().filter(|k| k.starts_with("wish|")) {
        if !wanted.contains_key(key) {
            db::wishlist::remove(pool, &key["wish|".len()..]).await?;
            changed = true;
        }
    }
    for (key, value) in wanted.range("wish|".to_owned()..) {
        let Some(id) = key.strip_prefix("wish|") else {
            break;
        };
        if local.entries.get(key) == Some(value) {
            continue;
        }
        if !local.entries.contains_key(key) {
            let Some(game) = merged.games.get(id) else {
                continue;
            };
            if let Err(error) = db::wishlist::add(pool, game).await {
                log("wishlist", &error);
                continue;
            }
        }
        let target = value["targetPrice"].as_f64();
        let sale = value["notifyOnSale"].as_bool().unwrap_or(true);
        match db::wishlist::update(pool, id, target, sale).await {
            Ok(_) => changed = true,
            Err(error) => log("wishlist", &error),
        }
    }

    // Sessions from the other computers.
    for (key, session) in &merged.sessions {
        let Some(game_id) = local.ids.get(&session.game) else {
            continue;
        };
        if local.sessions.contains_key(key) || session.duration_seconds <= 0 {
            continue;
        }
        let exists: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM play_sessions WHERE game_id = ? AND started_at = ?")
                .bind(game_id)
                .bind(&session.started_at)
                .fetch_optional(pool)
                .await?;
        if exists.is_none() {
            sqlx::query(
                "INSERT INTO play_sessions (game_id, started_at, ended_at, duration_seconds) VALUES (?, ?, ?, ?)",
            )
            .bind(game_id)
            .bind(&session.started_at)
            .bind(session.ended_at.as_deref().unwrap_or(&session.started_at))
            .bind(session.duration_seconds)
            .execute(pool)
            .await?;
            changed = true;
        }
    }
    Ok(changed)
}

/// One sync's work, without the cloud: the merged document and whether this
/// computer changed. `remote` None = no file in the cloud yet.
async fn sync_with(pool: &SqlitePool, device: &str, remote: Option<Doc>) -> AppResult<(Doc, bool)> {
    // No cloud file: nothing was agreed on with it.
    let base: Entries = if remote.is_some() {
        db::settings::get_json(pool, BASE_KEY)
            .await?
            .unwrap_or_default()
    } else {
        Entries::new()
    };
    let local = local(pool, &base).await?;
    let remote = remote.unwrap_or_else(|| empty(device));
    let mut entries = merge(&base, &local.entries, &remote.entries);
    // Members of a collection that is gone go with it.
    let names: HashSet<String> = entries
        .keys()
        .filter_map(|k| k.strip_prefix("col|").map(str::to_owned))
        .collect();
    entries.retain(|key, _| {
        !key.starts_with("colgame|") || key.splitn(3, '|').nth(2).is_some_and(|n| names.contains(n))
    });
    let mut sessions = remote.sessions.clone();
    sessions.extend(local.sessions.clone());
    let mut games = remote.games.clone();
    games.extend(local.games.clone());
    // Only wishlist games stay in the file.
    games.retain(|id, _| entries.contains_key(&format!("wish|{id}")));
    let merged = Doc {
        format: FORMAT.into(),
        version: VERSION,
        device: device.to_owned(),
        saved_at: now_iso(),
        entries,
        sessions,
        games,
    };
    let changed = apply(pool, &local, &merged).await?;
    db::settings::set_json(pool, BASE_KEY, &merged.entries).await?;
    Ok((merged, changed))
}

fn empty(device: &str) -> Doc {
    Doc {
        format: FORMAT.into(),
        version: VERSION,
        device: device.to_owned(),
        saved_at: now_iso(),
        entries: Entries::new(),
        sessions: BTreeMap::new(),
        games: BTreeMap::new(),
    }
}

/// Same content (who saved it and when don't count).
fn same(a: &Doc, b: &Doc) -> bool {
    a.entries == b.entries && a.sessions == b.sessions && a.games == b.games
}

pub type Changed = Arc<dyn Fn(bool) + Send + Sync>;

pub struct LibrarySync {
    pool: SqlitePool,
    defaults: AppSettings,
    saves: Arc<SaveSync>,
    busy: tokio::sync::Mutex<()>,
    wake: tokio::sync::Notify,
    problem: std::sync::Mutex<Option<String>>,
    /// After a sync; true = this computer's library changed.
    on_synced: Changed,
}

impl LibrarySync {
    pub fn new(
        pool: SqlitePool,
        defaults: AppSettings,
        saves: Arc<SaveSync>,
        on_synced: Changed,
    ) -> Self {
        Self {
            pool,
            defaults,
            saves,
            busy: tokio::sync::Mutex::new(()),
            wake: tokio::sync::Notify::new(),
            problem: std::sync::Mutex::new(None),
            on_synced,
        }
    }

    /// Something synced changed here: a sync follows shortly.
    pub fn changed(&self) {
        self.wake.notify_one();
    }

    fn set_problem(&self, problem: Option<String>) {
        if let Ok(mut slot) = self.problem.lock() {
            *slot = problem;
        }
    }

    pub async fn status(&self) -> AppResult<LibrarySyncStatus> {
        let settings = db::settings::load(&self.pool, &self.defaults).await?;
        Ok(LibrarySyncStatus {
            enabled: settings.backups.library_sync,
            connected: self.saves.status().await?.provider.is_some(),
            last_synced_at: db::settings::get_json(&self.pool, AT_KEY).await?,
            problem: self.problem.lock().ok().and_then(|p| p.clone()),
        })
    }

    /// Syncs now when it is on and a cloud is connected.
    pub async fn sync(&self) -> AppResult<LibrarySyncStatus> {
        let settings = db::settings::load(&self.pool, &self.defaults).await?;
        if settings.backups.library_sync {
            let result = self.run().await;
            self.set_problem(
                result
                    .as_ref()
                    .err()
                    .map(crate::error::AppError::user_message),
            );
            if result.is_err() {
                // The sidebar shows the problem.
                (self.on_synced)(false);
            }
            result?;
        }
        self.status().await
    }

    async fn run(&self) -> AppResult<()> {
        let _busy = self.busy.lock().await;
        let Some(remote) = self.saves.remote().await? else {
            return Ok(());
        };
        let found: Option<Doc> = match remote.get(FILE).await? {
            Some(bytes) => match serde_json::from_slice::<Doc>(&bytes) {
                Ok(doc) if doc.format == FORMAT && doc.version <= VERSION => Some(doc),
                // A newer Cove wrote it: leave it alone.
                Ok(_) => return Ok(()),
                Err(error) => {
                    eprintln!("[library-sync] unreadable cloud file: {error}");
                    None
                }
            },
            None => None,
        };
        let device = self.saves.status().await?.device;
        let (merged, changed) = sync_with(&self.pool, &device, found.clone()).await?;
        if found.as_ref().is_none_or(|doc| !same(doc, &merged)) {
            remote
                .put_bytes(FILE, serde_json::to_vec(&merged)?, found.is_some())
                .await?;
        }
        db::settings::set_json(&self.pool, AT_KEY, &now_iso()).await?;
        (self.on_synced)(changed);
        Ok(())
    }

    /// Runs in the background: shortly after the start, after changes, and
    /// every half hour.
    pub async fn watch(self: Arc<Self>, first: Duration) {
        tokio::time::sleep(first).await;
        loop {
            if let Err(error) = self.sync().await {
                eprintln!("[library-sync] {error}");
            }
            tokio::select! {
                () = self.wake.notified() => tokio::time::sleep(SETTLE).await,
                () = tokio::time::sleep(PERIOD) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::library::tests::sample_game;

    fn entries(pairs: &[(&str, Value)]) -> Entries {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    #[test]
    fn merge_takes_the_side_that_changed() {
        let t = Value::Bool(true);
        let base = entries(&[
            ("fav|a", t.clone()),
            ("fav|b", t.clone()),
            ("col|x", json!("X")),
        ]);
        // Here: b unfavoured, c favoured. There: a unfavoured, collection renamed.
        let local = entries(&[
            ("fav|a", t.clone()),
            ("fav|c", t.clone()),
            ("col|x", json!("X")),
        ]);
        let remote = entries(&[("fav|b", t.clone()), ("col|x", json!("x!"))]);
        assert_eq!(
            merge(&base, &local, &remote),
            entries(&[("fav|c", t.clone()), ("col|x", json!("x!"))])
        );
        // Both changed: this computer wins.
        let local = entries(&[("col|x", json!("Here"))]);
        assert_eq!(
            merge(&base, &local, &remote).get("col|x"),
            Some(&json!("Here"))
        );
        // First sync: both sides together.
        let merged = merge(
            &Entries::new(),
            &entries(&[("fav|a", t.clone())]),
            &entries(&[("fav|b", t.clone())]),
        );
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn game_keys_name_the_store_or_the_title() {
        assert_eq!(
            game_key(StoreSource::Steam, Some("620"), "Portal 2"),
            "steam:620"
        );
        assert_eq!(
            game_key(StoreSource::Manual, Some("x"), "The Witcher® 3"),
            "title:the witcher 3"
        );
        assert_eq!(
            key_game("colgame|steam:620|mit freunden"),
            Some("steam:620")
        );
        assert_eq!(key_game("col|mit freunden"), None);
    }

    async fn computer() -> SqlitePool {
        let pool = db::open_in_memory().await.unwrap();
        for (id, title, store) in [
            ("steam-620", "Portal 2", "620"),
            ("steam-10", "Lantern", "10"),
        ] {
            db::library::insert_store_game(
                &pool,
                &sample_game(id, title),
                StoreSource::Steam,
                Some(store),
                None,
            )
            .await
            .unwrap();
        }
        pool
    }

    async fn favourite(pool: &SqlitePool, id: &str, on: bool) {
        sqlx::query("UPDATE library_entries SET favorite = ? WHERE game_id = ?")
            .bind(on)
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }

    async fn is_favourite(pool: &SqlitePool, id: &str) -> bool {
        db::library::require(pool, id).await.unwrap().entry.favorite
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)] // two computers through several rounds
    async fn two_computers_share_favourites_notes_collections_wishlist_and_playtime() {
        let pc = computer().await;
        let deck = computer().await;
        // The Deck doesn't have Lantern; the pc has a game only it knows.
        sqlx::query("DELETE FROM library_entries WHERE game_id = 'steam-10'")
            .execute(&deck)
            .await
            .unwrap();

        // Desk: favourite, note, collection, wishlist, a session.
        favourite(&pc, "steam-620", true).await;
        favourite(&pc, "steam-10", true).await;
        db::notes::set(&pc, "steam-620", "Chapter 6").await.unwrap();
        let friends = db::collections::create(&pc, "Mit Freunden").await.unwrap();
        db::collections::set_game(&pc, &friends.id, "steam-620", true)
            .await
            .unwrap();
        db::wishlist::add(&pc, &sample_game("igdb-77", "Hollow Peak"))
            .await
            .unwrap();
        db::wishlist::update(&pc, "igdb-77", Some(9.99), false)
            .await
            .unwrap();
        sqlx::query("INSERT INTO play_sessions (game_id, started_at, ended_at, duration_seconds) VALUES ('steam-620', '2026-11-01T10:00:00Z', '2026-11-01T11:00:00Z', 3600)")
            .execute(&pc)
            .await
            .unwrap();
        // Deck: its own collection, already before the first sync.
        db::collections::create(&deck, "Couch").await.unwrap();

        let (cloud, changed) = sync_with(&pc, "Desk", None).await.unwrap();
        assert!(!changed);
        let (cloud, changed) = sync_with(&deck, "Deck", Some(cloud)).await.unwrap();
        assert!(changed);
        assert!(is_favourite(&deck, "steam-620").await);
        assert_eq!(
            db::notes::get(&deck, "steam-620")
                .await
                .unwrap()
                .unwrap()
                .text,
            "Chapter 6"
        );
        let names: Vec<String> = db::collections::list(&deck)
            .await
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, ["Couch", "Mit Freunden"]);
        let wish = db::wishlist::get(&deck, "igdb-77").await.unwrap().unwrap();
        assert_eq!(
            (wish.target_price, wish.notify_on_sale),
            (Some(9.99), false)
        );
        let portal = db::library::require(&deck, "steam-620").await.unwrap();
        assert_eq!(portal.entry.playtime_minutes, 60);
        // Lantern's favourite stays in the cloud for the computers that have it.
        assert!(cloud.entries.contains_key("fav|steam:10"));

        // Deck: unfavour Portal, change the note, leave the collection, drop the wish.
        favourite(&deck, "steam-620", false).await;
        db::notes::set(&deck, "steam-620", "Chapter 7")
            .await
            .unwrap();
        let deck_friends = db::collections::list(&deck)
            .await
            .unwrap()
            .into_iter()
            .find(|c| c.name == "Mit Freunden")
            .unwrap();
        db::collections::set_game(&deck, &deck_friends.id, "steam-620", false)
            .await
            .unwrap();
        db::wishlist::remove(&deck, "igdb-77").await.unwrap();
        let (cloud, _) = sync_with(&deck, "Deck", Some(cloud)).await.unwrap();
        assert!(cloud.entries.contains_key("fav|steam:10"));

        // Desk: gets all of it, Lantern stays a favourite, Couch arrives.
        let (cloud, changed) = sync_with(&pc, "Desk", Some(cloud)).await.unwrap();
        assert!(changed);
        assert!(!is_favourite(&pc, "steam-620").await);
        assert!(is_favourite(&pc, "steam-10").await);
        assert_eq!(
            db::notes::get(&pc, "steam-620")
                .await
                .unwrap()
                .unwrap()
                .text,
            "Chapter 7"
        );
        let collections = db::collections::list(&pc).await.unwrap();
        assert_eq!(collections.len(), 2);
        assert!(collections.iter().all(|c| c.game_ids.is_empty()));
        assert!(db::wishlist::get(&pc, "igdb-77").await.unwrap().is_none());
        let portal = db::library::require(&pc, "steam-620").await.unwrap();
        assert_eq!(portal.entry.playtime_minutes, 60);

        // Nothing new: the next sync changes nothing anywhere.
        let (again, changed) = sync_with(&pc, "Desk", Some(cloud.clone())).await.unwrap();
        assert!(!changed && same(&again, &cloud));

        // The cloud file is gone: nothing here is removed, it all goes up again.
        let (fresh, changed) = sync_with(&pc, "Desk", None).await.unwrap();
        assert!(!changed);
        assert_eq!(fresh.entries, cloud.entries);
    }
}
