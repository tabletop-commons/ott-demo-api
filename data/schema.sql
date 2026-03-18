-- OpenTabletop Recommended PostgreSQL Schema
-- This is a reference schema for implementers, not a requirement.
-- See docs/src/guides/implementing.md for how to use this.

-- ============================================================
-- Extensions
-- ============================================================

CREATE EXTENSION IF NOT EXISTS "pgcrypto";  -- for gen_random_uuid() fallback

-- ============================================================
-- Controlled Vocabularies (from data/taxonomy/)
-- ============================================================

CREATE TABLE mechanics (
    id          UUID PRIMARY KEY,
    slug        VARCHAR(255) UNIQUE NOT NULL,
    name        VARCHAR(255) NOT NULL,
    description TEXT,
    parent_id   UUID REFERENCES mechanics(id)
);

CREATE TABLE categories (
    id          UUID PRIMARY KEY,
    slug        VARCHAR(255) UNIQUE NOT NULL,
    name        VARCHAR(255) NOT NULL,
    description TEXT,
    parent_id   UUID REFERENCES categories(id)
);

CREATE TABLE themes (
    id          UUID PRIMARY KEY,
    slug        VARCHAR(255) UNIQUE NOT NULL,
    name        VARCHAR(255) NOT NULL,
    description TEXT,
    parent_id   UUID REFERENCES themes(id)
);

-- ============================================================
-- Core Game Entity (ADR-0006: Unified Game with Type Discriminator)
-- ============================================================

CREATE TABLE games (
    id                              UUID PRIMARY KEY,
    slug                            VARCHAR(255) UNIQUE NOT NULL,
    name                            VARCHAR(500) NOT NULL,
    sort_name                       VARCHAR(500),
    type                            VARCHAR(50) NOT NULL
        CHECK (type IN ('base_game', 'expansion', 'standalone_expansion', 'promo', 'accessory', 'fan_expansion')),
    parent_game_id                  UUID REFERENCES games(id),
    year_published                  INTEGER,
    description                     TEXT,
    description_short               VARCHAR(1000),

    -- Player count (publisher-stated)
    min_players                     INTEGER,
    max_players                     INTEGER,

    -- Playtime: dual model (ADR-0014, games.md)
    min_playtime                    INTEGER,
    max_playtime                    INTEGER,
    community_min_playtime          INTEGER,
    community_max_playtime          INTEGER,
    community_median_playtime       INTEGER,

    -- Age
    min_age                         INTEGER,
    community_suggested_age         INTEGER,

    -- Rating: four-layer model (see rating-model.md, games.md)
    rating                          NUMERIC(4,2),
    bayes_rating                    NUMERIC(4,2),
    rating_votes                    INTEGER DEFAULT 0,
    rating_stddev                   NUMERIC(4,2),
    rating_confidence               NUMERIC(3,2),
    rating_distribution             INTEGER[],  -- 10 buckets (1-10 stars)

    -- Weight (see weight-model.md)
    weight                          NUMERIC(3,2),
    weight_votes                    INTEGER DEFAULT 0,

    -- Derived player count arrays (materialized from player_count_ratings)
    top_player_counts               INTEGER[],
    recommended_player_counts       INTEGER[],

    -- Community signals (ADR-0041)
    rank_overall                    INTEGER,
    rank_by_category                JSONB,
    owner_count                     INTEGER DEFAULT 0,
    wishlist_count                  INTEGER DEFAULT 0,
    total_plays                     INTEGER DEFAULT 0,

    -- Classification
    mode                            VARCHAR(50),
    funding_source                  VARCHAR(50),
    language_dependence             VARCHAR(50),

    -- Images
    image_url                       VARCHAR(1000),
    thumbnail_url                   VARCHAR(1000),

    -- External IDs
    bgg_id                          INTEGER,

    -- Audit
    status                          VARCHAR(50) NOT NULL DEFAULT 'active',
    created_at                      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at                      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_games_type ON games(type, status);
CREATE INDEX idx_games_parent ON games(parent_game_id);
CREATE INDEX idx_games_year ON games(year_published);
CREATE INDEX idx_games_rating ON games(rating DESC, rating_votes DESC);
CREATE INDEX idx_games_weight ON games(weight);
CREATE INDEX idx_games_bgg ON games(bgg_id);
CREATE INDEX idx_games_rating_confidence ON games(rating_confidence);
CREATE INDEX idx_games_min_age ON games(min_age);
CREATE INDEX idx_games_community_age ON games(community_suggested_age);

-- Full-text search (ADR-0027)
ALTER TABLE games ADD COLUMN search_vector tsvector
    GENERATED ALWAYS AS (
        setweight(to_tsvector('english', coalesce(name, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(description_short, '')), 'B') ||
        setweight(to_tsvector('english', coalesce(description, '')), 'C')
    ) STORED;
CREATE INDEX idx_games_search ON games USING GIN(search_vector);

-- ============================================================
-- Game ↔ Taxonomy Junction Tables
-- ============================================================

CREATE TABLE game_mechanics (
    game_id     UUID NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    mechanic_id UUID NOT NULL REFERENCES mechanics(id),
    PRIMARY KEY (game_id, mechanic_id)
);
CREATE INDEX idx_game_mechanics_mechanic ON game_mechanics(mechanic_id);

CREATE TABLE game_categories (
    game_id     UUID NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    category_id UUID NOT NULL REFERENCES categories(id),
    PRIMARY KEY (game_id, category_id)
);
CREATE INDEX idx_game_categories_category ON game_categories(category_id);

CREATE TABLE game_themes (
    game_id  UUID NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    theme_id UUID NOT NULL REFERENCES themes(id),
    PRIMARY KEY (game_id, theme_id)
);
CREATE INDEX idx_game_themes_theme ON game_themes(theme_id);

-- ============================================================
-- Game Relationships (ADR-0011: Typed directed edges)
-- ============================================================

CREATE TABLE game_relationships (
    source_game_id UUID NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    target_game_id UUID NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    relationship_type VARCHAR(50) NOT NULL
        CHECK (relationship_type IN ('expands', 'reimplements', 'contains', 'requires', 'recommends', 'integrates_with')),
    ordinal        INTEGER DEFAULT 0,
    PRIMARY KEY (source_game_id, target_game_id, relationship_type)
);
CREATE INDEX idx_game_rels_target ON game_relationships(target_game_id);

-- ============================================================
-- Player Count Ratings (ADR-0010, ADR-0043: numeric 1-5 model)
-- ============================================================

CREATE TABLE player_count_ratings (
    game_id         UUID NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    player_count    INTEGER NOT NULL,
    average_rating  NUMERIC(2,1) NOT NULL,  -- 1.0 to 5.0
    rating_count    INTEGER NOT NULL DEFAULT 0,
    rating_stddev   NUMERIC(4,3),
    PRIMARY KEY (game_id, player_count)
);

-- ============================================================
-- Experience-Bucketed Playtime (ADR-0034)
-- ============================================================

CREATE TABLE experience_playtime (
    game_id          UUID NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    experience_level VARCHAR(50) NOT NULL
        CHECK (experience_level IN ('first_play', 'learning', 'experienced', 'expert')),
    median_minutes   INTEGER NOT NULL,
    p10_minutes      INTEGER,
    p90_minutes      INTEGER,
    report_count     INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (game_id, experience_level)
);

CREATE TABLE experience_playtime_profiles (
    game_id              UUID PRIMARY KEY REFERENCES games(id) ON DELETE CASCADE,
    sufficient_data      BOOLEAN DEFAULT FALSE,
    multiplier_first_play NUMERIC(3,2),
    multiplier_learning   NUMERIC(3,2),
    multiplier_experienced NUMERIC(3,2) DEFAULT 1.00,
    multiplier_expert     NUMERIC(3,2)
);

-- ============================================================
-- Expansion Combinations (ADR-0007: three-tier resolution)
-- ============================================================
-- Tier 1: Explicit records (this table)
-- Tier 2: Computed from property_modifications (fallback)
-- Tier 3: Base game only (no data needed)

CREATE TABLE expansion_combinations (
    id                      UUID PRIMARY KEY,
    base_game_id            UUID NOT NULL REFERENCES games(id),
    expansion_ids           UUID[] NOT NULL,
    source                  VARCHAR(50) NOT NULL DEFAULT 'community_poll',
    effective_min_players   INTEGER,
    effective_max_players   INTEGER,
    effective_weight        NUMERIC(3,2),
    effective_playtime_min  INTEGER,
    effective_playtime_max  INTEGER,
    effective_min_age       INTEGER,
    top_at                  INTEGER[],
    recommended_at          INTEGER[]
);
CREATE INDEX idx_expansion_combos_base ON expansion_combinations(base_game_id);

-- ============================================================
-- Property Modifications (individual expansion deltas)
-- ============================================================

CREATE TABLE property_modifications (
    expansion_id        UUID NOT NULL REFERENCES games(id),
    base_game_id        UUID NOT NULL REFERENCES games(id),
    max_players_delta   INTEGER DEFAULT 0,
    weight_delta        NUMERIC(3,2) DEFAULT 0.0,
    playtime_min_delta  INTEGER DEFAULT 0,
    playtime_max_delta  INTEGER DEFAULT 0,
    min_age_delta       INTEGER DEFAULT 0,
    description         TEXT,
    PRIMARY KEY (expansion_id, base_game_id)
);

-- ============================================================
-- People & Credits (ADR-0039)
-- ============================================================

CREATE TABLE people (
    id          UUID PRIMARY KEY,
    slug        VARCHAR(255) UNIQUE NOT NULL,
    name        VARCHAR(500) NOT NULL,
    description TEXT
);

CREATE TABLE game_credits (
    game_id     UUID NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    person_id   UUID NOT NULL REFERENCES people(id),
    role        VARCHAR(50) NOT NULL,
    credit_order INTEGER DEFAULT 0,
    PRIMARY KEY (game_id, person_id, role)
);
CREATE INDEX idx_game_credits_person ON game_credits(person_id);
CREATE INDEX idx_game_credits_role ON game_credits(role);

CREATE TABLE publishers (
    id          UUID PRIMARY KEY,
    slug        VARCHAR(255) UNIQUE NOT NULL,
    name        VARCHAR(500) NOT NULL,
    description TEXT,
    country     VARCHAR(2),
    website     VARCHAR(1000)
);

-- ============================================================
-- Editions (ADR-0035, ADR-0040)
-- ============================================================

CREATE TABLE game_editions (
    id              UUID PRIMARY KEY,
    game_id         UUID NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    name            VARCHAR(500) NOT NULL,
    year_published  INTEGER,
    language        VARCHAR(10),
    release_status  VARCHAR(50),
    is_canonical    BOOLEAN DEFAULT FALSE,
    product_codes   JSONB,
    dimensions      JSONB,
    box_weight      JSONB,
    image_url       VARCHAR(1000),
    notes           TEXT
);
CREATE INDEX idx_editions_game ON game_editions(game_id);

-- Edition ↔ Publisher junction table (editions can have multiple publishers)
CREATE TABLE edition_publishers (
    edition_id   UUID NOT NULL REFERENCES game_editions(id) ON DELETE CASCADE,
    publisher_id UUID NOT NULL REFERENCES publishers(id),
    PRIMARY KEY (edition_id, publisher_id)
);
CREATE INDEX idx_edition_publishers_pub ON edition_publishers(publisher_id);

-- ============================================================
-- Awards (ADR-0042)
-- ============================================================

CREATE TABLE awards (
    id          UUID PRIMARY KEY,
    slug        VARCHAR(255) UNIQUE NOT NULL,
    name        VARCHAR(500) NOT NULL,
    organization VARCHAR(500),
    website     VARCHAR(1000)
);

CREATE TABLE game_awards (
    game_id     UUID NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    award_id    UUID NOT NULL REFERENCES awards(id),
    year        INTEGER NOT NULL,
    result      VARCHAR(50) NOT NULL
        CHECK (result IN ('nominated', 'shortlisted', 'recommended', 'won')),
    category    VARCHAR(255),
    PRIMARY KEY (game_id, award_id, year, result)
);
CREATE INDEX idx_game_awards_award ON game_awards(award_id, year);

-- ============================================================
-- Alternate Names (ADR-0038)
-- ============================================================

CREATE TABLE alternate_names (
    id       UUID PRIMARY KEY,
    game_id  UUID NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    name     VARCHAR(500) NOT NULL,
    language VARCHAR(10),
    source   VARCHAR(50)
);
CREATE INDEX idx_alt_names_game ON alternate_names(game_id);

-- ============================================================
-- External Identifiers (ADR-0008)
-- ============================================================

CREATE TABLE external_identifiers (
    game_id     UUID NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    source      VARCHAR(50) NOT NULL,
    external_id VARCHAR(255) NOT NULL,
    PRIMARY KEY (game_id, source, external_id)
);

-- ============================================================
-- Game Snapshots (ADR-0036: longitudinal trends)
-- ============================================================

CREATE TABLE game_snapshots (
    game_id           UUID NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    snapshot_date     DATE NOT NULL,
    rating            NUMERIC(4,2),
    rating_votes      INTEGER,
    rating_confidence NUMERIC(3,2),
    weight            NUMERIC(3,2),
    weight_votes      INTEGER,
    rank_overall      INTEGER,
    rank_by_category  JSONB,
    play_count_period INTEGER,
    owner_count       INTEGER,
    PRIMARY KEY (game_id, snapshot_date)
);

-- ============================================================
-- Raw Vote Tables (Tier 1: append-only input data)
-- ============================================================

CREATE TABLE rating_votes (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    game_id          UUID NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    rating           INTEGER NOT NULL CHECK (rating >= 1 AND rating <= 10),
    declared_scale   VARCHAR(10),
    play_count       INTEGER,
    experience_level VARCHAR(50),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_rating_votes_game ON rating_votes(game_id);

CREATE TABLE weight_votes_raw (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    game_id     UUID NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    weight      NUMERIC(2,1) NOT NULL CHECK (weight >= 1.0 AND weight <= 5.0),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_weight_votes_raw_game ON weight_votes_raw(game_id);
