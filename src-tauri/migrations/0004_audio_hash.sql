-- Empreinte des octets audio seuls, et traçabilité de l'identification.
--
-- ── Le défaut corrigé ───────────────────────────────────────────────────────
--
-- `content_hash` couvre le fichier entier. Or Onzer réécrit les tags après
-- identification acoustique : le fichier change d'octets sans changer de
-- musique. L'empreinte stockée cessait alors de correspondre au fichier
-- d'origine, et un second exemplaire du même téléchargement n'était plus
-- reconnu comme doublon.
--
-- Le dédoublonnage par tags ne rattrapait pas la chute : la ligne en base
-- portait déjà les tags corrigés, l'entrant portait encore les siens. Les deux
-- filets se trouaient en même temps, et chaque passage du dossier de dépôt
-- ajoutait un exemplaire. Trois copies du même fichier ont ainsi été observées.
--
-- `audio_hash` ne couvre que l'audio. Retaguer ne le change pas.

ALTER TABLE tracks ADD COLUMN audio_hash TEXT;

-- Partiel : la colonne reste vide tant que le rattrapage n'est pas passé, et
-- un index sur des NULL ne sert à personne.
CREATE INDEX idx_tracks_audio_hash
    ON tracks(audio_hash)
 WHERE audio_hash IS NOT NULL;


-- ── Traçabilité de l'identification ─────────────────────────────────────────
--
-- Une identification acceptée n'est pas forcément juste. Conserver la confiance
-- de l'empreinte et la raison du verdict permet de revenir sur une décision
-- sans tout ré-interroger — et de montrer à l'utilisateur pourquoi un morceau
-- porte le titre qu'il porte.

ALTER TABLE tracks ADD COLUMN identification_score REAL;
ALTER TABLE tracks ADD COLUMN identification_note TEXT;

-- Ce que le fichier annonçait avant toute réécriture.
--
-- Sans cette mémoire, une identification erronée est irréversible : les tags
-- d'origine sont écrasés dans la base ET dans le fichier, et plus rien ne
-- permet de dire ce qu'était le morceau. C'est ce qui s'est produit sur
-- « Dieu ne ment jamais », devenu « carmen » de Stromae.
ALTER TABLE tracks ADD COLUMN original_title  TEXT;
ALTER TABLE tracks ADD COLUMN original_artist TEXT;
ALTER TABLE tracks ADD COLUMN original_album  TEXT;
