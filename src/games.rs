// Game endpoints (Implementing Guide Step 4)
// GET /v1/games — list games with keyset pagination (ADR-0012)

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::*;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct ListGamesParams {
    pub cursor: Option<String>,
    pub before: Option<String>,
    pub limit: Option<i64>,
    // Filter params (same as getting-started.md GET examples)
    pub players: Option<i32>,
    pub players_min: Option<i32>,
    pub players_max: Option<i32>,
    pub weight_min: Option<f64>,
    pub weight_max: Option<f64>,
    #[serde(alias = "type")]
    pub game_type: Option<String>,
    pub mode: Option<String>,
    pub sort: Option<String>,
    pub order: Option<String>,
    pub effective: Option<bool>,
    pub community_playtime_max: Option<i32>,
}

pub async fn list_games(
    State(state): State<AppState>,
    Query(params): Query<ListGamesParams>,
) -> Result<Json<PaginatedResponse<GameWithMatch>>, StatusCode> {
    let limit = params.limit.unwrap_or(25).min(100);

    // Decode cursors (base64url-encoded UUID for keyset pagination)
    let decode_cursor = |c: &str| -> Option<Uuid> {
        let bytes = URL_SAFE_NO_PAD.decode(c).ok()?;
        let s = std::str::from_utf8(&bytes).ok()?;
        s.parse().ok()
    };
    let cursor_id: Option<Uuid> = params.cursor.as_deref().and_then(decode_cursor);
    let before_id: Option<Uuid> = params.before.as_deref().and_then(decode_cursor);
    let is_backward = before_id.is_some() && cursor_id.is_none();

    // Build WHERE clauses from filter params
    let effective = params.effective.unwrap_or(false);
    let mut conditions: Vec<String> = Vec::new();

    // Player count filter — when effective=true, also match via expansion combinations
    if let Some(players) = params.players {
        if effective {
            // Match if base supports it OR any expansion combination supports it
            // OR base + max property_modification delta supports it (three-tier)
            conditions.push(format!(
                "((min_players <= {p} AND max_players >= {p})
                  OR id IN (
                     SELECT base_game_id FROM expansion_combinations
                     WHERE effective_min_players <= {p} AND effective_max_players >= {p}
                  )
                  OR id IN (
                     SELECT base_game_id FROM property_modifications
                     GROUP BY base_game_id
                     HAVING (SELECT g2.max_players FROM games g2 WHERE g2.id = base_game_id)
                            + MAX(max_players_delta) >= {p}
                  ))",
                p = players
            ));
        } else {
            conditions.push(format!("min_players <= {} AND max_players >= {}", players, players));
        }
    }
    if let Some(min) = params.players_min {
        conditions.push(format!("max_players >= {}", min));
    }
    if let Some(max) = params.players_max {
        conditions.push(format!("min_players <= {}", max));
    }

    // Weight filter — when effective=true, also match via expansion-modified weight
    if let Some(min) = params.weight_min {
        conditions.push(format!("weight >= {}", min));
    }
    if let Some(max) = params.weight_max {
        conditions.push(format!("weight <= {}", max));
    }
    if let Some(ref t) = params.game_type {
        conditions.push(format!("type = '{}'", t.replace('\'', "''")));
    }
    if let Some(ref m) = params.mode {
        conditions.push(format!("mode = '{}'", m.replace('\'', "''")));
    }
    if let Some(max) = params.community_playtime_max {
        conditions.push(format!("community_median_playtime <= {}", max));
    }
    if let Some(after_id) = cursor_id {
        conditions.push(format!("id > '{}'", after_id));
    }
    if let Some(bef_id) = before_id {
        conditions.push(format!("id < '{}'", bef_id));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sort_col = match params.sort.as_deref() {
        Some("bayes_rating") => "bayes_rating",
        Some("weight") => "weight",
        Some("year") => "year_published",
        Some("name") => "name",
        _ => "id",
    };
    let default_order = if sort_col == "id" { "ASC" } else { "DESC NULLS LAST" };
    let sort_order = match params.order.as_deref() {
        Some("asc") => "ASC NULLS LAST",
        Some("desc") => "DESC NULLS LAST",
        _ => default_order,
    };

    // For backward pagination, reverse the sort direction to fetch the previous page,
    // then reverse results back to normal order after fetching.
    let query_order = if is_backward {
        if sort_order.starts_with("ASC") { "DESC NULLS LAST" } else { "ASC NULLS LAST" }
    } else {
        sort_order
    };

    // Count query (without cursor conditions for accurate total)
    let count_conditions: Vec<&str> = conditions
        .iter()
        .filter(|c| !c.starts_with("id >") && !c.starts_with("id <"))
        .map(|c| c.as_str())
        .collect();
    let count_where = if count_conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", count_conditions.join(" AND "))
    };
    let count_sql = format!("SELECT COUNT(*) FROM games {}", count_where);
    let total: i64 = sqlx::query_scalar(&count_sql)
        .fetch_one(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Data query
    let data_sql = format!(
        "SELECT {} FROM games {} ORDER BY {} {} LIMIT {}",
        GAME_COLUMNS, where_clause, sort_col, query_order, limit
    );
    let mut games: Vec<Game> = sqlx::query_as(&data_sql)
        .fetch_all(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Reverse results for backward pagination to restore natural order
    if is_backward {
        games.reverse();
    }

    // If effective mode, populate matched_via for games that matched through expansions
    let games_with_match: Vec<GameWithMatch> = if effective {
        crate::effective::populate_matched_via(&state.db, games, params.players)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        games
            .into_iter()
            .map(|game| GameWithMatch {
                game,
                matched_via: None,
            })
            .collect()
    };

    // Build cursors
    let has_full_page = games_with_match.len() as i64 == limit;
    let has_cursor = cursor_id.is_some() || before_id.is_some();

    let next_cursor = if (is_backward && has_cursor) || (!is_backward && has_full_page) {
        games_with_match
            .last()
            .map(|g| URL_SAFE_NO_PAD.encode(g.game.id.to_string()))
    } else {
        None
    };

    let prev_cursor = if (is_backward && has_full_page) || (!is_backward && has_cursor) {
        games_with_match
            .first()
            .map(|g| URL_SAFE_NO_PAD.encode(g.game.id.to_string()))
    } else {
        None
    };

    let next_link = next_cursor.as_ref().map(|c| Link {
        href: format!("/v1/games?cursor={}&limit={}", c, limit),
        title: None,
    });
    let prev_link = prev_cursor.as_ref().map(|c| Link {
        href: format!("/v1/games?before={}&limit={}", c, limit),
        title: None,
    });

    Ok(Json(PaginatedResponse {
        data: games_with_match,
        meta: PaginationMeta {
            total,
            next_cursor,
            prev_cursor,
        },
        _links: PaginationLinks {
            self_link: Link {
                href: "/v1/games".to_string(),
                title: None,
            },
            next: next_link,
            prev: prev_link,
        },
    }))
}

// Shared SQL columns for Game queries (cast NUMERIC to FLOAT8 for Rust f64)
pub const GAME_COLUMNS: &str =
    "id, slug, name, type, sort_name, parent_game_id, year_published,
     description, description_short, min_players, max_players,
     min_playtime, max_playtime,
     community_min_playtime, community_max_playtime,
     community_median_playtime, min_age, community_suggested_age,
     rating::FLOAT8, bayes_rating::FLOAT8, rating_votes,
     rating_stddev::FLOAT8, rating_confidence::FLOAT8, rating_distribution,
     weight::FLOAT8, weight_votes, rank_overall,
     owner_count, wishlist_count, total_plays, mode, funding_source,
     language_dependence, image_url, thumbnail_url, bgg_id, status,
     top_player_counts, recommended_player_counts, created_at, updated_at";

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
    pub effective_properties: Link,
    pub player_count_ratings: Link,
    pub relationships: Link,
    pub experience_playtime: Link,
}

pub type ApiError = (StatusCode, Json<ErrorResponse>);

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
            effective_properties: Link {
                href: format!("/v1/games/{}/effective-properties", slug),
                title: None,
            },
            player_count_ratings: Link {
                href: format!("/v1/games/{}/player-count-ratings", slug),
                title: None,
            },
            relationships: Link {
                href: format!("/v1/games/{}/relationships", slug),
                title: None,
            },
            experience_playtime: Link {
                href: format!("/v1/games/{}/experience-playtime", slug),
                title: None,
            },
        },
        game,
        expansions,
    }))
}

// GET /v1/games/{id_or_slug}/expansions (Implementing Guide Step 4)
// Lists expansions where parent_game_id matches the base game

pub async fn list_expansions(
    State(state): State<AppState>,
    Path(id_or_slug): Path<String>,
) -> Result<Json<PaginatedResponse<Game>>, ApiError> {
    // Resolve the base game first
    let base_game_id = resolve_game_id(&state.db, &id_or_slug).await?;

    let expansions = sqlx::query_as::<_, Game>(&format!(
        "SELECT {} FROM games WHERE parent_game_id = $1 ORDER BY year_published, name",
        GAME_COLUMNS
    ))
    .bind(base_game_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| internal_error())?;

    let total = expansions.len() as i64;

    Ok(Json(PaginatedResponse {
        data: expansions,
        meta: PaginationMeta {
            total,
            next_cursor: None,
            prev_cursor: None,
        },
        _links: PaginationLinks {
            self_link: Link {
                href: format!("/v1/games/{}/expansions", id_or_slug),
                title: None,
            },
            next: None,
            prev: None,
        },
    }))
}

// Helper: resolve UUID or slug to a game ID
pub async fn resolve_game_id(db: &PgPool, id_or_slug: &str) -> Result<Uuid, ApiError> {
    let id: Option<Uuid> = if let Ok(uuid) = id_or_slug.parse::<Uuid>() {
        sqlx::query_scalar("SELECT id FROM games WHERE id = $1")
            .bind(uuid)
            .fetch_optional(db)
            .await
            .map_err(|_| internal_error())?
    } else {
        sqlx::query_scalar("SELECT id FROM games WHERE slug = $1")
            .bind(id_or_slug)
            .fetch_optional(db)
            .await
            .map_err(|_| internal_error())?
    };

    id.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error_type: "about:blank".to_string(),
                title: "Not Found".to_string(),
                status: 404,
                detail: Some(format!("Game '{}' not found.", id_or_slug)),
            }),
        )
    })
}

pub fn internal_error() -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error_type: "about:blank".to_string(),
            title: "Internal Server Error".to_string(),
            status: 500,
            detail: None,
        }),
    )
}
