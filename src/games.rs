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

    // Decode cursor (base64url-encoded UUID for keyset pagination)
    let cursor_id: Option<Uuid> = params.cursor.as_ref().and_then(|c| {
        let bytes = URL_SAFE_NO_PAD.decode(c).ok()?;
        let s = std::str::from_utf8(&bytes).ok()?;
        s.parse().ok()
    });

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
        conditions.push(format!("community_playtime_median_minutes <= {}", max));
    }
    if let Some(after_id) = cursor_id {
        conditions.push(format!("id > '{}'", after_id));
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
    let sort_order = match params.order.as_deref() {
        Some("asc") => "ASC NULLS LAST",
        Some("desc") => "DESC NULLS LAST",
        _ => if sort_col == "id" { "ASC" } else { "DESC NULLS LAST" },
    };

    // Count query
    let count_sql = format!("SELECT COUNT(*) FROM games {}", where_clause);
    let total: i64 = sqlx::query_scalar(&count_sql)
        .fetch_one(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Data query
    let data_sql = format!(
        "SELECT {} FROM games {} ORDER BY {} {} LIMIT {}",
        GAME_COLUMNS, where_clause, sort_col, sort_order, limit
    );
    let games: Vec<Game> = sqlx::query_as(&data_sql)
        .fetch_all(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // If effective mode, populate matched_via for games that matched through expansions
    let games_with_match: Vec<GameWithMatch> = if effective && params.players.is_some() {
        let players = params.players.unwrap();
        let mut results = Vec::new();
        for game in games {
            let base_matches = game.min_players.unwrap_or(0) <= players
                && game.max_players.unwrap_or(0) >= players;

            let matched_via = if base_matches {
                Some(MatchedVia {
                    match_type: "base".to_string(),
                    expansions: None,
                    effective_properties: None,
                    resolution_tier: 3,
                })
            } else {
                // Check which expansion combination matched
                let combo: Option<(Option<i32>, Option<i32>, Option<f64>, Option<i32>, Option<i32>)> =
                    sqlx::query_as(
                        "SELECT effective_min_players, effective_max_players,
                                effective_weight::FLOAT8, effective_playtime_min, effective_playtime_max
                         FROM expansion_combinations
                         WHERE base_game_id = $1
                           AND effective_min_players <= $2 AND effective_max_players >= $2
                         LIMIT 1",
                    )
                    .bind(game.id)
                    .bind(players)
                    .fetch_optional(&state.db)
                    .await
                    .ok()
                    .flatten();

                if let Some((min_p, max_p, w, min_t, max_t)) = combo {
                    // Tier 1: explicit combination
                    // Look up expansion names
                    let exp_names: Vec<MatchedExpansion> = sqlx::query_as::<_, (String, String)>(
                        "SELECT g.slug, g.name FROM games g
                         JOIN expansion_combinations ec ON g.id = ANY(ec.expansion_ids)
                         WHERE ec.base_game_id = $1
                         LIMIT 10",
                    )
                    .bind(game.id)
                    .fetch_all(&state.db)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(slug, name)| MatchedExpansion { slug, name })
                    .collect();

                    Some(MatchedVia {
                        match_type: "expansion_combination".to_string(),
                        expansions: Some(exp_names),
                        effective_properties: Some(EffectiveMatchProperties {
                            min_players: min_p,
                            max_players: max_p,
                            weight: w,
                            min_playtime: min_t,
                            max_playtime: max_t,
                        }),
                        resolution_tier: 1,
                    })
                } else {
                    // Tier 2: delta sum
                    Some(MatchedVia {
                        match_type: "delta_sum".to_string(),
                        expansions: None,
                        effective_properties: None,
                        resolution_tier: 2,
                    })
                }
            };
            results.push(GameWithMatch { game, matched_via });
        }
        results
    } else {
        games
            .into_iter()
            .map(|game| GameWithMatch {
                game,
                matched_via: None,
            })
            .collect()
    };

    // Build next cursor from last game's id
    let next_cursor = if games_with_match.len() as i64 == limit {
        games_with_match
            .last()
            .map(|g| URL_SAFE_NO_PAD.encode(g.game.id.to_string()))
    } else {
        None
    };

    let next_link = next_cursor.as_ref().map(|c| Link {
        href: format!("/v1/games?cursor={}&limit={}", c, limit),
        title: None,
    });

    Ok(Json(PaginatedResponse {
        data: games_with_match,
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
pub const GAME_COLUMNS: &str =
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
