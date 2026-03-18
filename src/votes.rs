// Vote submission endpoints
// POST /v1/games/{id}/ratings -- Submit a raw rating vote (1-10)
// POST /v1/games/{id}/weight  -- Submit a raw weight vote (1.0-5.0)
// Raw votes are stored in Tier 1 tables and do NOT immediately
// update the materialized aggregates on the Game entity.
// See: materialization.md

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::games::{resolve_game_id, internal_error, ApiError};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct RatingVoteRequest {
    pub rating: i32,
    // Layer 1 vote context (rating-model.md input contract)
    pub declared_scale: Option<String>,   // e.g., "1-5", "1-10", "5-10"
    pub play_count: Option<i32>,
    pub experience_level: Option<String>, // "first_play", "learning", "experienced", "expert"
}

#[derive(Debug, Serialize)]
pub struct VoteResponse {
    pub status: &'static str,
    pub message: String,
}

pub async fn submit_rating(
    State(state): State<AppState>,
    Path(id_or_slug): Path<String>,
    Json(body): Json<RatingVoteRequest>,
) -> Result<(StatusCode, Json<VoteResponse>), ApiError> {
    // Validate rating range (1-10)
    if body.rating < 1 || body.rating > 10 {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(VoteResponse {
                status: "error",
                message: "Rating must be between 1 and 10.".to_string(),
            }),
        ));
    }

    let game_id = resolve_game_id(&state.db, &id_or_slug).await?;

    // Insert raw vote with context -- does NOT update game aggregates
    sqlx::query(
        "INSERT INTO rating_votes (game_id, rating, declared_scale, play_count, experience_level)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(game_id)
    .bind(body.rating)
    .bind(&body.declared_scale)
    .bind(body.play_count)
    .bind(&body.experience_level)
    .execute(&state.db)
    .await
    .map_err(|_| internal_error())?;

    Ok((
        StatusCode::CREATED,
        Json(VoteResponse {
            status: "accepted",
            message: format!(
                "Rating of {} recorded for game '{}'. Aggregates will update on next materialization.",
                body.rating, id_or_slug
            ),
        }),
    ))
}

// POST /v1/games/{id}/weight -- Submit a raw weight vote (weight-model.md)
#[derive(Debug, Deserialize)]
pub struct WeightVoteRequest {
    pub weight: f64, // 1.0-5.0 per weight-model.md
}

pub async fn submit_weight(
    State(state): State<AppState>,
    Path(id_or_slug): Path<String>,
    Json(body): Json<WeightVoteRequest>,
) -> Result<(StatusCode, Json<VoteResponse>), ApiError> {
    if body.weight < 1.0 || body.weight > 5.0 {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(VoteResponse {
                status: "error",
                message: "Weight must be between 1.0 and 5.0.".to_string(),
            }),
        ));
    }

    let game_id = resolve_game_id(&state.db, &id_or_slug).await?;

    sqlx::query("INSERT INTO weight_votes_raw (game_id, weight) VALUES ($1, $2)")
        .bind(game_id)
        .bind(body.weight)
        .execute(&state.db)
        .await
        .map_err(|_| internal_error())?;

    Ok((
        StatusCode::CREATED,
        Json(VoteResponse {
            status: "accepted",
            message: format!(
                "Weight of {:.1} recorded for game '{}'. Aggregates will update on next materialization.",
                body.weight, id_or_slug
            ),
        }),
    ))
}
