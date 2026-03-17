// POST /v1/games/search — compound filtering (Implementing Guide Step 5)
// Cross-dimension: AND (all active dimensions must be satisfied)
// Within dimension: OR (multiple values in one dimension)
// Exclusion: _not parameters remove matches

use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use uuid::Uuid;

use crate::games::GAME_COLUMNS;
use crate::models::*;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    // Player count filters
    pub players: Option<i32>,
    pub players_min: Option<i32>,
    pub players_max: Option<i32>,

    // Weight filters
    pub weight_min: Option<f64>,
    pub weight_max: Option<f64>,

    // Type and mode
    #[serde(rename = "type")]
    pub game_type: Option<Vec<String>>,
    pub mode: Option<String>,

    // Mechanics (OR within dimension)
    pub mechanics: Option<Vec<String>>,
    pub mechanics_all: Option<Vec<String>>,
    pub mechanics_not: Option<Vec<String>>,

    // Themes
    pub themes: Option<Vec<String>>,
    pub theme_not: Option<Vec<String>>,

    // Year
    pub year_min: Option<i32>,
    pub year_max: Option<i32>,

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
) -> Result<Json<PaginatedResponse<Game>>, StatusCode> {
    let limit = req.limit.unwrap_or(25).min(100);

    // Build dynamic WHERE clauses (all AND'd together = cross-dimension AND)
    let mut conditions: Vec<String> = Vec::new();
    let mut param_idx = 1u32;

    // We'll collect bind values as strings and bind them dynamically
    // For simplicity, build the full SQL string with parameter placeholders
    let mut bind_values: Vec<BindValue> = Vec::new();

    // Player count: exact match (supports this count)
    if let Some(players) = req.players {
        conditions.push(format!(
            "g.min_players <= ${} AND g.max_players >= ${}",
            param_idx,
            param_idx + 1
        ));
        bind_values.push(BindValue::Int(players));
        bind_values.push(BindValue::Int(players));
        param_idx += 2;
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

    // Weight range
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

    // Type filter (OR within dimension)
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

    // Mode
    if let Some(ref mode) = req.mode {
        conditions.push(format!("g.mode = ${}", param_idx));
        bind_values.push(BindValue::Str(mode.clone()));
        param_idx += 1;
    }

    // Year range
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

    // Themes: OR
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

    // Themes: NOT
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

    // Build WHERE clause
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Sorting
    let sort_col = match req.sort.as_deref() {
        Some("rating") | Some("rating_desc") => "g.average_rating",
        Some("bayes_rating") => "g.bayes_rating",
        Some("weight") => "g.weight",
        Some("year") => "g.year_published",
        Some("name") => "g.name",
        _ => "g.average_rating",
    };
    let sort_order = match req.order.as_deref() {
        Some("asc") => "ASC NULLS LAST",
        _ => "DESC NULLS LAST",
    };

    // Alias game columns with "g." prefix
    let columns = GAME_COLUMNS.replace("id,", "g.id,").replace(
        ", slug,",
        ", g.slug,",
    );
    // Simpler: just select from games with alias
    let game_cols = GAME_COLUMNS
        .split(", ")
        .map(|c| {
            let c = c.trim();
            if c.contains("::") {
                // e.g. "average_rating::FLOAT8" -> "g.average_rating::FLOAT8"
                format!("g.{}", c)
            } else {
                format!("g.{}", c)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");

    // Count query
    let count_sql = format!("SELECT COUNT(*) FROM games g {}", where_clause);

    // Data query
    let data_sql = format!(
        "SELECT {} FROM games g {} ORDER BY {} {} LIMIT ${}",
        game_cols,
        where_clause,
        sort_col,
        sort_order,
        param_idx
    );
    bind_values.push(BindValue::Bigint(limit));

    // Execute count
    let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql);
    for val in &bind_values[..bind_values.len() - 1] {
        count_query = match val {
            BindValue::Int(v) => count_query.bind(*v),
            BindValue::Float(v) => count_query.bind(*v),
            BindValue::Str(v) => count_query.bind(v.as_str()),
            BindValue::Bigint(v) => count_query.bind(*v),
        };
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
        data_query = match val {
            BindValue::Int(v) => data_query.bind(*v),
            BindValue::Float(v) => data_query.bind(*v),
            BindValue::Str(v) => data_query.bind(v.as_str()),
            BindValue::Bigint(v) => data_query.bind(*v),
        };
    }
    let games = data_query.fetch_all(&state.db).await.map_err(|e| {
        tracing::error!("Search data error: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(PaginatedResponse {
        data: games,
        meta: PaginationMeta {
            total,
            next_cursor: None,
            prev_cursor: None,
        },
        _links: PaginationLinks {
            self_link: Link {
                href: "/v1/games/search".to_string(),
                title: None,
            },
            next: None,
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
