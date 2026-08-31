-- ════════════════════════════════════════════════════════════════════════════
--  Onzer — Moteur de recommandation (v2)
--
--  Le schéma v1 contenait déjà `track_features`, `track_transitions` et
--  `context_profiles`. Cette migration ajoute ce qui manquait pour que le
--  moteur puisse **apprendre de ses propres erreurs**.
-- ════════════════════════════════════════════════════════════════════════════


-- Bras du bandit contextuel.
--
-- Le moteur ne mélange pas ses stratégies selon des poids fixés à la main : il
-- les met en concurrence. Chaque stratégie porte une loi Beta(α, β) mise à jour
-- selon que le morceau qu'elle a proposé a été écouté ou rejeté.
--
-- α et β démarrent à 1 : c'est la loi uniforme, soit « je ne sais rien ».
-- Après quelques dizaines d'écoutes, les stratégies qui marchent chez CET
-- utilisateur prennent naturellement le dessus.
CREATE TABLE reco_strategies (
    name        TEXT PRIMARY KEY,
    -- Succès + 1. Un succès = morceau écouté au-delà du seuil de rejet.
    alpha       REAL NOT NULL DEFAULT 1.0,
    -- Échecs + 1.
    beta        REAL NOT NULL DEFAULT 1.0,
    -- Nombre de propositions faites, à titre de diagnostic.
    proposals   INTEGER NOT NULL DEFAULT 0,
    updated_at  INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;

INSERT INTO reco_strategies (name) VALUES
    ('similarity'),   -- ressemble à ce que tu écoutes en ce moment
    ('affinity'),     -- tu l'aimes, tout simplement
    ('context'),      -- tu écoutes ça à cette heure-ci
    ('transition'),   -- ça s'enchaîne bien après le morceau précédent
    ('discovery'),    -- tu ne l'as presque jamais écouté
    ('forgotten');    -- tu l'aimais, tu l'as oublié


-- Playlists générées, conservées pour pouvoir mesurer leur qualité.
--
-- `play_events.source_id` pointe ici : c'est ce qui permet de savoir *quelle*
-- radio a produit un morceau écouté ou rejeté.
CREATE TABLE reco_sessions (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    -- « radio », « daily », « forgotten »…
    kind         TEXT    NOT NULL,
    -- Morceau de départ, pour une radio.
    seed_track_id INTEGER REFERENCES tracks(id) ON DELETE SET NULL,
    -- Contexte au moment de la génération, en JSON.
    context      TEXT,
    track_count  INTEGER NOT NULL DEFAULT 0,
    created_at   INTEGER NOT NULL
);

CREATE INDEX idx_reco_sessions_created ON reco_sessions(created_at DESC);


-- Quelle stratégie a proposé quel morceau, dans quelle session.
--
-- Sans cette table, impossible d'attribuer un succès ou un échec à la bonne
-- stratégie : le bandit n'aurait rien pour apprendre.
CREATE TABLE reco_proposals (
    session_id  INTEGER NOT NULL REFERENCES reco_sessions(id) ON DELETE CASCADE,
    track_id    INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    strategy    TEXT    NOT NULL,
    -- Score au moment de la proposition, pour analyser après coup.
    score       REAL    NOT NULL DEFAULT 0,
    -- Renseigné quand le verdict est tombé : 1 écouté, 0 rejeté.
    outcome     INTEGER,
    PRIMARY KEY (session_id, position)
) WITHOUT ROWID;

CREATE INDEX idx_reco_proposals_track    ON reco_proposals(track_id);
CREATE INDEX idx_reco_proposals_strategy ON reco_proposals(strategy);


-- ── Mesure de la qualité du moteur ──────────────────────────────────────────
--
-- « Sans mesure, une recommandation n'est que de l'astrologie. »
--
-- Cette vue compare le taux de complétion des morceaux proposés par l'IA à
-- celui des morceaux choisis à la main ou tirés au hasard. Si la ligne « reco »
-- fait moins bien que « shuffle », le moteur est moins bon que le hasard — et
-- il faut le savoir.
CREATE VIEW reco_quality AS
SELECT
    source,
    COUNT(*)                                              AS plays,
    ROUND(AVG(completion), 3)                             AS avg_completion,
    ROUND(AVG(CASE WHEN end_reason = 'completed' THEN 1.0 ELSE 0.0 END), 3)
                                                          AS completion_rate,
    ROUND(AVG(CASE WHEN end_reason = 'skipped'
                    AND skip_at_ms < 15000 THEN 1.0 ELSE 0.0 END), 3)
                                                          AS early_skip_rate
FROM play_events
GROUP BY source;


-- Efficacité comparée des stratégies, telle qu'observée.
CREATE VIEW reco_strategy_quality AS
SELECT
    s.name,
    s.proposals,
    ROUND(s.alpha / (s.alpha + s.beta), 3)                AS estimated_success_rate,
    COUNT(p.track_id)                                     AS judged,
    ROUND(AVG(CAST(p.outcome AS REAL)), 3)                AS observed_success_rate
FROM reco_strategies s
LEFT JOIN reco_proposals p ON p.strategy = s.name AND p.outcome IS NOT NULL
GROUP BY s.name;
