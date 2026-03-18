// GET /v1/games/{id}/snapshots -- Longitudinal trend data (ADR-0036)
// Returns historical snapshots of a game's materialized aggregates.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::games::{resolve_game_id, ApiError, internal_error};
use crate::models::Link;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct SnapshotParams {
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct GameSnapshot {
    pub snapshot_date: chrono::NaiveDate,
    pub rating: Option<f64>,
    pub rating_votes: Option<i32>,
    pub rating_confidence: Option<f64>,
    pub weight: Option<f64>,
    pub weight_votes: Option<i32>,
    pub rank_overall: Option<i32>,
    pub play_count_period: Option<i32>,
    pub owner_count: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct SnapshotsResponse {
    pub game_id: String,
    pub data: Vec<GameSnapshot>,
    pub _links: SnapshotLinks,
}

#[derive(Debug, Serialize)]
pub struct SnapshotLinks {
    #[serde(rename = "self")]
    pub self_link: Link,
    pub game: Link,
}

pub async fn get_snapshots(
    State(state): State<AppState>,
    Path(id_or_slug): Path<String>,
    Query(params): Query<SnapshotParams>,
) -> Result<Json<SnapshotsResponse>, ApiError> {
    let game_id = resolve_game_id(&state.db, &id_or_slug).await?;
    let limit = params.limit.unwrap_or(90).min(365);

    let snapshots = sqlx::query_as::<_, GameSnapshot>(
        "SELECT snapshot_date,
                rating::FLOAT8, rating_votes, rating_confidence::FLOAT8,
                weight::FLOAT8, weight_votes, rank_overall,
                play_count_period, owner_count
         FROM game_snapshots
         WHERE game_id = $1
         ORDER BY snapshot_date DESC
         LIMIT $2",
    )
    .bind(game_id)
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .map_err(|_| internal_error())?;

    let slug = &id_or_slug;
    Ok(Json(SnapshotsResponse {
        game_id: game_id.to_string(),
        data: snapshots,
        _links: SnapshotLinks {
            self_link: Link {
                href: format!("/v1/games/{}/snapshots", slug),
                title: None,
            },
            game: Link {
                href: format!("/v1/games/{}", slug),
                title: None,
            },
        },
    }))
}
