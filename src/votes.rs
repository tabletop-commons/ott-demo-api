// POST /v1/games/{id}/ratings — Submit a raw rating vote
// Raw votes are stored in rating_votes table and do NOT immediately
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

    // Insert raw vote — does NOT update game aggregates
    sqlx::query("INSERT INTO rating_votes (game_id, rating) VALUES ($1, $2)")
        .bind(game_id)
        .bind(body.rating)
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
