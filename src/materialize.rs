// POST /v1/admin/materialize — Trigger aggregate recomputation
// In production this would be a cron job (see deploying.md "Materialization Jobs").
// The demo API exposes it as an endpoint for manual triggering.
//
// Execution order (per deploying guide):
// 1. Per-game aggregates (average_rating, rating_count, distribution, stddev)
// 2. Global parameters (global mean for Bayesian)
// 3. Bayesian ratings (bayes_rating)
// 4. Rankings (rank_overall)

use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct MaterializeResponse {
    pub status: &'static str,
    pub games_updated: i64,
    pub message: String,
}

pub async fn materialize(
    State(state): State<AppState>,
) -> Result<Json<MaterializeResponse>, StatusCode> {
    // Step 1: Recompute per-game rating aggregates from raw votes
    let result = sqlx::query(
        "UPDATE games g SET
            average_rating = sub.avg_rating,
            rating_count = sub.vote_count,
            rating_stddev = sub.stddev_rating,
            rating_distribution = sub.distribution,
            updated_at = NOW()
         FROM (
            SELECT
                game_id,
                AVG(rating)::NUMERIC(4,2) as avg_rating,
                COUNT(*)::INTEGER as vote_count,
                STDDEV(rating)::NUMERIC(4,2) as stddev_rating,
                ARRAY[
                    COUNT(*) FILTER (WHERE rating = 1),
                    COUNT(*) FILTER (WHERE rating = 2),
                    COUNT(*) FILTER (WHERE rating = 3),
                    COUNT(*) FILTER (WHERE rating = 4),
                    COUNT(*) FILTER (WHERE rating = 5),
                    COUNT(*) FILTER (WHERE rating = 6),
                    COUNT(*) FILTER (WHERE rating = 7),
                    COUNT(*) FILTER (WHERE rating = 8),
                    COUNT(*) FILTER (WHERE rating = 9),
                    COUNT(*) FILTER (WHERE rating = 10)
                ]::INTEGER[] as distribution
            FROM rating_votes
            GROUP BY game_id
         ) sub
         WHERE g.id = sub.game_id",
    )
    .execute(&state.db)
    .await
    .map_err(|e| {
        tracing::error!("Materialization step 1 failed: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let games_updated = result.rows_affected() as i64;

    // Step 2: Compute global mean (needed for Bayesian rating)
    let global_mean: Option<f64> =
        sqlx::query_scalar("SELECT AVG(average_rating)::FLOAT8 FROM games WHERE rating_count > 0")
            .fetch_one(&state.db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let global_mean = global_mean.unwrap_or(5.5);

    // Step 3: Compute Bayesian rating (Dirichlet-prior approximation)
    // bayes = (C * global_mean + sum_of_ratings) / (C + rating_count)
    // C = prior weight (using 100 as a reasonable default for small datasets)
    let prior_weight = 100.0;
    sqlx::query(
        "UPDATE games SET
            bayes_rating = ((($1::FLOAT8 * $2::FLOAT8) + (average_rating::FLOAT8 * rating_count::FLOAT8))
                / ($1::FLOAT8 + rating_count::FLOAT8))::NUMERIC(4,2)
         WHERE rating_count > 0",
    )
    .bind(prior_weight)
    .bind(global_mean)
    .execute(&state.db)
    .await
    .map_err(|e| {
        tracing::error!("Materialization step 3 failed: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Step 4: Recompute rankings (sorted by bayes_rating)
    sqlx::query(
        "UPDATE games g SET rank_overall = sub.rank
         FROM (
            SELECT id, ROW_NUMBER() OVER (ORDER BY bayes_rating DESC NULLS LAST) as rank
            FROM games
            WHERE rating_count > 0 AND type = 'base_game'
         ) sub
         WHERE g.id = sub.id",
    )
    .execute(&state.db)
    .await
    .map_err(|e| {
        tracing::error!("Materialization step 4 failed: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    tracing::info!(
        "Materialization complete: {} games updated, global_mean={:.2}",
        games_updated,
        global_mean
    );

    Ok(Json(MaterializeResponse {
        status: "complete",
        games_updated,
        message: format!(
            "Materialized aggregates for {} games. Global mean: {:.2}. Rankings updated.",
            games_updated, global_mean
        ),
    }))
}
