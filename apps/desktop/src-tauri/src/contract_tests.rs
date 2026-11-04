//! Contract test: everything the core sends to the UI must validate against
//! the shared JSON schema generated from `packages/shared/src/models.ts`.

use serde::Serialize;
use serde_json::{json, Value};

use crate::db;
use crate::db::library::tests::sample_game;
use crate::error::AppError;
use crate::models::{
    ExecutableKind, GameState, GameStateEvent, LaunchMethod, LaunchResult, ManualGameInput,
    RunningGame, ScanCandidate, StoreSource,
};

fn schema_definitions() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../packages/shared/schemas/models.schema.json"
    );
    let raw = std::fs::read_to_string(path).expect("schema file");
    let schema: Value = serde_json::from_str(&raw).unwrap();
    schema["definitions"].clone()
}

fn errors(definition: &str, value: &impl Serialize) -> Vec<String> {
    let wrapper = json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "definitions": schema_definitions(),
        "allOf": [{ "$ref": format!("#/definitions/{definition}") }],
    });
    let validator = jsonschema::validator_for(&wrapper).expect("valid schema");
    let instance = serde_json::to_value(value).unwrap();
    validator
        .iter_errors(&instance)
        .map(|e| format!("{e} at {}", e.instance_path))
        .collect()
}

fn assert_valid(definition: &str, value: &impl Serialize) {
    let errors = errors(definition, value);
    assert!(
        errors.is_empty(),
        "{definition} does not match the shared schema:\n{}",
        errors.join("\n")
    );
}

#[test]
fn schema_check_is_not_vacuous() {
    let none: Vec<String> = Vec::new();
    assert_ne!(
        errors("LibraryItem", &json!({ "entry": {}, "game": {} })),
        none
    );
    let bad_state =
        json!({ "gameId": "g", "state": "paused", "startedAt": "x", "sessionSeconds": null });
    assert_ne!(errors("GameStateEvent", &bad_state), none);
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one assertion per wire type
async fn core_responses_match_shared_schema() {
    let pool = db::open_in_memory().await.unwrap();
    let manual = db::library::add_manual(
        &pool,
        &ManualGameInput {
            title: "Tide".into(),
            executable_path: "/g/tide".into(),
            size_bytes: Some(10),
        },
    )
    .await
    .unwrap();
    let store = db::library::insert_store_game(
        &pool,
        &sample_game("orbit", "Orbit"),
        StoreSource::Steam,
        Some("620"),
        None,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO achievements (game_id, id, title, rarity_percent) VALUES ('orbit', 'a', 'A', 12.5)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO user_achievements VALUES ('orbit', 'a', '2026-11-01T10:00:00Z')")
        .execute(&pool)
        .await
        .unwrap();
    db::downloads::enqueue(&pool, "orbit").await.unwrap();

    assert_valid("LibraryItem", &manual);
    assert_valid("LibraryItem", &store);
    for item in db::library::list(&pool).await.unwrap() {
        assert_valid("LibraryItem", &item);
    }
    for a in db::achievements::list(&pool, "orbit").await.unwrap() {
        assert_valid("Achievement", &a);
    }
    for u in db::achievements::recent(&pool, 5).await.unwrap() {
        assert_valid("AchievementUnlock", &u);
    }
    let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();
    assert_valid(
        "PlaytimeStats",
        &db::sessions::stats(&pool, "orbit", today).await.unwrap(),
    );
    for d in db::downloads::list(&pool).await.unwrap() {
        assert_valid("DownloadItem", &d);
    }
    for s in db::sources::list(&pool).await.unwrap() {
        assert_valid("StoreAccount", &s);
    }
    let settings = db::settings::load(&pool, &crate::state::default_settings(None))
        .await
        .unwrap();
    assert_valid("AppSettings", &settings);
    assert_valid("UserProfile", &crate::commands::system_profile());
    assert_valid("AboutInfo", &crate::commands::system_about());
    assert_valid(
        "ScanCandidate",
        &ScanCandidate {
            title: "T".into(),
            executable_path: "/t".into(),
            size_bytes: 1,
            kind: ExecutableKind::DesktopEntry,
            already_in_library: false,
        },
    );
    assert_valid(
        "GameStateEvent",
        &GameStateEvent {
            game_id: "g".into(),
            state: GameState::Stopped,
            started_at: "2026-11-03T10:00:00Z".into(),
            session_seconds: Some(5),
        },
    );
    assert_valid(
        "RunningGame",
        &RunningGame {
            game_id: "g".into(),
            started_at: "2026-11-03T10:00:00Z".into(),
        },
    );
    assert_valid(
        "LaunchResult",
        &LaunchResult {
            game_id: "g".into(),
            method: LaunchMethod::Url,
            tracked: false,
        },
    );
    let game = sample_game("igdb-1", "Orbit");
    assert_valid(
        "CatalogPage",
        &crate::models::CatalogPage {
            reasons: [("igdb-1".to_owned(), "Ähnlich wie Tide".to_owned())].into(),
            items: vec![game.clone()],
            page: 0,
            has_more: true,
            total: None,
            offline: true,
        },
    );
    assert_valid(
        "CatalogGameDetails",
        &crate::models::CatalogGameDetails {
            game: game.clone(),
            library_game_id: Some("manual-orbit".into()),
            offline: false,
        },
    );
    assert_valid(
        "CatalogStatus",
        &crate::models::CatalogStatus {
            sources: vec![
                crate::models::CatalogSource::Cove,
                crate::models::CatalogSource::Igdb,
            ],
            cove_service: crate::models::CoveServiceStatus {
                available: true,
                enabled: true,
            },
            configured: crate::models::ProvidersConfigured {
                igdb: false,
                rawg: true,
            },
            online: true,
        },
    );
    assert_valid(
        "MatchCandidate",
        &crate::models::MatchCandidate { game, score: 0.8 },
    );
    assert_valid(
        "LibraryMatchEvent",
        &crate::models::LibraryMatchEvent {
            game_id: "g".into(),
            status: crate::models::MatchStatus::Pending,
        },
    );
    assert_valid(
        "CatalogCredentials",
        &crate::models::CatalogCredentials::Igdb {
            client_id: "a".into(),
            client_secret: "b".into(),
        },
    );
    assert_valid("AppErrorPayload", &AppError::CatalogNotConfigured);
    let mut with_features = sample_game("igdb-9", "Featured");
    with_features.features = Some(crate::models::GameFeatures {
        themes: vec!["Fantasy".into()],
        game_modes: vec!["Single player".into()],
        perspectives: vec!["Side view".into()],
        keywords: vec!["metroidvania".into()],
    });
    assert_valid("Game", &with_features);
    assert_valid(
        "Recommendation",
        &crate::models::Recommendation {
            game: with_features.clone(),
            score: 0.9,
            reason: "Ähnlich wie X".into(),
        },
    );
    let items = crate::db::library::list(&pool).await.unwrap();
    let (library_inputs, game_inputs) =
        crate::recommend::to_library_inputs(&items, &std::collections::HashMap::new());
    for g in &library_inputs {
        assert_valid("RecommenderLibraryGame", g);
    }
    for g in &game_inputs {
        assert_valid("RecommenderGame", g);
    }
    assert_valid(
        "RecommenderGame",
        &crate::recommend::to_recommender_game(&with_features, "igdb-9"),
    );
    assert_valid(
        "AppErrorPayload",
        &AppError::RecommenderUnavailable("startet nicht".into()),
    );
    assert_valid("AppErrorPayload", &AppError::NotRunning("g".into()));
    assert_valid(
        "AppErrorPayload",
        &AppError::CompatToolNotFound("Proton".into()),
    );
}
