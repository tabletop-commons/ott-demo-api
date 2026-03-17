// Player count ratings endpoint (Implementing Guide Step 5)
// GET /v1/games/{id}/player-count-ratings
// Returns per-count community ratings (ADR-0010, ADR-0043 numeric 1-5 model)

use axum::{
    extract::{Path, State},
    Json,
};
use serde::Serialize;
use sqlx::FromRow;

use crate::games::{resolve_game_id, ApiError};
use crate::models::Link;
use crate::AppState;

#[derive(Debug, Serialize, FromRow)]
pub struct PlayerCountRating {
    pub player_count: i32,
    pub average_rating: f64,
    pub rating_count: i32,
    pub rating_stddev: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct PlayerCountRatingsResponse {
    pub game_id: String,
    pub ratings: Vec<PlayerCountRating>,
    pub _links: PlayerCountLinks,
}

#[derive(Debug, Serialize)]
pub struct PlayerCountLinks {
    #[serde(rename = "self")]
    pub self_link: Link,
    pub game: Link,
}

pub async fn get_player_count_ratings(
    State(state): State<AppState>,
    Path(id_or_slug): Path<String>,
) -> Result<Json<PlayerCountRatingsResponse>, ApiError> {
    let game_id = resolve_game_id(&state.db, &id_or_slug).await?;

    let ratings = sqlx::query_as::<_, PlayerCountRating>(
        "SELECT player_count, average_rating::FLOAT8 as average_rating,
                rating_count, rating_stddev::FLOAT8 as rating_stddev
         FROM player_count_ratings
         WHERE game_id = $1
         ORDER BY player_count",
    )
    .bind(game_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| crate::games::internal_error())?;

    Ok(Json(PlayerCountRatingsResponse {
        game_id: game_id.to_string(),
        ratings,
        _links: PlayerCountLinks {
            self_link: Link {
                href: format!("/v1/games/{}/player-count-ratings", id_or_slug),
                title: None,
            },
            game: Link {
                href: format!("/v1/games/{}", id_or_slug),
                title: None,
            },
        },
    }))
}
