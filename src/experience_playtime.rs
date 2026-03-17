// GET /v1/games/{id}/experience-playtime — ADR-0034 experience-bucketed model
// Returns per-level playtime data and multipliers

use axum::{
    extract::{Path, State},
    Json,
};
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

use crate::games::{resolve_game_id, internal_error, ApiError};
use crate::models::Link;
use crate::AppState;

#[derive(Debug, Serialize, FromRow)]
pub struct ExperienceLevel {
    pub experience_level: String,
    pub median_minutes: i32,
    pub p10_minutes: Option<i32>,
    pub p90_minutes: Option<i32>,
    pub report_count: i32,
}

#[derive(Debug, Serialize, FromRow)]
pub struct ExperienceProfile {
    pub sufficient_data: Option<bool>,
    pub multiplier_first_play: Option<f64>,
    pub multiplier_learning: Option<f64>,
    pub multiplier_experienced: Option<f64>,
    pub multiplier_expert: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct ExperiencePlaytimeResponse {
    pub game_id: String,
    pub levels: Vec<ExperienceLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multipliers: Option<ExperienceMultipliers>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sufficient_data: Option<bool>,
    pub _links: ExperienceLinks,
}

#[derive(Debug, Serialize)]
pub struct ExperienceMultipliers {
    pub first_play: Option<f64>,
    pub learning: Option<f64>,
    pub experienced: Option<f64>,
    pub expert: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct ExperienceLinks {
    #[serde(rename = "self")]
    pub self_link: Link,
    pub game: Link,
}

pub async fn get_experience_playtime(
    State(state): State<AppState>,
    Path(id_or_slug): Path<String>,
) -> Result<Json<ExperiencePlaytimeResponse>, ApiError> {
    let game_id = resolve_game_id(&state.db, &id_or_slug).await?;

    let levels = sqlx::query_as::<_, ExperienceLevel>(
        "SELECT experience_level, median_minutes, p10_minutes, p90_minutes, report_count
         FROM experience_playtime
         WHERE game_id = $1
         ORDER BY CASE experience_level
           WHEN 'first_play' THEN 1
           WHEN 'learning' THEN 2
           WHEN 'experienced' THEN 3
           WHEN 'expert' THEN 4
         END",
    )
    .bind(game_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| internal_error())?;

    let profile = sqlx::query_as::<_, ExperienceProfile>(
        "SELECT sufficient_data,
                multiplier_first_play::FLOAT8 as multiplier_first_play,
                multiplier_learning::FLOAT8 as multiplier_learning,
                multiplier_experienced::FLOAT8 as multiplier_experienced,
                multiplier_expert::FLOAT8 as multiplier_expert
         FROM experience_playtime_profiles
         WHERE game_id = $1",
    )
    .bind(game_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| internal_error())?;

    let (multipliers, sufficient_data) = if let Some(p) = profile {
        (
            Some(ExperienceMultipliers {
                first_play: p.multiplier_first_play,
                learning: p.multiplier_learning,
                experienced: p.multiplier_experienced,
                expert: p.multiplier_expert,
            }),
            p.sufficient_data,
        )
    } else {
        (None, None)
    };

    Ok(Json(ExperiencePlaytimeResponse {
        game_id: game_id.to_string(),
        levels,
        multipliers,
        sufficient_data,
        _links: ExperienceLinks {
            self_link: Link {
                href: format!("/v1/games/{}/experience-playtime", id_or_slug),
                title: None,
            },
            game: Link {
                href: format!("/v1/games/{}", id_or_slug),
                title: None,
            },
        },
    }))
}
