-- Seed expansion property modifications and one explicit combination
-- for Spirit Island. This gives the effective-mode three-tier resolution
-- real data to work with.
--
-- Property modifications (Tier 2: individual expansion deltas)
-- These are summed when no explicit ExpansionCombination exists.

-- Branch & Claw: no player count change, slightly higher weight, longer playtime
INSERT INTO property_modifications (expansion_id, base_game_id, max_players_delta, weight_delta, playtime_min_delta, playtime_max_delta, min_age_delta)
VALUES (
    '01912f4c-8a2b-7c3d-9e4f-0a1b2c3d4e5f',  -- Branch & Claw
    '01912f4c-7e3a-7b1a-8c5d-9f0e1a2b3c4d',  -- Spirit Island base
    0, 0.15, 10, 20, 0
);

-- Jagged Earth: adds 5-6 player support, higher weight, longer playtime
INSERT INTO property_modifications (expansion_id, base_game_id, max_players_delta, weight_delta, playtime_min_delta, playtime_max_delta, min_age_delta)
VALUES (
    '01912f4c-9b3c-7d4e-af50-1b2c3d4e5f60',  -- Jagged Earth
    '01912f4c-7e3a-7b1a-8c5d-9f0e1a2b3c4d',  -- Spirit Island base
    2, 0.26, 15, 40, 0
);

-- Nature Incarnate: adds 5-6 player support, higher weight
INSERT INTO property_modifications (expansion_id, base_game_id, max_players_delta, weight_delta, playtime_min_delta, playtime_max_delta, min_age_delta)
VALUES (
    '01912f4c-ac4d-7e5f-b061-2c3d4e5f6071',  -- Nature Incarnate
    '01912f4c-7e3a-7b1a-8c5d-9f0e1a2b3c4d',  -- Spirit Island base
    2, 0.32, 10, 30, 0
);

-- Explicit ExpansionCombination (Tier 1: community-curated)
-- Branch & Claw + Jagged Earth together
-- The combination has emergent effects different from summing individual deltas
INSERT INTO expansion_combinations (id, base_game_id, expansion_ids, source,
    effective_min_players, effective_max_players, effective_weight,
    effective_playtime_min, effective_playtime_max, effective_min_age)
VALUES (
    'a0000001-0000-0000-0000-000000000001',
    '01912f4c-7e3a-7b1a-8c5d-9f0e1a2b3c4d',  -- Spirit Island base
    ARRAY['01912f4c-8a2b-7c3d-9e4f-0a1b2c3d4e5f', '01912f4c-9b3c-7d4e-af50-1b2c3d4e5f60']::UUID[],
    'community_poll',
    1, 6, 4.07, 105, 180, 13
);
