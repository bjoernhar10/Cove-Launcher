//! Store sources (Steam, GOG, Epic, manual) and their connection state.
//! Only display data lives here; tokens are in the OS keyring.

use sqlx::{FromRow, SqlitePool};

use crate::error::{AppError, AppResult};
use crate::models::{
    now_iso, ConnectorLoginMode, StoreAccount, StoreAccountStatus, StoreProblem, StoreSource,
};

#[derive(Debug, FromRow)]
struct SourceRow {
    id: String,
    connected: bool,
    needs_reauth: bool,
    account_name: Option<String>,
    last_synced_at: Option<String>,
    problem: Option<String>,
    game_count: i64,
}

/// How each store shows its login page.
pub fn login_mode(store: StoreSource) -> ConnectorLoginMode {
    match store {
        StoreSource::Steam | StoreSource::Manual => ConnectorLoginMode::Browser,
        StoreSource::Gog | StoreSource::Epic | StoreSource::Amazon => ConnectorLoginMode::Webview,
    }
}

impl SourceRow {
    fn into_account(self) -> Option<StoreAccount> {
        let store = StoreSource::parse(&self.id)?;
        let status = match (self.connected, self.needs_reauth) {
            (false, _) => StoreAccountStatus::Disconnected,
            (true, false) => StoreAccountStatus::Connected,
            (true, true) => StoreAccountStatus::NeedsReauth,
        };
        Some(StoreAccount {
            store,
            status,
            connected: self.connected,
            account_name: self.account_name,
            game_count: self.game_count,
            last_synced_at: self.last_synced_at,
            login_mode: login_mode(store),
            problem: self.problem.and_then(|p| serde_json::from_str(&p).ok()),
        })
    }
}

const SELECT: &str =
    "SELECT s.id, s.connected, s.needs_reauth, s.account_name, s.last_synced_at, s.problem, \
       (SELECT COUNT(*) FROM library_entries e WHERE e.source_id = s.id) AS game_count \
     FROM sources s";

/// Connectable stores (manual excluded) with the number of library games from each.
pub async fn list(pool: &SqlitePool) -> AppResult<Vec<StoreAccount>> {
    let rows: Vec<SourceRow> = sqlx::query_as(&format!(
        "{SELECT} WHERE s.id <> 'manual' \
         ORDER BY CASE s.id WHEN 'steam' THEN 0 WHEN 'gog' THEN 1 WHEN 'epic' THEN 2 ELSE 3 END"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(SourceRow::into_account)
        .collect())
}

pub async fn get(pool: &SqlitePool, store: StoreSource) -> AppResult<StoreAccount> {
    let row: SourceRow = sqlx::query_as(&format!("{SELECT} WHERE s.id = ?"))
        .bind(store.as_str())
        .fetch_one(pool)
        .await?;
    row.into_account()
        .ok_or_else(|| AppError::InvalidInput("Unbekannter Store.".into()))
}

pub async fn set_connected(
    pool: &SqlitePool,
    store: StoreSource,
    account_name: &str,
) -> AppResult<StoreAccount> {
    sqlx::query(
        "UPDATE sources SET connected = 1, needs_reauth = 0, problem = NULL, account_name = ? WHERE id = ?",
    )
    .bind(account_name)
    .bind(store.as_str())
    .execute(pool)
    .await?;
    get(pool, store).await
}

pub async fn set_disconnected(pool: &SqlitePool, store: StoreSource) -> AppResult<StoreAccount> {
    sqlx::query(
        "UPDATE sources SET connected = 0, needs_reauth = 0, problem = NULL, account_name = NULL WHERE id = ?",
    )
    .bind(store.as_str())
    .execute(pool)
    .await?;
    get(pool, store).await
}

pub async fn set_needs_reauth(pool: &SqlitePool, store: StoreSource) -> AppResult<()> {
    sqlx::query("UPDATE sources SET needs_reauth = 1 WHERE id = ? AND connected = 1")
        .bind(store.as_str())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_problem(
    pool: &SqlitePool,
    store: StoreSource,
    problem: Option<&StoreProblem>,
) -> AppResult<()> {
    sqlx::query("UPDATE sources SET problem = ? WHERE id = ?")
        .bind(problem.map(serde_json::to_string).transpose()?)
        .bind(store.as_str())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn mark_synced(pool: &SqlitePool, store: StoreSource) -> AppResult<()> {
    sqlx::query("UPDATE sources SET last_synced_at = ? WHERE id = ?")
        .bind(now_iso())
        .bind(store.as_str())
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::models::StoreProblemKind;

    #[tokio::test]
    async fn lists_the_stores_disconnected() {
        let pool = open_in_memory().await.unwrap();
        let accounts = list(&pool).await.unwrap();
        let stores: Vec<StoreSource> = accounts.iter().map(|a| a.store).collect();
        assert_eq!(
            stores,
            [
                StoreSource::Steam,
                StoreSource::Gog,
                StoreSource::Epic,
                StoreSource::Amazon
            ]
        );
        assert!(accounts
            .iter()
            .all(|a| a.status == StoreAccountStatus::Disconnected && a.game_count == 0));
        assert_eq!(accounts[0].login_mode, ConnectorLoginMode::Browser);
        assert_eq!(accounts[2].login_mode, ConnectorLoginMode::Webview);
    }

    #[tokio::test]
    async fn connection_lifecycle() {
        let pool = open_in_memory().await.unwrap();
        let account = set_connected(&pool, StoreSource::Gog, "kiefer")
            .await
            .unwrap();
        assert_eq!(account.status, StoreAccountStatus::Connected);
        assert_eq!(account.account_name.as_deref(), Some("kiefer"));

        set_needs_reauth(&pool, StoreSource::Gog).await.unwrap();
        let problem = StoreProblem {
            kind: StoreProblemKind::Unavailable,
            message: "GOG ist gerade nicht verfügbar.".into(),
            help_url: None,
        };
        set_problem(&pool, StoreSource::Gog, Some(&problem))
            .await
            .unwrap();
        mark_synced(&pool, StoreSource::Gog).await.unwrap();
        let account = get(&pool, StoreSource::Gog).await.unwrap();
        assert_eq!(account.status, StoreAccountStatus::NeedsReauth);
        assert_eq!(account.problem, Some(problem));
        assert!(account.last_synced_at.is_some());

        // A fresh login clears both.
        let account = set_connected(&pool, StoreSource::Gog, "kiefer")
            .await
            .unwrap();
        assert_eq!(
            (account.status, account.problem),
            (StoreAccountStatus::Connected, None)
        );

        let account = set_disconnected(&pool, StoreSource::Gog).await.unwrap();
        assert_eq!(account.status, StoreAccountStatus::Disconnected);
        assert_eq!(account.account_name, None);

        // Disconnected stores cannot "need reauth".
        set_needs_reauth(&pool, StoreSource::Gog).await.unwrap();
        assert_eq!(
            get(&pool, StoreSource::Gog).await.unwrap().status,
            StoreAccountStatus::Disconnected
        );
    }
}
