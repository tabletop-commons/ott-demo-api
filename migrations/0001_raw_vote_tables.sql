-- Raw vote tables for materialization pipeline
-- See: docs/src/pillars/data-model/materialization.md

-- Individual rating votes (raw input → materialized to games.average_rating, etc.)
CREATE TABLE IF NOT EXISTS rating_votes (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    game_id     UUID NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    rating      INTEGER NOT NULL CHECK (rating >= 1 AND rating <= 10),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_rating_votes_game ON rating_votes(game_id);

-- Individual weight votes (raw input → materialized to games.weight, weight_votes)
CREATE TABLE IF NOT EXISTS weight_votes_raw (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    game_id     UUID NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    weight      NUMERIC(2,1) NOT NULL CHECK (weight >= 1.0 AND weight <= 5.0),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_weight_votes_raw_game ON weight_votes_raw(game_id);
