// Game endpoints (Implementing Guide Step 4)
// GET /v1/games — list games with keyset pagination (ADR-0012)

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
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

// Shared SQL columns for Game queries (cast NUMERIC to FLOAT8 for Rust f64)
const GAME_COLUMNS: &str =
    "id, slug, name, type, sort_name, parent_game_id, year_published,
     description, description_short, min_players, max_players,
     min_playtime_minutes, max_playtime_minutes,
     community_playtime_min_minutes, community_playtime_max_minutes,
     community_playtime_median_minutes, min_age, community_suggested_age,
     average_rating::FLOAT8, bayes_rating::FLOAT8, rating_count,
     rating_stddev::FLOAT8, rating_confidence::FLOAT8,
     weight::FLOAT8, weight_votes, rank_overall,
     owner_count, wishlist_count, total_plays, mode, funding_source,
     language_dependence, image_url, thumbnail_url, bgg_id, status";

// GET /v1/games/{id_or_slug} — single game by UUID or slug (Implementing Guide Step 4)
// Lookup by UUID or slug (ADR-0008: both are valid identifiers)
// 404 with RFC 9457 ErrorResponse if not found (ADR-0015)

#[derive(Debug, Deserialize)]
pub struct GetGameParams {
    pub include: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct GameWithIncludes {
    #[serde(flatten)]
    pub game: Game,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expansions: Option<Vec<Game>>,
    pub _links: GameLinks,
}

#[derive(Debug, serde::Serialize)]
pub struct GameLinks {
    #[serde(rename = "self")]
    pub self_link: Link,
    pub expansions: Link,
}

type ApiError = (StatusCode, Json<ErrorResponse>);

pub async fn get_game(
    State(state): State<AppState>,
    Path(id_or_slug): Path<String>,
    Query(params): Query<GetGameParams>,
) -> Result<Json<GameWithIncludes>, ApiError> {
    // Try UUID first, fall back to slug lookup
    let game = if let Ok(uuid) = id_or_slug.parse::<Uuid>() {
        sqlx::query_as::<_, Game>(&format!("SELECT {} FROM games WHERE id = $1", GAME_COLUMNS))
            .bind(uuid)
            .fetch_optional(&state.db)
            .await
    } else {
        sqlx::query_as::<_, Game>(&format!(
            "SELECT {} FROM games WHERE slug = $1",
            GAME_COLUMNS
        ))
        .bind(&id_or_slug)
        .fetch_optional(&state.db)
        .await
    }
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error_type: "about:blank".to_string(),
                title: "Internal Server Error".to_string(),
                status: 500,
                detail: None,
            }),
        )
    })?;

    let game = game.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error_type: "about:blank".to_string(),
                title: "Not Found".to_string(),
                status: 404,
                detail: Some(format!("Game '{}' not found.", id_or_slug)),
            }),
        )
    })?;

    // ?include=expansions support (ADR-0017)
    let expansions = if params
        .include
        .as_deref()
        .is_some_and(|i| i.contains("expansions"))
    {
        let exps = sqlx::query_as::<_, Game>(&format!(
            "SELECT {} FROM games WHERE parent_game_id = $1 ORDER BY year_published, name",
            GAME_COLUMNS
        ))
        .bind(game.id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
        Some(exps)
    } else {
        None
    };

    let slug = &game.slug;
    Ok(Json(GameWithIncludes {
        _links: GameLinks {
            self_link: Link {
                href: format!("/v1/games/{}", slug),
                title: None,
            },
            expansions: Link {
                href: format!("/v1/games/{}/expansions", slug),
                title: None,
            },
        },
        game,
        expansions,
    }))
}
