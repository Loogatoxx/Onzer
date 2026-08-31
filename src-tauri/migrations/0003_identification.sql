-- ════════════════════════════════════════════════════════════════════════════
--  Onzer — Identification par empreinte acoustique (v3)
--
--  Trois colonnes seulement, mais une distinction importante : l'état
--  d'ANALYSE (extraction des features pour la recommandation) et l'état
--  d'IDENTIFICATION (reconnaissance du morceau) sont indépendants.
--
--  L'analyse est purement locale et fonctionne hors ligne. L'identification
--  exige le réseau et une clé d'API. Les mélanger empêcherait la
--  recommandation de fonctionner chez un utilisateur sans connexion.
-- ════════════════════════════════════════════════════════════════════════════

ALTER TABLE tracks ADD COLUMN identification_state TEXT NOT NULL DEFAULT 'pending';

-- Identifiant MusicBrainz de l'ENREGISTREMENT.
--
-- Pas de l'œuvre ni de la chanson : de cet enregistrement précis. C'est ce qui
-- distingue une version album de sa version radio, un live d'un studio.
ALTER TABLE tracks ADD COLUMN recording_mbid TEXT;

ALTER TABLE tracks ADD COLUMN identified_at INTEGER;

-- Index partiel : ne couvre que la file d'attente, donc minuscule.
CREATE INDEX idx_tracks_identification
    ON tracks(identification_state)
 WHERE identification_state = 'pending';

CREATE INDEX idx_tracks_recording_mbid ON tracks(recording_mbid);

INSERT INTO settings (key, value, updated_at) VALUES
    ('acoustid_api_key', 'null', 0);
