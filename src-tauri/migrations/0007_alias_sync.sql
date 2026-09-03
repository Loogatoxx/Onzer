-- ════════════════════════════════════════════════════════════════════════════
--  Ce que l'import nous a appris
--
--  # Le défaut que cette table corrige
--
--  À chaque synchronisation, onze morceaux revenaient dans la liste des
--  manquants. On les téléchargeait, et l'import les reconnaissait aussitôt :
--  « déjà là sous un autre nom ». Le fichier partait à la poubelle, et la fois
--  suivante on recommençait — onze téléchargements pour rien, indéfiniment.
--
--  L'appariement de la fusion regarde le chemin, puis les tags. Ces onze-là
--  échappent aux deux : ce sont les mêmes fichiers, rangés autrement et tagués
--  autrement. Seul l'import sait les reconnaître, parce qu'il lit le contenu.
--
--  Il sait donc quelque chose que la fusion ignore. Cette table est l'endroit
--  où il le lui dit.
--
--  # Pourquoi le chemin distant suffit comme clé
--
--  Deux appareils qui se synchronisent partagent une bibliothèque copiée : un
--  chemin relatif y est aussi distinctif qu'un identifiant. Le jour où un
--  troisième appareil réutiliserait le même chemin pour un autre morceau,
--  l'alias serait faux — et l'import, qui lit le contenu, le corrigerait à la
--  première occasion.
-- ════════════════════════════════════════════════════════════════════════════

CREATE TABLE sync_alias (
    -- Le chemin tel que l'autre appareil le connaît.
    remote_path TEXT    PRIMARY KEY,
    -- Le nôtre, pour le même morceau.
    local_path  TEXT    NOT NULL,
    at          INTEGER NOT NULL
);

CREATE INDEX idx_sync_alias_local ON sync_alias(local_path);
