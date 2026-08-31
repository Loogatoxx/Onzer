-- ════════════════════════════════════════════════════════════════════════════
--  Onzer — Schéma initial (v1)
--
--  Principe fondateur : `play_events` est un JOURNAL IMMUABLE (append-only).
--  Tout le reste — compteurs, scores d'affinité, profils de contexte — en est
--  DÉRIVÉ et donc entièrement recalculable. Changer la formule de scoring de la
--  recommandation ne fait perdre aucune donnée historique.
--
--  Conventions :
--    · Les horodatages sont des entiers, en millisecondes Unix (UTC).
--    · Les booléens sont des entiers 0/1 (SQLite n'a pas de type booléen).
--    · Aucun chemin absolu n'est stocké (ADR-006 : le SSD est amovible).
-- ════════════════════════════════════════════════════════════════════════════


-- ════════════════════════════════════════════════════════════════════════════
--  GROUPE 1 — RÉFÉRENTIEL : la bibliothèque
-- ════════════════════════════════════════════════════════════════════════════

-- Les artistes.
-- `normalized_name` est la clé de dédoublonnage : minuscules, sans accents ni
-- ponctuation. C'est elle qui empêche « A$AP Rocky », « ASAP Rocky » et
-- « asap rocky » de créer trois artistes distincts.
CREATE TABLE artists (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT    NOT NULL,          -- affiché tel quel
    sort_name       TEXT,                      -- « Beatles, The » pour le tri
    normalized_name TEXT    NOT NULL UNIQUE,   -- clé de dédoublonnage
    mbid            TEXT,                      -- MusicBrainz ID, si enrichi
    image_path      TEXT,                      -- relatif à artwork/
    created_at      INTEGER NOT NULL
);


-- Les albums.
-- Un album est identifié par le trio (artiste principal, titre normalisé, année),
-- ce qui permet de distinguer les rééditions et les albums homonymes.
CREATE TABLE albums (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    title            TEXT    NOT NULL,
    normalized_title TEXT    NOT NULL,
    album_artist_id  INTEGER REFERENCES artists(id) ON DELETE SET NULL,
    year             INTEGER,
    release_date     TEXT,                     -- ISO 8601 si connue précisément
    total_tracks     INTEGER,
    total_discs      INTEGER,
    artwork_hash     TEXT,                     -- nom du fichier dans artwork/
    mbid             TEXT,
    created_at       INTEGER NOT NULL,
    UNIQUE (album_artist_id, normalized_title, year)
);

CREATE INDEX idx_albums_artist ON albums(album_artist_id);


-- LA TABLE CENTRALE.
--
-- Deux états d'absence à ne surtout pas confondre :
--   · is_available = 0 → le fichier est temporairement introuvable
--                        (SSD débranché, fichier déplacé à la main).
--                        RIEN n'est perdu, tout revient au rebranchement.
--   · deleted_at != NULL → l'utilisateur a retiré le morceau de sa bibliothèque.
--                          L'historique d'écoute, lui, est CONSERVÉ.
--
-- Un morceau n'est jamais supprimé physiquement de cette table : son historique
-- alimente les statistiques et le moteur de recommandation.
CREATE TABLE tracks (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    title             TEXT    NOT NULL,
    normalized_title  TEXT    NOT NULL,
    album_id          INTEGER REFERENCES albums(id) ON DELETE SET NULL,
    track_no          INTEGER,
    disc_no           INTEGER,
    year              INTEGER,
    duration_ms       INTEGER NOT NULL,

    -- ── Localisation (ADR-006) ────────────────────────────────────────────
    -- Toujours relatif à la racine de bibliothèque, jamais absolu : le point de
    -- montage du SSD externe n'est pas stable.
    relative_path     TEXT    NOT NULL UNIQUE,
    file_size         INTEGER NOT NULL,
    -- BLAKE3 du contenu. Sert à deux choses : détecter les doublons à l'import,
    -- et ré-identifier un fichier que l'utilisateur aurait déplacé ou renommé
    -- lui-même dans le Finder.
    content_hash      TEXT    NOT NULL,
    file_modified_at  INTEGER,

    -- ── Caractéristiques techniques ───────────────────────────────────────
    format            TEXT    NOT NULL,        -- mp3 | flac | m4a | ogg | wav
    bitrate           INTEGER,
    sample_rate       INTEGER,
    channels          INTEGER,
    -- Normalisation du volume : évite le morceau qui explose les oreilles
    -- après un titre mixé bas.
    replaygain_gain   REAL,
    replaygain_peak   REAL,

    -- ── Cycle de vie ──────────────────────────────────────────────────────
    is_available      INTEGER NOT NULL DEFAULT 1,
    last_seen_at      INTEGER,
    added_at          INTEGER NOT NULL,
    deleted_at        INTEGER,
    source            TEXT    NOT NULL DEFAULT 'scan',
    -- File d'attente de l'analyse audio (extraction des features pour la reco).
    analysis_state    TEXT    NOT NULL DEFAULT 'pending',
    analysis_error    TEXT,

    -- ── Appréciation manuelle ─────────────────────────────────────────────
    rating            INTEGER,
    is_loved          INTEGER NOT NULL DEFAULT 0,
    lyrics            TEXT,

    CHECK (rating IS NULL OR rating BETWEEN 0 AND 5),
    CHECK (source         IN ('scan', 'manual', 'auto_import')),
    CHECK (analysis_state IN ('pending', 'running', 'done', 'failed', 'skipped'))
);

CREATE INDEX idx_tracks_album     ON tracks(album_id);
CREATE INDEX idx_tracks_hash      ON tracks(content_hash);
CREATE INDEX idx_tracks_added     ON tracks(added_at DESC);
-- Dédoublonnage par tags : même titre + même durée = doublon probable.
CREATE INDEX idx_tracks_dedupe    ON tracks(normalized_title, duration_ms);
-- Index partiels : ne couvrent que les lignes utiles, donc minuscules et rapides.
CREATE INDEX idx_tracks_available ON tracks(id)             WHERE is_available = 1 AND deleted_at IS NULL;
CREATE INDEX idx_tracks_pending   ON tracks(analysis_state) WHERE analysis_state = 'pending';


-- Relation N-N morceau ↔ artiste, avec un rôle.
-- C'est ce qui permet de ranger « Daft Punk feat. Pharrell » sous Daft Punk
-- tout en gardant Pharrell cherchable et créditée.
CREATE TABLE track_artists (
    track_id  INTEGER NOT NULL REFERENCES tracks(id)  ON DELETE CASCADE,
    artist_id INTEGER NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    role      TEXT    NOT NULL DEFAULT 'main',
    position  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (track_id, artist_id, role),
    CHECK (role IN ('main', 'featuring', 'remixer', 'producer', 'composer'))
) WITHOUT ROWID;

CREATE INDEX idx_track_artists_artist ON track_artists(artist_id);


CREATE TABLE genres (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL,
    normalized_name TEXT NOT NULL UNIQUE
);

CREATE TABLE track_genres (
    track_id INTEGER NOT NULL REFERENCES tracks(id)  ON DELETE CASCADE,
    genre_id INTEGER NOT NULL REFERENCES genres(id)  ON DELETE CASCADE,
    PRIMARY KEY (track_id, genre_id)
) WITHOUT ROWID;

CREATE INDEX idx_track_genres_genre ON track_genres(genre_id);


-- Recherche plein texte instantanée.
-- `remove_diacritics 2` fait que « beyonce » trouve « Beyoncé ».
-- Table maintenue explicitement par la couche applicative (et non par des
-- triggers) : la colonne `artist_names` agrège plusieurs tables, ce qu'un
-- trigger ne saurait pas reconstruire proprement.
CREATE VIRTUAL TABLE tracks_fts USING fts5(
    track_id UNINDEXED,
    title,
    artist_names,
    album_title,
    tokenize = "unicode61 remove_diacritics 2"
);


-- ════════════════════════════════════════════════════════════════════════════
--  GROUPE 2 — LE JOURNAL D'ÉCOUTE : le cerveau de la recommandation
-- ════════════════════════════════════════════════════════════════════════════

-- Une session = une période d'écoute continue (séparée par une inactivité
-- prolongée ou la fermeture de l'application).
-- `detected_context` est rempli a posteriori par le clustering : l'utilisateur
-- n'a jamais à déclarer « je fais du sport », le système le déduit.
CREATE TABLE listening_sessions (
    id                TEXT PRIMARY KEY,        -- UUID
    started_at        INTEGER NOT NULL,
    ended_at          INTEGER,
    track_count       INTEGER NOT NULL DEFAULT 0,
    total_listened_ms INTEGER NOT NULL DEFAULT 0,
    skip_count        INTEGER NOT NULL DEFAULT 0,
    detected_context  TEXT
);

CREATE INDEX idx_sessions_started ON listening_sessions(started_at DESC);


-- ⭐ LE JOURNAL. Une ligne par écoute. Append-only, protégé par trigger.
--
-- Chaque colonne marquée ⚡ ci-dessous est un signal que la plupart des lecteurs
-- ne capturent pas, et sans lequel la recommandation reste grossière.
CREATE TABLE play_events (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    -- RESTRICT et non CASCADE : on refuse de perdre l'historique en supprimant
    -- un morceau. La suppression se fait en douceur via tracks.deleted_at.
    track_id             INTEGER NOT NULL REFERENCES tracks(id) ON DELETE RESTRICT,
    session_id           TEXT    NOT NULL REFERENCES listening_sessions(id) ON DELETE RESTRICT,

    started_at           INTEGER NOT NULL,
    ended_at             INTEGER,
    -- Temps RÉELLEMENT écouté, pauses et sauts déduits. Différent de
    -- (ended_at - started_at), qui inclurait une pause de 3 heures.
    listened_ms          INTEGER NOT NULL DEFAULT 0,
    -- Instantané de la durée au moment de l'écoute : le fichier peut être
    -- remplacé plus tard par une version différente.
    duration_ms          INTEGER NOT NULL,
    completion           REAL    NOT NULL DEFAULT 0,   -- 0.0 → 1.0

    end_reason           TEXT,
    -- ⚡ Position exacte du skip. Skipper à 3 s signifie « je déteste ce son » ;
    --    skipper à 2 min signifie « je l'aime, mais pas maintenant ».
    --    Sans cette colonne, les deux cas sont indiscernables.
    skip_at_ms           INTEGER,
    seek_count           INTEGER NOT NULL DEFAULT 0,
    pause_count          INTEGER NOT NULL DEFAULT 0,

    -- ⚡ D'où vient l'écoute. C'est la SEULE boucle de qualité du moteur :
    --    comparer le taux de skip des titres proposés par l'IA à celui des
    --    titres choisis à la main dit si l'algorithme est bon.
    source               TEXT    NOT NULL DEFAULT 'library',
    source_id            INTEGER,                       -- id de playlist / radio
    -- ⚡ Construit la matrice de transitions : apprend quels ENCHAÎNEMENTS
    --    fonctionnent, pas seulement quels morceaux sont aimés.
    previous_track_id    INTEGER REFERENCES tracks(id) ON DELETE SET NULL,
    -- ⚡ Chercher activement un morceau est un signal d'affinité bien plus fort
    --    que le laisser passer dans une file.
    was_manual_selection INTEGER NOT NULL DEFAULT 0,

    -- ⚡ Casque = écoute attentive. Enceintes = fond sonore. Deux intentions
    --    différentes, donc deux recommandations différentes.
    output_device        TEXT,
    volume               REAL,

    -- Contexte dénormalisé : évite de recalculer un fuseau horaire sur des
    -- centaines de milliers de lignes à chaque requête de statistiques.
    hour_local           INTEGER NOT NULL,              -- 0-23, heure LOCALE
    weekday              INTEGER NOT NULL,              -- 0 = lundi
    is_weekend           INTEGER NOT NULL DEFAULT 0,

    CHECK (end_reason IS NULL OR end_reason IN ('completed', 'skipped', 'stopped', 'replaced', 'error')),
    CHECK (source IN ('library', 'playlist', 'radio', 'reco', 'search', 'queue', 'shuffle')),
    CHECK (hour_local BETWEEN 0 AND 23),
    CHECK (weekday    BETWEEN 0 AND 6)
);

CREATE INDEX idx_events_track_time ON play_events(track_id, started_at DESC);
CREATE INDEX idx_events_time       ON play_events(started_at DESC);
CREATE INDEX idx_events_session    ON play_events(session_id);
CREATE INDEX idx_events_hour       ON play_events(hour_local);
CREATE INDEX idx_events_source     ON play_events(source, started_at DESC);


-- Garde-fou : le journal ne se supprime pas. Une erreur de code ou une requête
-- maladroite ne doit jamais pouvoir effacer des années d'historique.
CREATE TRIGGER trg_play_events_no_delete
BEFORE DELETE ON play_events
BEGIN
    SELECT RAISE(ABORT, 'play_events est un journal append-only : suppression interdite');
END;


-- ════════════════════════════════════════════════════════════════════════════
--  GROUPE 3 — DÉRIVÉS : performance et intelligence
--
--  Tout ce groupe est RECALCULABLE depuis play_events. En cas de doute sur une
--  donnée, on peut le vider et le régénérer intégralement.
-- ════════════════════════════════════════════════════════════════════════════

-- Compteurs pré-agrégés, mis à jour à chaque fin d'écoute.
-- `affinity_score` applique une décroissance temporelle (demi-vie ~30 jours) :
-- un morceau adoré il y a deux ans pèse moins qu'un morceau adoré ce mois-ci.
CREATE TABLE track_stats (
    track_id            INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    play_count          INTEGER NOT NULL DEFAULT 0,
    completed_count     INTEGER NOT NULL DEFAULT 0,
    skip_count          INTEGER NOT NULL DEFAULT 0,
    early_skip_count    INTEGER NOT NULL DEFAULT 0,   -- skips avant 15 s : rejet franc
    total_listened_ms   INTEGER NOT NULL DEFAULT 0,
    avg_completion      REAL    NOT NULL DEFAULT 0,
    first_played_at     INTEGER,
    last_played_at      INTEGER,
    affinity_score      REAL    NOT NULL DEFAULT 0,
    affinity_updated_at INTEGER
);

CREATE INDEX idx_stats_affinity    ON track_stats(affinity_score DESC);
CREATE INDEX idx_stats_last_played ON track_stats(last_played_at DESC);


-- Le vecteur audio de chaque morceau : ce qui permet de dire « ce titre
-- ressemble à celui-là » sans jamais consulter Internet.
-- `analyzer` + `analyzer_version` permettent de savoir exactement quoi
-- réanalyser le jour où l'algorithme d'extraction s'améliore.
CREATE TABLE track_features (
    track_id         INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    embedding        BLOB    NOT NULL,        -- Vec<f32> sérialisé
    embedding_dim    INTEGER NOT NULL,
    -- Features lisibles : exposables dans l'UI et utilisables dans les règles
    -- des playlists intelligentes (« tempo > 120 et énergie élevée »).
    tempo            REAL,
    energy           REAL,
    loudness         REAL,
    danceability     REAL,
    valence          REAL,                    -- positivité perçue
    instrumentalness REAL,
    musical_key      INTEGER,                 -- 0 = Do … 11 = Si
    musical_mode     INTEGER,                 -- 0 = mineur, 1 = majeur
    analyzer         TEXT    NOT NULL,
    analyzer_version INTEGER NOT NULL,
    analyzed_at      INTEGER NOT NULL
);

CREATE INDEX idx_features_version ON track_features(analyzer, analyzer_version);


-- Matrice de co-occurrence : « après A, j'ai écouté B ».
-- `skip_after_count` est le signal négatif correspondant : la transition a été
-- proposée mais rejetée. C'est ce qui fait qu'une radio coule au lieu de sauter
-- du coq à l'âne.
CREATE TABLE track_transitions (
    from_track_id    INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    to_track_id      INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    play_count       INTEGER NOT NULL DEFAULT 0,
    skip_after_count INTEGER NOT NULL DEFAULT 0,
    last_at          INTEGER,
    PRIMARY KEY (from_track_id, to_track_id)
) WITHOUT ROWID;

CREATE INDEX idx_transitions_to ON track_transitions(to_track_id);


-- Les « moods » détectés automatiquement par clustering sur
-- (heure, jour, tempo, énergie). L'utilisateur ne déclare rien.
CREATE TABLE context_profiles (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    label          TEXT    NOT NULL,
    is_auto        INTEGER NOT NULL DEFAULT 1,   -- 0 si renommé à la main
    centroid       BLOB,
    hour_histogram BLOB,
    event_count    INTEGER NOT NULL DEFAULT 0,
    updated_at     INTEGER NOT NULL
);


-- ════════════════════════════════════════════════════════════════════════════
--  GROUPE 4 — PLAYLISTS
-- ════════════════════════════════════════════════════════════════════════════

-- `kind` distingue trois natures :
--   manual    → l'utilisateur y ajoute ce qu'il veut
--   smart     → définie par des règles (rules_json), recalculée à la volée
--   generated → produite par le moteur de recommandation
CREATE TABLE playlists (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT    NOT NULL,
    description TEXT,
    kind        TEXT    NOT NULL DEFAULT 'manual',
    rules_json  TEXT,
    cover_path  TEXT,
    is_pinned   INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    CHECK (kind IN ('manual', 'smart', 'generated'))
);

-- La clé primaire (playlist, position) garantit l'unicité de l'ordre.
-- Un même morceau peut apparaître plusieurs fois dans une playlist.
CREATE TABLE playlist_tracks (
    playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    track_id    INTEGER NOT NULL REFERENCES tracks(id)    ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    added_at    INTEGER NOT NULL,
    PRIMARY KEY (playlist_id, position)
) WITHOUT ROWID;

CREATE INDEX idx_playlist_tracks_track ON playlist_tracks(track_id);


-- ════════════════════════════════════════════════════════════════════════════
--  GROUPE 5 — SYSTÈME
-- ════════════════════════════════════════════════════════════════════════════

CREATE TABLE settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,          -- JSON
    updated_at INTEGER NOT NULL
) WITHOUT ROWID;


-- Journal des imports. `source_path` conserve l'emplacement d'ORIGINE du
-- fichier : l'import déplaçant les fichiers (ADR-007), c'est ce qui rend
-- l'opération annulable.
CREATE TABLE import_jobs (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    source_path      TEXT    NOT NULL,
    destination_path TEXT,
    origin           TEXT    NOT NULL DEFAULT 'inbox',
    state            TEXT    NOT NULL DEFAULT 'pending',
    track_id         INTEGER REFERENCES tracks(id) ON DELETE SET NULL,
    -- Indices de métadonnées fournis par le script externe (JSON), utilisés
    -- quand les tags du fichier sont absents ou faux.
    metadata_hint    TEXT,
    error            TEXT,
    created_at       INTEGER NOT NULL,
    completed_at     INTEGER,
    CHECK (origin IN ('inbox', 'api', 'manual')),
    CHECK (state  IN ('pending', 'running', 'done', 'duplicate', 'failed', 'reverted'))
);

CREATE INDEX idx_import_jobs_state ON import_jobs(state, created_at DESC);


-- ── Amorce ──────────────────────────────────────────────────────────────────
INSERT INTO settings (key, value, updated_at) VALUES
    ('library_root',      'null',            0),   -- défini à la configuration
    ('library_volume',    'null',            0),   -- nom du volume, pour diagnostic
    ('analyzer_version',  '1',               0),
    ('created_at',        '0',               0);
