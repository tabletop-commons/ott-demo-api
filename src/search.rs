// POST /v1/games/search -- compound filtering (Implementing Guide Step 9)
// Cross-dimension: AND (all active dimensions must be satisfied)
// Within dimension: OR (multiple values in one dimension)
// Exclusion: _not parameters remove matches

use axum::{extract::State, http::StatusCode, Json};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Deserialize;
use uuid::Uuid;

use crate::games::GAME_COLUMNS;
use crate::models::*;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    // Dimension 1: Rating & confidence
    pub rating_min: Option<f64>,
    pub rating_max: Option<f64>,
    pub min_rating_votes: Option<i32>,
    pub confidence_min: Option<f64>,

    // Dimension 2: Weight
    pub weight_min: Option<f64>,
    pub weight_max: Option<f64>,

    // Dimension 3: Player count
    pub players: Option<i32>,
    pub players_min: Option<i32>,
    pub players_max: Option<i32>,
    pub top_at: Option<i32>,
    pub recommended_at: Option<i32>,

    // Dimension 4: Play time
    pub playtime_min: Option<i32>,
    pub playtime_max: Option<i32>,
    pub playtime_source: Option<String>,     // "publisher" | "community"
    pub playtime_experience: Option<String>,  // "first_play" | "learning" | "experienced" | "expert"

    // Dimension 5: Age
    pub age_min: Option<i32>,
    pub age_max: Option<i32>,
    pub age_source: Option<String>, // "publisher" | "community"

    // Dimension 6: Game type & mechanics
    #[serde(rename = "type")]
    pub game_type: Option<Vec<String>>,
    pub mode: Option<String>,
    pub mechanics: Option<Vec<String>>,
    pub mechanics_all: Option<Vec<String>>,
    pub mechanics_not: Option<Vec<String>>,

    // Dimension 7: Theme
    pub themes: Option<Vec<String>>,
    pub theme_not: Option<Vec<String>>,

    // Dimension 8: Metadata
    pub designer: Option<String>,
    pub publisher: Option<String>,
    pub category: Option<String>,
    pub year_min: Option<i32>,
    pub year_max: Option<i32>,

    // Effective mode
    pub effective: Option<bool>,

    // Sorting
    pub sort: Option<String>,
    pub order: Option<String>,

    // Pagination
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

pub async fn search_games(
    State(state): State<AppState>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<PaginatedResponse<GameWithMatch>>, StatusCode> {
    let limit = req.limit.unwrap_or(25).min(100);
    let effective = req.effective.unwrap_or(false);

    // Decode cursor (base64url-encoded UUID for keyset pagination)
    let cursor_id: Option<Uuid> = req.cursor.as_ref().and_then(|c| {
        let bytes = URL_SAFE_NO_PAD.decode(c).ok()?;
        let s = std::str::from_utf8(&bytes).ok()?;
        s.parse().ok()
    });

    // Build dynamic WHERE clauses (all AND'd together = cross-dimension AND)
    let mut conditions: Vec<String> = Vec::new();
    let mut param_idx = 1u32;
    let mut bind_values: Vec<BindValue> = Vec::new();

    // --- Dimension 1: Rating & Confidence ---

    if let Some(min) = req.rating_min {
        conditions.push(format!("g.rating >= ${}", param_idx));
        bind_values.push(BindValue::Float(min));
        param_idx += 1;
    }
    if let Some(max) = req.rating_max {
        conditions.push(format!("g.rating <= ${}", param_idx));
        bind_values.push(BindValue::Float(max));
        param_idx += 1;
    }
    if let Some(min_votes) = req.min_rating_votes {
        conditions.push(format!("g.rating_votes >= ${}", param_idx));
        bind_values.push(BindValue::Int(min_votes));
        param_idx += 1;
    }
    if let Some(min_conf) = req.confidence_min {
        conditions.push(format!("g.rating_confidence >= ${}", param_idx));
        bind_values.push(BindValue::Float(min_conf));
        param_idx += 1;
    }

    // --- Dimension 2: Weight ---

    if let Some(min) = req.weight_min {
        conditions.push(format!("g.weight >= ${}", param_idx));
        bind_values.push(BindValue::Float(min));
        param_idx += 1;
    }
    if let Some(max) = req.weight_max {
        conditions.push(format!("g.weight <= ${}", param_idx));
        bind_values.push(BindValue::Float(max));
        param_idx += 1;
    }

    // --- Dimension 3: Player count ---

    if let Some(players) = req.players {
        if effective {
            // Three-tier: base OR expansion_combinations OR property_modifications
            conditions.push(format!(
                "((g.min_players <= ${p1} AND g.max_players >= ${p2})
                  OR g.id IN (
                     SELECT base_game_id FROM expansion_combinations
                     WHERE effective_min_players <= ${p3} AND effective_max_players >= ${p4}
                  )
                  OR g.id IN (
                     SELECT base_game_id FROM property_modifications
                     GROUP BY base_game_id
                     HAVING (SELECT g2.max_players FROM games g2 WHERE g2.id = base_game_id)
                            + MAX(max_players_delta) >= ${p5}
                  ))",
                p1 = param_idx, p2 = param_idx + 1,
                p3 = param_idx + 2, p4 = param_idx + 3,
                p5 = param_idx + 4
            ));
            for _ in 0..5 {
                bind_values.push(BindValue::Int(players));
            }
            param_idx += 5;
        } else {
            conditions.push(format!(
                "g.min_players <= ${} AND g.max_players >= ${}",
                param_idx,
                param_idx + 1
            ));
            bind_values.push(BindValue::Int(players));
            bind_values.push(BindValue::Int(players));
            param_idx += 2;
        }
    }
    if let Some(min) = req.players_min {
        conditions.push(format!("g.max_players >= ${}", param_idx));
        bind_values.push(BindValue::Int(min));
        param_idx += 1;
    }
    if let Some(max) = req.players_max {
        conditions.push(format!("g.min_players <= ${}", param_idx));
        bind_values.push(BindValue::Int(max));
        param_idx += 1;
    }
    if let Some(count) = req.top_at {
        conditions.push(format!("g.top_player_counts @> ARRAY[${}]::INTEGER[]", param_idx));
        bind_values.push(BindValue::Int(count));
        param_idx += 1;
    }
    if let Some(count) = req.recommended_at {
        conditions.push(format!("g.recommended_player_counts @> ARRAY[${}]::INTEGER[]", param_idx));
        bind_values.push(BindValue::Int(count));
        param_idx += 1;
    }

    // --- Dimension 4: Play time ---

    if req.playtime_experience.is_some() && (req.playtime_min.is_some() || req.playtime_max.is_some()) {
        // Filter on experience_playtime table for the specified level
        let level = req.playtime_experience.as_deref().unwrap();
        let mut sub_conditions = vec![format!("ep.experience_level = ${}", param_idx)];
        bind_values.push(BindValue::Str(level.to_string()));
        param_idx += 1;

        if let Some(min) = req.playtime_min {
            sub_conditions.push(format!("ep.median_minutes >= ${}", param_idx));
            bind_values.push(BindValue::Int(min));
            param_idx += 1;
        }
        if let Some(max) = req.playtime_max {
            sub_conditions.push(format!("ep.median_minutes <= ${}", param_idx));
            bind_values.push(BindValue::Int(max));
            param_idx += 1;
        }
        conditions.push(format!(
            "EXISTS (SELECT 1 FROM experience_playtime ep WHERE ep.game_id = g.id AND {})",
            sub_conditions.join(" AND ")
        ));
    } else {
        // Filter on game-level playtime columns, choosing source
        let (min_col, max_col) = match req.playtime_source.as_deref() {
            Some("community") => ("g.community_min_playtime", "g.community_max_playtime"),
            _ => ("g.min_playtime", "g.max_playtime"),  // publisher is default per playtime.md
        };

        if let Some(min) = req.playtime_min {
            // Game's max playtime must be at least the requested minimum
            conditions.push(format!("{} >= ${}", max_col, param_idx));
            bind_values.push(BindValue::Int(min));
            param_idx += 1;
        }
        if let Some(max) = req.playtime_max {
            // Game's min playtime must be at most the requested maximum
            conditions.push(format!("{} <= ${}", min_col, param_idx));
            bind_values.push(BindValue::Int(max));
            param_idx += 1;
        }
    }

    // --- Dimension 5: Age ---

    let age_col = match req.age_source.as_deref() {
        Some("community") => "g.community_suggested_age",
        _ => "g.min_age",
    };
    if let Some(min) = req.age_min {
        conditions.push(format!("{} >= ${}", age_col, param_idx));
        bind_values.push(BindValue::Int(min));
        param_idx += 1;
    }
    if let Some(max) = req.age_max {
        conditions.push(format!("{} <= ${}", age_col, param_idx));
        bind_values.push(BindValue::Int(max));
        param_idx += 1;
    }

    // --- Dimension 6: Game type & mechanics ---

    if let Some(ref types) = req.game_type {
        let placeholders: Vec<String> = types
            .iter()
            .map(|_| {
                let p = format!("${}", param_idx);
                param_idx += 1;
                p
            })
            .collect();
        conditions.push(format!("g.type IN ({})", placeholders.join(", ")));
        for t in types {
            bind_values.push(BindValue::Str(t.clone()));
        }
    }

    if let Some(ref mode) = req.mode {
        conditions.push(format!("g.mode = ${}", param_idx));
        bind_values.push(BindValue::Str(mode.clone()));
        param_idx += 1;
    }

    // Mechanics: OR (any of these)
    if let Some(ref mechs) = req.mechanics {
        let placeholders: Vec<String> = mechs
            .iter()
            .map(|_| {
                let p = format!("${}", param_idx);
                param_idx += 1;
                p
            })
            .collect();
        conditions.push(format!(
            "EXISTS (SELECT 1 FROM game_mechanics gm JOIN mechanics m ON m.id = gm.mechanic_id WHERE gm.game_id = g.id AND m.slug IN ({}))",
            placeholders.join(", ")
        ));
        for m in mechs {
            bind_values.push(BindValue::Str(m.clone()));
        }
    }

    // Mechanics: AND (all of these)
    if let Some(ref mechs) = req.mechanics_all {
        for mech in mechs {
            conditions.push(format!(
                "EXISTS (SELECT 1 FROM game_mechanics gm JOIN mechanics m ON m.id = gm.mechanic_id WHERE gm.game_id = g.id AND m.slug = ${})",
                param_idx
            ));
            bind_values.push(BindValue::Str(mech.clone()));
            param_idx += 1;
        }
    }

    // Mechanics: NOT (none of these)
    if let Some(ref mechs) = req.mechanics_not {
        let placeholders: Vec<String> = mechs
            .iter()
            .map(|_| {
                let p = format!("${}", param_idx);
                param_idx += 1;
                p
            })
            .collect();
        conditions.push(format!(
            "NOT EXISTS (SELECT 1 FROM game_mechanics gm JOIN mechanics m ON m.id = gm.mechanic_id WHERE gm.game_id = g.id AND m.slug IN ({}))",
            placeholders.join(", ")
        ));
        for m in mechs {
            bind_values.push(BindValue::Str(m.clone()));
        }
    }

    // --- Dimension 7: Theme ---

    if let Some(ref themes) = req.themes {
        let placeholders: Vec<String> = themes
            .iter()
            .map(|_| {
                let p = format!("${}", param_idx);
                param_idx += 1;
                p
            })
            .collect();
        conditions.push(format!(
            "EXISTS (SELECT 1 FROM game_themes gt JOIN themes t ON t.id = gt.theme_id WHERE gt.game_id = g.id AND t.slug IN ({}))",
            placeholders.join(", ")
        ));
        for t in themes {
            bind_values.push(BindValue::Str(t.clone()));
        }
    }

    if let Some(ref themes) = req.theme_not {
        let placeholders: Vec<String> = themes
            .iter()
            .map(|_| {
                let p = format!("${}", param_idx);
                param_idx += 1;
                p
            })
            .collect();
        conditions.push(format!(
            "NOT EXISTS (SELECT 1 FROM game_themes gt JOIN themes t ON t.id = gt.theme_id WHERE gt.game_id = g.id AND t.slug IN ({}))",
            placeholders.join(", ")
        ));
        for t in themes {
            bind_values.push(BindValue::Str(t.clone()));
        }
    }

    // --- Dimension 8: Metadata ---

    if let Some(ref designer) = req.designer {
        conditions.push(format!(
            "EXISTS (SELECT 1 FROM game_credits gc JOIN people p ON p.id = gc.person_id WHERE gc.game_id = g.id AND gc.role = 'designer' AND p.slug = ${})",
            param_idx
        ));
        bind_values.push(BindValue::Str(designer.clone()));
        param_idx += 1;
    }

    if let Some(ref publisher) = req.publisher {
        conditions.push(format!(
            "EXISTS (SELECT 1 FROM game_editions ge JOIN edition_publishers ep ON ep.edition_id = ge.id JOIN publishers pub ON pub.id = ep.publisher_id WHERE ge.game_id = g.id AND pub.slug = ${})",
            param_idx
        ));
        bind_values.push(BindValue::Str(publisher.clone()));
        param_idx += 1;
    }

    if let Some(ref category) = req.category {
        conditions.push(format!(
            "EXISTS (SELECT 1 FROM game_categories gc JOIN categories c ON c.id = gc.category_id WHERE gc.game_id = g.id AND c.slug = ${})",
            param_idx
        ));
        bind_values.push(BindValue::Str(category.clone()));
        param_idx += 1;
    }

    if let Some(min) = req.year_min {
        conditions.push(format!("g.year_published >= ${}", param_idx));
        bind_values.push(BindValue::Int(min));
        param_idx += 1;
    }
    if let Some(max) = req.year_max {
        conditions.push(format!("g.year_published <= ${}", param_idx));
        bind_values.push(BindValue::Int(max));
        param_idx += 1;
    }

    // --- Cursor pagination ---

    // Save bind count before cursor for count query (count excludes cursor condition)
    let bind_count_before_cursor = bind_values.len();

    if let Some(after_id) = cursor_id {
        conditions.push(format!("g.id > ${}", param_idx));
        bind_values.push(BindValue::Str(after_id.to_string()));
        param_idx += 1;
    }

    // Build WHERE clause
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Count query (without cursor condition for accurate total)
    let count_where = if bind_count_before_cursor == 0
        || conditions.len() <= 1 && cursor_id.is_some()
    {
        String::new()
    } else {
        let count_conds: Vec<&str> = conditions
            .iter()
            .filter(|c| !c.contains("g.id >"))
            .map(|s| s.as_str())
            .collect();
        if count_conds.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", count_conds.join(" AND "))
        }
    };

    // Sorting
    let sort_col = match req.sort.as_deref() {
        Some("rating") | Some("rating_desc") => "g.rating",
        Some("bayes_rating") => "g.bayes_rating",
        Some("weight") => "g.weight",
        Some("year") => "g.year_published",
        Some("name") => "g.name",
        _ => "g.rating",
    };
    let sort_order = match req.order.as_deref() {
        Some("asc") => "ASC NULLS LAST",
        _ => "DESC NULLS LAST",
    };

    // Alias game columns with "g." prefix
    let game_cols = GAME_COLUMNS
        .split(", ")
        .map(|c| format!("g.{}", c.trim()))
        .collect::<Vec<_>>()
        .join(", ");

    let count_sql = format!("SELECT COUNT(*) FROM games g {}", count_where);

    // Data query
    let data_sql = format!(
        "SELECT {} FROM games g {} ORDER BY {} {}, g.id ASC LIMIT ${}",
        game_cols,
        where_clause,
        sort_col,
        sort_order,
        param_idx
    );
    bind_values.push(BindValue::Bigint(limit));

    // Execute count (bind only pre-cursor values)
    let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql);
    for val in &bind_values[..bind_count_before_cursor] {
        count_query = bind_value(count_query, val);
    }
    let total = count_query
        .fetch_one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!("Search count error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Execute data
    let mut data_query = sqlx::query_as::<_, Game>(&data_sql);
    for val in &bind_values {
        data_query = bind_value_query_as(data_query, val);
    }
    let games = data_query.fetch_all(&state.db).await.map_err(|e| {
        tracing::error!("Search data error: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Build cursor from last result
    let next_cursor = if games.len() as i64 == limit {
        games
            .last()
            .map(|g| URL_SAFE_NO_PAD.encode(g.id.to_string()))
    } else {
        None
    };

    let prev_cursor = if cursor_id.is_some() {
        games
            .first()
            .map(|g| URL_SAFE_NO_PAD.encode(g.id.to_string()))
    } else {
        None
    };

    // Populate matched_via if effective mode
    let games_with_match: Vec<GameWithMatch> = if effective {
        crate::effective::populate_matched_via(&state.db, games, req.players)
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

    let next_link = next_cursor.as_ref().map(|c| Link {
        href: format!("/v1/games/search?cursor={}&limit={}", c, limit),
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
                href: "/v1/games/search".to_string(),
                title: None,
            },
            next: next_link,
            prev: None,
        },
    }))
}

// Dynamic bind value enum (SQLx doesn't support heterogeneous binds easily)
#[derive(Debug)]
enum BindValue {
    Int(i32),
    Float(f64),
    Str(String),
    Bigint(i64),
}

fn bind_value<'q>(
    query: sqlx::query::QueryScalar<'q, sqlx::Postgres, i64, sqlx::postgres::PgArguments>,
    val: &'q BindValue,
) -> sqlx::query::QueryScalar<'q, sqlx::Postgres, i64, sqlx::postgres::PgArguments> {
    match val {
        BindValue::Int(v) => query.bind(*v),
        BindValue::Float(v) => query.bind(*v),
        BindValue::Str(v) => query.bind(v.as_str()),
        BindValue::Bigint(v) => query.bind(*v),
    }
}

fn bind_value_query_as<'q>(
    query: sqlx::query::QueryAs<'q, sqlx::Postgres, Game, sqlx::postgres::PgArguments>,
    val: &'q BindValue,
) -> sqlx::query::QueryAs<'q, sqlx::Postgres, Game, sqlx::postgres::PgArguments> {
    match val {
        BindValue::Int(v) => query.bind(*v),
        BindValue::Float(v) => query.bind(*v),
        BindValue::Str(v) => query.bind(v.as_str()),
        BindValue::Bigint(v) => query.bind(*v),
    }
}
