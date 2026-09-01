-- Notes personnelles sur un morceau.
--
-- ── Pourquoi ce champ existe ────────────────────────────────────────────────
--
-- Une bibliothèque personnelle n'est pas un catalogue : chaque morceau y est
-- arrivé pour une raison, et cette raison ne tient dans aucun tag. « Découvert
-- en Corse », « celui que passait mon frère », « à réécouter au casque » — rien
-- de tout cela n'existe dans MusicBrainz, et c'est précisément ce qu'un
-- catalogue en ligne ne pourra jamais rendre.
--
-- Le champ est libre, court, et ne sert à rien d'autre qu'à être relu. Il
-- n'entre ni dans la recommandation ni dans les statistiques : ce serait
-- transformer un souvenir en donnée.
ALTER TABLE tracks ADD COLUMN note TEXT;

-- Les morceaux annotés se retrouvent : une note qu'on ne peut pas relire
-- n'aurait pas de raison d'être écrite.
CREATE INDEX idx_tracks_note ON tracks(id) WHERE note IS NOT NULL AND note <> '';
