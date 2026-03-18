# ott-demo-api

Demo API server conforming to the [OpenTabletop](https://github.com/tabletop-commons) commons spec. Built in Rust with [Axum](https://github.com/tokio-rs/axum) and PostgreSQL, this reference implementation exercises the spec's core pillars: game metadata, community ratings, expansion-aware filtering, compound search, and vote materialization.

## Quick start

```bash
# Start PostgreSQL (auto-loads schema + seed data)
docker compose up -d

# Run the API server
cargo run

# Verify
curl http://localhost:8080/healthz
# {"status":"ok"}
```

The seed data includes two base games (Terraforming Mars, Spirit Island), several expansions, categories, player-count ratings, experience playtime data, and pre-materialized aggregates.

## API endpoints

### Health

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/healthz` | Liveness probe |
| `GET` | `/readyz` | Readiness probe (checks DB connection) |

### Games

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/v1/games` | List games with filtering and keyset pagination |
| `GET` | `/v1/games/{id_or_slug}` | Get a game by UUID or slug |
| `GET` | `/v1/games/{id_or_slug}/expansions` | List expansions for a base game |
| `GET` | `/v1/games/{id_or_slug}/player-count-ratings` | Community ratings per player count |
| `GET` | `/v1/games/{id_or_slug}/effective-properties` | Expansion-aware effective properties (three-tier resolution) |
| `GET` | `/v1/games/{id_or_slug}/relationships` | Typed directed relationships (expands, reimplements, etc.) |
| `GET` | `/v1/games/{id_or_slug}/experience-playtime` | Playtime by experience level (first play through expert) |
| `GET` | `/v1/games/{id_or_slug}/snapshots` | Historical trend snapshots |

### Search

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/v1/search` | Full-text search (PostgreSQL tsvector/tsquery) |
| `POST` | `/v1/games/search` | Advanced compound filtering across 8 dimensions |

### Taxonomy

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/v1/mechanics` | List all mechanics (hierarchical) |
| `GET` | `/v1/categories` | List all categories |
| `GET` | `/v1/themes` | List all themes |

### Votes

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/v1/games/{id_or_slug}/ratings` | Submit a rating vote (1-10) |
| `POST` | `/v1/games/{id_or_slug}/weight` | Submit a weight vote (1.0-5.0) |

### Admin

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/v1/admin/materialize` | Trigger aggregate recomputation pipeline |

## Key features

- **Effective mode** -- `?effective=true` filters consider expansion combinations via three-tier resolution (explicit combinations, summed deltas, base-only fallback), returning `matched_via` metadata
- **Compound search** -- 8 independent filter dimensions (rating, weight, player count, playtime, age, type/mechanics, theme, metadata) with cross-dimension AND and within-dimension OR semantics
- **Vote materialization** -- append-only raw votes feed an 8-step batch pipeline: per-game aggregates, Bayesian ratings (C=100 prior), rankings, confidence scoring, weight aggregation, player-count arrays, and snapshot capture
- **Keyset pagination** -- cursor-based pagination with base64url-encoded cursors and HAL-style `_links`
- **Dual playtime model** -- publisher-declared and community-reported playtime tracked independently, with experience-bucketed breakdowns and multipliers
- **RFC 9457 errors** -- Problem Details responses for all error cases

## Project structure

```
src/
  main.rs                  Server setup, router, AppState
  models.rs                Data models and error types
  health.rs                Health check endpoints
  games.rs                 Game listing, filtering, detail
  player_counts.rs         Player count ratings
  experience_playtime.rs   Experience-bucketed playtime
  effective.rs             Three-tier effective properties
  relationships.rs         Game relationship edges
  snapshots.rs             Historical trend snapshots
  fulltext_search.rs       Full-text search (tsvector)
  search.rs                Compound multi-dimension search
  taxonomy.rs              Mechanics, categories, themes
  votes.rs                 Rating and weight vote submission
  materialize.rs           Batch materialization pipeline

data/
  schema.sql               Full PostgreSQL schema
  seed.sql                 Sample data for local development
```

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | -- | PostgreSQL connection string |
| `PORT` | `8080` | HTTP listen port |
| `LOG_LEVEL` | `debug` | tracing filter directive |
| `LOG_FORMAT` | `pretty` | `pretty` for development, `json` for production |

## Database

PostgreSQL 17. The `docker-compose.yml` automatically loads `data/schema.sql` and `data/seed.sql` on first start. Key tables:

- **`games`** -- polymorphic game entity with type discriminator (base_game, expansion, standalone_expansion, promo, accessory, fan_expansion)
- **`game_relationships`** -- typed directed edges between games
- **`player_count_ratings`** -- per-count community ratings
- **`experience_playtime`** -- playtime by experience level with statistical percentiles
- **`expansion_combinations`** / **`property_modifications`** -- three-tier effective property resolution
- **`rating_votes`** / **`weight_votes_raw`** -- append-only raw vote storage
- **`game_snapshots`** -- daily materialized snapshots for longitudinal analysis
- **`mechanics`**, **`categories`**, **`themes`** -- hierarchical taxonomies
- **`game_editions`**, **`people`**, **`awards`** -- edition tracking, credits, and awards

## Architecture notes

This implementation follows decisions documented in the OpenTabletop spec pillars:

- **ADR-0006** -- Unified game entity with type discriminator
- **ADR-0007** -- Three-tier expansion property resolution
- **ADR-0012** -- Keyset pagination
- **ADR-0015** -- RFC 9457 Problem Details errors
- **ADR-0018** -- HAL-style hypermedia links
- **ADR-0020** -- 12-factor configuration
- **ADR-0021** -- Distroless container images
- **ADR-0025** -- Axum framework
- **ADR-0034** -- Experience-bucketed playtime

See the spec's rating-model, weight-model, playtime, property-deltas, relationships, materialization, and effective-mode documents for detailed design rationale.

## License

Apache-2.0
