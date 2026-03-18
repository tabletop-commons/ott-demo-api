// POST /v1/admin/materialize — Trigger aggregate recomputation
// In production this would be a cron job (see deploying.md "Materialization Jobs").
// The demo API exposes it as an endpoint for manual triggering.
//
// Execution order (per materialization.md):
// 1. Per-game aggregates (rating, rating_votes, distribution, stddev)
// 2. Global parameters (global mean for Bayesian)
// 3. Bayesian ratings (bayes_rating) — Layer 4 implementation recommendation
// 4. Rankings (rank_overall)
// 5. Rating confidence — spec-level three-factor formula (rating-model.md Layer 3)
// 6. Weight materialization
// 7. Player count arrays

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
            rating = sub.avg_rating,
            rating_votes = sub.vote_count,
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

    // Step 2: Compute global mean (needed for Bayesian rating and confidence)
    let global_mean: Option<f64> =
        sqlx::query_scalar("SELECT AVG(rating)::FLOAT8 FROM games WHERE rating_votes > 0")
            .fetch_one(&state.db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let global_mean = global_mean.unwrap_or(5.5);

    // Step 3: Compute Bayesian rating (Layer 4 implementation recommendation)
    // bayes = (C * global_mean + sum_of_ratings) / (C + rating_votes)
    // C = prior weight (using 100 as a reasonable default for small datasets)
    let prior_weight = 100.0;
    sqlx::query(
        "UPDATE games SET
            bayes_rating = ((($1::FLOAT8 * $2::FLOAT8) + (rating::FLOAT8 * rating_votes::FLOAT8))
                / ($1::FLOAT8 + rating_votes::FLOAT8))::NUMERIC(4,2)
         WHERE rating_votes > 0",
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
            WHERE rating_votes > 0 AND type = 'base_game'
         ) sub
         WHERE g.id = sub.id",
    )
    .execute(&state.db)
    .await
    .map_err(|e| {
        tracing::error!("Materialization step 4 failed: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Step 5: Rating confidence — three-factor formula (rating-model.md Layer 3)
    // confidence = sample_factor × shape_factor × deviation_factor
    //   sample_factor    = rating_votes / (rating_votes + C)       -- Wilson-style, C=100
    //   shape_factor     = 1.0 - (stddev / 4.5)                   -- max stddev on 1-10 ≈ 4.5
    //   deviation_factor = 1.0 - (|rating - global_mean| / 4.5)   -- max deviation from mean ≈ 4.5
    // Clamped to [0.0, 1.0]
    sqlx::query(
        "UPDATE games SET
            rating_confidence = GREATEST(0.0, LEAST(1.0,
                (rating_votes::FLOAT8 / (rating_votes::FLOAT8 + 100.0))
                * (1.0 - COALESCE(rating_stddev::FLOAT8, 0.0) / 4.5)
                * (1.0 - ABS(rating::FLOAT8 - $1::FLOAT8) / 4.5)
            ))::NUMERIC(3,2)
         WHERE rating_votes > 0",
    )
    .bind(global_mean)
    .execute(&state.db)
    .await
    .map_err(|e| {
        tracing::error!("Materialization step 5 (rating_confidence) failed: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Step 6: Weight materialization from raw votes
    sqlx::query(
        "UPDATE games g SET
            weight = sub.avg_weight,
            weight_votes = sub.vote_count,
            updated_at = NOW()
         FROM (
            SELECT game_id,
                   AVG(weight)::NUMERIC(3,2) as avg_weight,
                   COUNT(*)::INTEGER as vote_count
            FROM weight_votes_raw
            GROUP BY game_id
         ) sub
         WHERE g.id = sub.game_id",
    )
    .execute(&state.db)
    .await
    .map_err(|e| {
        tracing::error!("Materialization step 6 (weight) failed: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Step 7: Derive top/recommended player counts from player_count_ratings
    // top = counts rated >= 4.0, recommended = counts rated >= 3.0
    sqlx::query(
        "UPDATE games g SET
            top_player_counts = sub.top_counts,
            recommended_player_counts = sub.rec_counts,
            updated_at = NOW()
         FROM (
            SELECT pcr.game_id,
                   ARRAY_AGG(pcr.player_count ORDER BY pcr.player_count)
                       FILTER (WHERE pcr.average_rating >= 4.0) as top_counts,
                   ARRAY_AGG(pcr.player_count ORDER BY pcr.player_count)
                       FILTER (WHERE pcr.average_rating >= 3.0) as rec_counts
            FROM player_count_ratings pcr
            GROUP BY pcr.game_id
         ) sub
         WHERE g.id = sub.game_id",
    )
    .execute(&state.db)
    .await
    .map_err(|e| {
        tracing::error!("Materialization step 7 (player count arrays) failed: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Step 8: Write game snapshots (ADR-0036, materialization.md)
    // Snapshots are a side effect of materialization — capture the freshly-computed
    // aggregates for longitudinal trend analysis.
    sqlx::query(
        "INSERT INTO game_snapshots (game_id, snapshot_date, rating, rating_votes, rating_confidence,
                                     weight, weight_votes, rank_overall, owner_count)
         SELECT id, CURRENT_DATE, rating, rating_votes, rating_confidence,
                weight, weight_votes, rank_overall, owner_count
         FROM games
         WHERE rating_votes > 0
         ON CONFLICT (game_id, snapshot_date) DO UPDATE SET
            rating = EXCLUDED.rating,
            rating_votes = EXCLUDED.rating_votes,
            rating_confidence = EXCLUDED.rating_confidence,
            weight = EXCLUDED.weight,
            weight_votes = EXCLUDED.weight_votes,
            rank_overall = EXCLUDED.rank_overall,
            owner_count = EXCLUDED.owner_count",
    )
    .execute(&state.db)
    .await
    .map_err(|e| {
        tracing::error!("Materialization step 8 (snapshots) failed: {:?}", e);
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
            "Materialized aggregates for {} games. Global mean: {:.2}. Rankings, confidence, weight, and player count arrays updated.",
            games_updated, global_mean
        ),
    }))
}
