// Game endpoints (Implementing Guide Step 4)
// GET /v1/games — list games with keyset pagination (ADR-0012)

use axum::{extract::{Query, State}, http::StatusCode, Json};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Deserialize;
use uuid::Uuid;

use crate::models::*;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct ListGamesParams {
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}

pub async fn list_games(
    State(state): State<AppState>,
    Query(params): Query<ListGamesParams>,
) -> Result<Json<PaginatedResponse<Game>>, StatusCode> {
    let limit = params.limit.unwrap_or(25).min(100);

    // Decode cursor (base64url-encoded UUID for keyset pagination)
    let cursor_id: Option<Uuid> = params.cursor.as_ref().and_then(|c| {
        let bytes = URL_SAFE_NO_PAD.decode(c).ok()?;
        let s = std::str::from_utf8(&bytes).ok()?;
        s.parse().ok()
    });

    // Count total games
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM games")
        .fetch_one(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Keyset pagination: fetch games after cursor, ordered by id
    let games = if let Some(after_id) = cursor_id {
        sqlx::query_as::<_, Game>(
            "SELECT id, slug, name, type, sort_name, parent_game_id, year_published,
                    description, description_short, min_players, max_players,
                    min_playtime_minutes, max_playtime_minutes,
                    community_playtime_min_minutes, community_playtime_max_minutes,
                    community_playtime_median_minutes, min_age, community_suggested_age,
                    average_rating::FLOAT8, bayes_rating::FLOAT8, rating_count,
                    rating_stddev::FLOAT8, rating_confidence::FLOAT8,
                    weight::FLOAT8, weight_votes, rank_overall,
                    owner_count, wishlist_count, total_plays, mode, funding_source,
                    language_dependence, image_url, thumbnail_url, bgg_id, status
             FROM games WHERE id > $1 ORDER BY id LIMIT $2",
        )
        .bind(after_id)
        .bind(limit)
        .fetch_all(&state.db)
        .await
    } else {
        sqlx::query_as::<_, Game>(
            "SELECT id, slug, name, type, sort_name, parent_game_id, year_published,
                    description, description_short, min_players, max_players,
                    min_playtime_minutes, max_playtime_minutes,
                    community_playtime_min_minutes, community_playtime_max_minutes,
                    community_playtime_median_minutes, min_age, community_suggested_age,
                    average_rating::FLOAT8, bayes_rating::FLOAT8, rating_count,
                    rating_stddev::FLOAT8, rating_confidence::FLOAT8,
                    weight::FLOAT8, weight_votes, rank_overall,
                    owner_count, wishlist_count, total_plays, mode, funding_source,
                    language_dependence, image_url, thumbnail_url, bgg_id, status
             FROM games ORDER BY id LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&state.db)
        .await
    }
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Build next cursor from last game's id
    let next_cursor = if games.len() as i64 == limit {
        games
            .last()
            .map(|g| URL_SAFE_NO_PAD.encode(g.id.to_string()))
    } else {
        None
    };

    let next_link = next_cursor.as_ref().map(|c| Link {
        href: format!("/v1/games?cursor={}&limit={}", c, limit),
        title: None,
    });

    Ok(Json(PaginatedResponse {
        data: games,
        meta: PaginationMeta {
            total,
            next_cursor,
            prev_cursor: None,
        },
        _links: PaginationLinks {
            self_link: Link {
                href: "/v1/games".to_string(),
                title: None,
            },
            next: next_link,
            prev: None,
        },
    }))
}
