// GET /v1/games/{id}/relationships -- typed directed edges (relationships.md)
// Returns expands, reimplements, contains, requires, recommends, integrates_with

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::games::{resolve_game_id, ApiError, internal_error};
use crate::models::Link;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct RelationshipParams {
    #[serde(rename = "type")]
    pub rel_type: Option<String>,
    pub direction: Option<String>, // "outbound" or "inbound"
}

#[derive(Debug, Serialize, FromRow)]
pub struct GameRelationship {
    pub source_game_id: Uuid,
    pub target_game_id: Uuid,
    pub relationship_type: String,
    pub ordinal: Option<i32>,
    // Denormalized names for convenience
    pub source_slug: Option<String>,
    pub source_name: Option<String>,
    pub target_slug: Option<String>,
    pub target_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RelationshipsResponse {
    pub data: Vec<GameRelationship>,
    pub _links: RelationshipLinks,
}

#[derive(Debug, Serialize)]
pub struct RelationshipLinks {
    #[serde(rename = "self")]
    pub self_link: Link,
    pub game: Link,
}

pub async fn get_relationships(
    State(state): State<AppState>,
    Path(id_or_slug): Path<String>,
    Query(params): Query<RelationshipParams>,
) -> Result<Json<RelationshipsResponse>, ApiError> {
    let game_id = resolve_game_id(&state.db, &id_or_slug).await?;

    let direction = params.direction.as_deref().unwrap_or("both");

    let relationships = match (direction, params.rel_type.as_deref()) {
        ("outbound", Some(t)) => {
            sqlx::query_as::<_, GameRelationship>(
                "SELECT r.source_game_id, r.target_game_id, r.relationship_type, r.ordinal,
                        s.slug as source_slug, s.name as source_name,
                        t.slug as target_slug, t.name as target_name
                 FROM game_relationships r
                 JOIN games s ON s.id = r.source_game_id
                 JOIN games t ON t.id = r.target_game_id
                 WHERE r.source_game_id = $1 AND r.relationship_type = $2
                 ORDER BY r.ordinal, t.name",
            )
            .bind(game_id)
            .bind(t)
            .fetch_all(&state.db)
            .await
        }
        ("inbound", Some(t)) => {
            sqlx::query_as::<_, GameRelationship>(
                "SELECT r.source_game_id, r.target_game_id, r.relationship_type, r.ordinal,
                        s.slug as source_slug, s.name as source_name,
                        t.slug as target_slug, t.name as target_name
                 FROM game_relationships r
                 JOIN games s ON s.id = r.source_game_id
                 JOIN games t ON t.id = r.target_game_id
                 WHERE r.target_game_id = $1 AND r.relationship_type = $2
                 ORDER BY r.ordinal, s.name",
            )
            .bind(game_id)
            .bind(t)
            .fetch_all(&state.db)
            .await
        }
        _ => {
            // Both directions, optionally filtered by type
            sqlx::query_as::<_, GameRelationship>(
                "SELECT r.source_game_id, r.target_game_id, r.relationship_type, r.ordinal,
                        s.slug as source_slug, s.name as source_name,
                        t.slug as target_slug, t.name as target_name
                 FROM game_relationships r
                 JOIN games s ON s.id = r.source_game_id
                 JOIN games t ON t.id = r.target_game_id
                 WHERE r.source_game_id = $1 OR r.target_game_id = $1
                 ORDER BY r.relationship_type, r.ordinal",
            )
            .bind(game_id)
            .fetch_all(&state.db)
            .await
        }
    }
    .map_err(|_| internal_error())?;

    Ok(Json(RelationshipsResponse {
        data: relationships,
        _links: RelationshipLinks {
            self_link: Link {
                href: format!("/v1/games/{}/relationships", id_or_slug),
                title: None,
            },
            game: Link {
                href: format!("/v1/games/{}", id_or_slug),
                title: None,
            },
        },
    }))
}
