// GET /v1/search?q=... — Full-text search (ADR-0027)
// Uses PostgreSQL tsvector/tsquery with ts_rank for relevance ordering

use axum::{extract::{Query, State}, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::models::Link;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub entity_type: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct SearchResult {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    #[sqlx(rename = "type")]
    #[serde(rename = "type")]
    pub game_type: String,
    pub year_published: Option<i32>,
    pub rank: f32,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub data: Vec<SearchResult>,
    pub query: String,
    pub _links: SearchLinks,
}

#[derive(Debug, Serialize)]
pub struct SearchLinks {
    #[serde(rename = "self")]
    pub self_link: Link,
}

pub async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<SearchResponse>, StatusCode> {
    let limit = params.limit.unwrap_or(25).min(100);

    let results = sqlx::query_as::<_, SearchResult>(
        "SELECT id, slug, name, type, year_published,
                ts_rank(search_vector, plainto_tsquery('english', $1)) as rank
         FROM games
         WHERE search_vector @@ plainto_tsquery('english', $1)
         ORDER BY rank DESC
         LIMIT $2",
    )
    .bind(&params.q)
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        tracing::error!("Search error: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(SearchResponse {
        query: params.q.clone(),
        data: results,
        _links: SearchLinks {
            self_link: Link {
                href: format!("/v1/search?q={}", params.q),
                title: None,
            },
        },
    }))
}
