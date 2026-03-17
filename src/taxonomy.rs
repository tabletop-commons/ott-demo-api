// Taxonomy endpoints (Implementing Guide Step 4)
// GET /v1/mechanics, /v1/categories, /v1/themes

use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

use crate::AppState;

#[derive(Debug, Serialize, FromRow)]
pub struct TaxonomyTerm {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TaxonomyResponse {
    pub data: Vec<TaxonomyTerm>,
}

pub async fn list_mechanics(
    State(state): State<AppState>,
) -> Result<Json<TaxonomyResponse>, StatusCode> {
    let terms = sqlx::query_as::<_, TaxonomyTerm>(
        "SELECT id, slug, name, description FROM mechanics ORDER BY name",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(TaxonomyResponse { data: terms }))
}

pub async fn list_categories(
    State(state): State<AppState>,
) -> Result<Json<TaxonomyResponse>, StatusCode> {
    let terms = sqlx::query_as::<_, TaxonomyTerm>(
        "SELECT id, slug, name, description FROM categories ORDER BY name",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(TaxonomyResponse { data: terms }))
}

pub async fn list_themes(
    State(state): State<AppState>,
) -> Result<Json<TaxonomyResponse>, StatusCode> {
    let terms = sqlx::query_as::<_, TaxonomyTerm>(
        "SELECT id, slug, name, description FROM themes ORDER BY name",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(TaxonomyResponse { data: terms }))
}
