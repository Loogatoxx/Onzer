-- ════════════════════════════════════════════════════════════════════════════
--  Synchronisation entre deux appareils
--
--  # Pourquoi des horodatages et non un simple « le Mac gagne »
--
--  Deux bibliothèques vivent en parallèle : celle du Mac, celle du téléphone.
--  Un cœur mis en favori dans le métro et une playlist créée au bureau doivent
--  tous deux survivre. Départager exige de savoir **quand** chaque changement a
--  eu lieu — et jusqu'ici rien ne le disait : `is_loved` était un booléen sans
--  mémoire, et une correction de titre ne laissait aucune trace de sa date.
--
--  # Pourquoi les colonnes sont nullables
--
--  Les morceaux existants n'ont pas d'histoire : leur `loved_at` est inconnu,
--  pas « à l'époque ». Mettre une date arbitraire ferait gagner ou perdre
--  arbitrairement des favoris à la première synchronisation. Une valeur absente
--  se traite pour ce qu'elle est : l'autre appareil, s'il a une date, sait
--  quelque chose que nous ignorons.
-- ════════════════════════════════════════════════════════════════════════════

ALTER TABLE tracks ADD COLUMN loved_at  INTEGER;
ALTER TABLE tracks ADD COLUMN edited_at INTEGER;

-- ════════════════════════════════════════════════════════════════════════════
--  Ce qui a été écrasé
--
--  Une fusion qui tranche en silence est une fusion à laquelle on ne peut pas
--  faire confiance : le jour où un favori disparaît, il n'y a rien à consulter
--  et le doute s'étend à tout le reste. Chaque décision où **les deux côtés
--  avaient une valeur différente** laisse donc une ligne ici.
--
--  Ce journal n'est pas un historique complet : les cas sans conflit — un seul
--  côté a changé — n'y figurent pas. Il ne consigne que les arbitrages.
-- ════════════════════════════════════════════════════════════════════════════

CREATE TABLE sync_journal (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    at         INTEGER NOT NULL,
    -- Le nom de l'appareil d'en face, tel qu'il s'est présenté.
    pair       TEXT    NOT NULL,
    -- 'loved' | 'metadata' | 'playlist'
    kind       TEXT    NOT NULL,
    -- De quoi il s'agit : « Adèle Castillon — Rêve », « Playlist : Été ».
    subject    TEXT    NOT NULL,
    -- Ce qui a été remplacé, et ce qui a gagné. Lisible par un humain.
    replaced   TEXT,
    kept       TEXT,
    CHECK (kind IN ('loved', 'metadata', 'playlist'))
);

CREATE INDEX idx_sync_journal_at ON sync_journal(at DESC);

-- L'appareil se nomme lui-même : c'est ce nom que l'autre affichera, et il doit
-- survivre au redémarrage. Rangé dans `settings` avec le reste des préférences.
