use axum::{routing::{get, post}, Router};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::net::SocketAddr;
use tracing_subscriber::EnvFilter;

mod effective;
mod experience_playtime;
mod fulltext_search;
mod games;
mod health;
mod materialize;
mod models;
mod player_counts;
mod relationships;
mod search;
mod snapshots;
mod taxonomy;
mod votes;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
}

#[tokio::main]
async fn main() {
    // Load .env for local development (12-factor: config from env, per ADR-0020)
    dotenvy::dotenv().ok();

    // Structured logging (per ADR-0023)
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse()
        .expect("PORT must be a valid u16");

    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&database_url)
        .await
        .expect("Failed to connect to database");

    let state = AppState { db: pool };

    let app = Router::new()
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz))
        .route("/v1/games", get(games::list_games))
        .route("/v1/games/{id_or_slug}", get(games::get_game))
        .route("/v1/games/{id_or_slug}/expansions", get(games::list_expansions))
        .route("/v1/games/{id_or_slug}/player-count-ratings", get(player_counts::get_player_count_ratings))
        .route("/v1/games/{id_or_slug}/effective-properties", get(effective::get_effective_properties))
        .route("/v1/games/{id_or_slug}/relationships", get(relationships::get_relationships))
        .route("/v1/games/{id_or_slug}/experience-playtime", get(experience_playtime::get_experience_playtime))
        .route("/v1/search", get(fulltext_search::search))
        .route("/v1/games/{id_or_slug}/ratings", post(votes::submit_rating))
        .route("/v1/games/{id_or_slug}/weight", post(votes::submit_weight))
        .route("/v1/games/{id_or_slug}/snapshots", get(snapshots::get_snapshots))
        .route("/v1/admin/materialize", post(materialize::materialize))
        .route("/v1/games/search", post(search::search_games))
        .route("/v1/mechanics", get(taxonomy::list_mechanics))
        .route("/v1/categories", get(taxonomy::list_categories))
        .route("/v1/themes", get(taxonomy::list_themes))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
