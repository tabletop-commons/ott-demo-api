// Game model matching the spec's Game schema (spec/schemas/Game.yaml)
// Required fields: id, slug, name, type

use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Serialize, FromRow)]
pub struct Game {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    #[sqlx(rename = "type")]
    #[serde(rename = "type")]
    pub game_type: String,
    pub sort_name: Option<String>,
    pub parent_game_id: Option<Uuid>,
    pub year_published: Option<i32>,
    pub description: Option<String>,
    pub description_short: Option<String>,
    pub min_players: Option<i32>,
    pub max_players: Option<i32>,
    pub min_playtime: Option<i32>,
    pub max_playtime: Option<i32>,
    pub community_min_playtime: Option<i32>,
    pub community_max_playtime: Option<i32>,
    pub community_median_playtime: Option<i32>,
    pub min_age: Option<i32>,
    pub community_suggested_age: Option<i32>,
    pub rating: Option<f64>,
    #[serde(skip_serializing)] // Internal: used for rank_overall computation, not spec-level (rating-model.md Layer 4)
    #[allow(dead_code)]
    pub bayes_rating: Option<f64>,
    pub rating_votes: Option<i32>,
    pub rating_stddev: Option<f64>,
    pub rating_confidence: Option<f64>,
    pub rating_distribution: Option<Vec<i32>>,
    pub weight: Option<f64>,
    pub weight_votes: Option<i32>,
    pub rank_overall: Option<i32>,
    pub owner_count: Option<i32>,
    pub wishlist_count: Option<i32>,
    pub total_plays: Option<i32>,
    pub mode: Option<String>,
    pub funding_source: Option<String>,
    pub language_dependence: Option<String>,
    pub image_url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub bgg_id: Option<i32>,
    pub status: String,
    pub top_player_counts: Option<Vec<i32>>,
    pub recommended_player_counts: Option<Vec<i32>>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

// HAL-style _links (per ADR-0018)
#[derive(Debug, Serialize)]
pub struct Link {
    pub href: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

// Paginated response (per ADR-0012: keyset pagination)
#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T: Serialize> {
    pub data: Vec<T>,
    pub meta: PaginationMeta,
    pub _links: PaginationLinks,
}

#[derive(Debug, Serialize)]
pub struct PaginationMeta {
    pub total: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PaginationLinks {
    #[serde(rename = "self")]
    pub self_link: Link,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<Link>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev: Option<Link>,
}

// Effective mode matched_via metadata (per effective-mode.md Response Format)
#[derive(Debug, Serialize, Clone)]
pub struct MatchedVia {
    #[serde(rename = "type")]
    pub match_type: String, // "base", "expansion_combination", "delta_sum"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expansions: Option<Vec<MatchedExpansion>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_properties: Option<EffectiveMatchProperties>,
    pub resolution_tier: i32, // 1, 2, or 3
}

#[derive(Debug, Serialize, Clone)]
pub struct MatchedExpansion {
    pub slug: String,
    pub name: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct EffectiveMatchProperties {
    pub min_players: Option<i32>,
    pub max_players: Option<i32>,
    pub weight: Option<f64>,
    pub min_playtime: Option<i32>,
    pub max_playtime: Option<i32>,
}

// Game with optional matched_via for effective mode results
#[derive(Debug, Serialize)]
pub struct GameWithMatch {
    #[serde(flatten)]
    pub game: Game,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_via: Option<MatchedVia>,
}

// RFC 9457 Problem Details error response (per ADR-0015)
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    #[serde(rename = "type")]
    pub error_type: String,
    pub title: String,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}
