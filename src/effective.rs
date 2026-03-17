// GET /v1/games/{id}/effective-properties?with=slug1,slug2
// Three-tier expansion resolution (ADR-0007, property-deltas.md)
// Tier 1: Explicit ExpansionCombination → use if found
// Tier 2: Sum individual PropertyModification deltas → computed
// Tier 3: Base game properties → base_only

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::games::{resolve_game_id, internal_error, ApiError};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct EffectiveParams {
    pub with: Option<String>, // comma-separated expansion slugs
}

#[derive(Debug, Serialize)]
pub struct EffectivePropertiesResponse {
    pub base: BaseProperties,
    pub applied_expansions: Vec<String>,
    pub effective: EffectivePropertyValues,
    pub combination_source: String, // "explicit", "computed", "base_only"
}

#[derive(Debug, Serialize, Clone)]
pub struct BaseProperties {
    pub min_players: Option<i32>,
    pub max_players: Option<i32>,
    pub min_playtime_minutes: Option<i32>,
    pub max_playtime_minutes: Option<i32>,
    pub weight: Option<f64>,
    pub min_age: Option<i32>,
}

#[derive(Debug, Serialize, Clone)]
pub struct EffectivePropertyValues {
    pub min_players: Option<i32>,
    pub max_players: Option<i32>,
    pub min_playtime_minutes: Option<i32>,
    pub max_playtime_minutes: Option<i32>,
    pub weight: Option<f64>,
    pub min_age: Option<i32>,
}

#[derive(Debug, FromRow)]
struct BaseGameRow {
    min_players: Option<i32>,
    max_players: Option<i32>,
    min_playtime_minutes: Option<i32>,
    max_playtime_minutes: Option<i32>,
    weight: Option<f64>,
    min_age: Option<i32>,
}

#[derive(Debug, FromRow)]
struct PropertyModRow {
    max_players_delta: Option<i32>,
    weight_delta: Option<f64>,
    playtime_min_delta: Option<i32>,
    playtime_max_delta: Option<i32>,
    min_age_delta: Option<i32>,
}

#[derive(Debug, FromRow)]
struct ExpansionComboRow {
    effective_min_players: Option<i32>,
    effective_max_players: Option<i32>,
    effective_weight: Option<f64>,
    effective_playtime_min: Option<i32>,
    effective_playtime_max: Option<i32>,
    effective_min_age: Option<i32>,
}

pub async fn get_effective_properties(
    State(state): State<AppState>,
    Path(id_or_slug): Path<String>,
    Query(params): Query<EffectiveParams>,
) -> Result<Json<EffectivePropertiesResponse>, ApiError> {
    let game_id = resolve_game_id(&state.db, &id_or_slug).await?;

    // Get base game properties
    let base = sqlx::query_as::<_, BaseGameRow>(
        "SELECT min_players, max_players, min_playtime_minutes, max_playtime_minutes,
                weight::FLOAT8 as weight, min_age
         FROM games WHERE id = $1",
    )
    .bind(game_id)
    .fetch_one(&state.db)
    .await
    .map_err(|_| internal_error())?;

    let base_props = BaseProperties {
        min_players: base.min_players,
        max_players: base.max_players,
        min_playtime_minutes: base.min_playtime_minutes,
        max_playtime_minutes: base.max_playtime_minutes,
        weight: base.weight,
        min_age: base.min_age,
    };

    // Parse expansion slugs
    let expansion_slugs: Vec<String> = params
        .with
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // If no expansions requested, return base_only (Tier 3)
    if expansion_slugs.is_empty() {
        return Ok(Json(EffectivePropertiesResponse {
            effective: EffectivePropertyValues {
                min_players: base_props.min_players,
                max_players: base_props.max_players,
                min_playtime_minutes: base_props.min_playtime_minutes,
                max_playtime_minutes: base_props.max_playtime_minutes,
                weight: base_props.weight,
                min_age: base_props.min_age,
            },
            base: base_props,
            applied_expansions: vec![],
            combination_source: "base_only".to_string(),
        }));
    }

    // Resolve expansion slugs to UUIDs
    let mut expansion_ids: Vec<Uuid> = Vec::new();
    for slug in &expansion_slugs {
        let id: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM games WHERE slug = $1 AND parent_game_id = $2")
                .bind(slug)
                .bind(game_id)
                .fetch_optional(&state.db)
                .await
                .map_err(|_| internal_error())?;
        if let Some(id) = id {
            expansion_ids.push(id);
        }
    }

    // Tier 1: Check for explicit ExpansionCombination
    // Sort expansion_ids for consistent lookup
    let mut sorted_ids = expansion_ids.clone();
    sorted_ids.sort();

    let combo = sqlx::query_as::<_, ExpansionComboRow>(
        "SELECT effective_min_players, effective_max_players, effective_weight::FLOAT8 as effective_weight,
                effective_playtime_min, effective_playtime_max, effective_min_age
         FROM expansion_combinations
         WHERE base_game_id = $1 AND expansion_ids @> $2 AND expansion_ids <@ $2",
    )
    .bind(game_id)
    .bind(&sorted_ids)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| internal_error())?;

    if let Some(combo) = combo {
        // Tier 1: Explicit combination found
        return Ok(Json(EffectivePropertiesResponse {
            effective: EffectivePropertyValues {
                min_players: combo.effective_min_players.or(base_props.min_players),
                max_players: combo.effective_max_players.or(base_props.max_players),
                min_playtime_minutes: combo.effective_playtime_min.or(base_props.min_playtime_minutes),
                max_playtime_minutes: combo.effective_playtime_max.or(base_props.max_playtime_minutes),
                weight: combo.effective_weight.or(base_props.weight),
                min_age: combo.effective_min_age.or(base_props.min_age),
            },
            base: base_props,
            applied_expansions: expansion_slugs,
            combination_source: "explicit".to_string(),
        }));
    }

    // Tier 2: Sum individual PropertyModification deltas
    let mut total_max_players_delta = 0i32;
    let mut total_weight_delta = 0.0f64;
    let mut total_playtime_min_delta = 0i32;
    let mut total_playtime_max_delta = 0i32;
    let mut total_min_age_delta = 0i32;
    let mut found_any = false;

    for exp_id in &expansion_ids {
        let delta = sqlx::query_as::<_, PropertyModRow>(
            "SELECT max_players_delta, weight_delta::FLOAT8 as weight_delta,
                    playtime_min_delta, playtime_max_delta, min_age_delta
             FROM property_modifications
             WHERE expansion_id = $1 AND base_game_id = $2",
        )
        .bind(exp_id)
        .bind(game_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| internal_error())?;

        if let Some(d) = delta {
            found_any = true;
            total_max_players_delta += d.max_players_delta.unwrap_or(0);
            total_weight_delta += d.weight_delta.unwrap_or(0.0);
            total_playtime_min_delta += d.playtime_min_delta.unwrap_or(0);
            total_playtime_max_delta += d.playtime_max_delta.unwrap_or(0);
            total_min_age_delta += d.min_age_delta.unwrap_or(0);
        }
    }

    if found_any {
        // Tier 2: Computed from deltas
        return Ok(Json(EffectivePropertiesResponse {
            effective: EffectivePropertyValues {
                min_players: base_props.min_players,
                max_players: base_props.max_players.map(|v| v + total_max_players_delta),
                min_playtime_minutes: base_props.min_playtime_minutes.map(|v| v + total_playtime_min_delta),
                max_playtime_minutes: base_props.max_playtime_minutes.map(|v| v + total_playtime_max_delta),
                weight: base_props.weight.map(|v| ((v + total_weight_delta) * 100.0).round() / 100.0),
                min_age: base_props.min_age.map(|v| v + total_min_age_delta),
            },
            base: base_props,
            applied_expansions: expansion_slugs,
            combination_source: "computed".to_string(),
        }));
    }

    // Tier 3: No expansion data, return base properties
    Ok(Json(EffectivePropertiesResponse {
        effective: EffectivePropertyValues {
            min_players: base_props.min_players,
            max_players: base_props.max_players,
            min_playtime_minutes: base_props.min_playtime_minutes,
            max_playtime_minutes: base_props.max_playtime_minutes,
            weight: base_props.weight,
            min_age: base_props.min_age,
        },
        base: base_props,
        applied_expansions: expansion_slugs,
        combination_source: "base_only".to_string(),
    }))
}
