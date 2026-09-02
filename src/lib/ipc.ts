/**
 * Client IPC typé — l'unique porte d'entrée vers le backend Rust.
 *
 * Aucun composant n'appelle `invoke` directement : tout passe par ici, ce qui
 * garantit qu'un changement de contrat côté Rust ne casse qu'un seul fichier.
 *
 * ⚠️ Ces types sont écrits à la main pour l'instant, et doivent rester le
 * miroir exact des structures Rust annotées `#[serde(rename_all = "camelCase")]`.
 * Ils seront générés automatiquement (via `specta`) quand la surface de
 * commandes grandira — voir « Dette technique assumée » dans
 * docs/ARCHITECTURE.md.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Miroir de `commands::system::AppStatus`. */
export interface AppStatus {
  schemaVersion: number;
  databasePath: string;
  /** `null` tant que l'utilisateur n'a pas choisi de dossier. */
  libraryRoot: string | null;
  /** Distingue « non configurée » de « SSD débranché » (ADR-006). */
  libraryOnline: boolean;
  trackCount: number;
  eventCount: number;
}

/** Miroir de `db::repository::TrackSummary`. */
export interface TrackSummary {
  id: number;
  title: string;
  artist: string | null;
  album: string | null;
  year: number | null;
  trackNo: number | null;
  durationMs: number;
  format: string;
  relativePath: string;
  isAvailable: boolean;
  artworkHash: string | null;
  isLoved: boolean;
  /** Date d'ajout à la bibliothèque, en millisecondes Unix. */
  addedAt: number;
  /**
   * Le morceau porte-t-il des paroles ?
   *
   * Le contenu n'est pas renvoyé : sur une liste de trois cents lignes, cela
   * ferait des centaines de kilo-octets pour afficher une pastille.
   */
  hasLyrics: boolean;
}

/** Miroir de `identify::spotdl::PlaylistTrack`. */
export interface PlaylistTrack {
  title: string;
  artists: string[];
  album: string | null;
  durationMs: number;
  url: string;
  /** « Artiste - Titre », prêt pour un téléchargeur externe. */
  query: string;
}

/** Miroir de `commands::system::MediaKeysStatus`. */
export interface MediaKeysStatus {
  working: boolean;
  /** Message du système, quand l'enregistrement a échoué. */
  error: string | null;
}

/** Miroir de `commands::sync::ExportedList`. */
export interface ExportedList {
  path: string;
  count: number;
  /** Boucle `yt-dlp` : ne dépend d'aucun accès à Spotify. */
  command: string;
  /** Variante `spotdl` : tague et récupère la pochette d'elle-même. */
  spotdlCommand: string;
}

/** Miroir de `commands::sync::PlaylistComparison`. */
export interface PlaylistComparison {
  playlistName: string;
  total: number;
  present: number;
  missing: PlaylistTrack[];
  /** Commande `spotdl`, quand son accès à Spotify fonctionne. */
  command: string;
  /** Boucle `yt-dlp`, qui ne dépend d'aucun accès à Spotify. */
  ytdlpCommand: string;
}

/** Miroir de `commands::sync::SpotifyStatus`. */
export interface SpotifyStatus {
  configured: boolean;
  /** Aperçu masqué, du genre `6847••••••`. Le secret ne revient jamais. */
  idHint: string | null;
}

/** Miroir de `commands::library::NearDuplicate`. */
export interface NearDuplicate {
  id: number;
  title: string;
  artist: string | null;
  album: string | null;
  durationMs: number;
  /** Clé de regroupement : les lignes d'un même groupe la partagent. */
  groupKey: string;
  relativePath: string;
  playCount: number;
}

/** Miroir de `commands::artists::ArtistSummary`. */
export interface ArtistSummary {
  id: number;
  name: string;
  trackCount: number;
  coverHash: string | null;
}

/** Miroir de `commands::collection::ArtworkProgress`. */
/**
 * Une fiche proposée par un catalogue, avant décision.
 *
 * `score` vient du tri fait côté Rust : la même règle pour les trois services,
 * sans quoi comparer leurs confiances n'aurait aucun sens.
 */
export interface MetadataCandidate {
  source: string;
  title: string;
  artist: string | null;
  album: string | null;
  year: number | null;
  /** Durée annoncée par le service. `0` quand elle manque. */
  durationMs: number;
  coverUrl: string | null;
  previewUrl: string | null;
  albumRef: string | null;
  score: number;
}

export interface Preferences {
  /** Onzer peut-il proposer de compléter les métadonnées en ligne ? */
  onlineCompletion: boolean;
}

/** Ce que la remise à zéro a fait. */
export interface RebuildReport {
  tracks: number;
  foldersSetAside: number;
  albums: number;
  artists: number;
}

export interface AlbumProgress {
  missing: number;
  running: boolean;
}

export interface ArtworkProgress {
  withArtwork: number;
  total: number;
  running: boolean;
}

/** Miroir de `commands::collection::LyricsProgress`. */
export interface LyricsProgress {
  withLyrics: number;
  total: number;
  /** Vrai tant qu'une récupération en lot tourne. */
  running: boolean;
}

/** Miroir de `commands::categories::Category`. */
export interface Category {
  key: string;
  title: string;
  subtitle: string;
  trackCount: number;
  coverHashes: string[];
}

/** Miroir de `identify::discover::Suggestion`. */
export interface Suggestion {
  name: string;
  /** Identifiant MusicBrainz, pour aller vérifier soi-même. */
  mbid: string;
  /** « Parce que tu écoutes Damso et Népal ». */
  reason: string;
  score: number;
}

/** Miroir de `identify::discover::TrackSuggestion`. */
export interface TrackSuggestion {
  title: string;
  artist: string;
  durationMs: number | null;
  /** « Artiste - Titre », prêt pour un téléchargeur externe. */
  query: string;
}

/** Miroir de `commands::home::HomeMix`. */
export interface HomeMix {
  /** Correspond à `reco::engine::PlaylistKind::as_str`. */
  kind: string;
  /** Renseigné pour un mix d'artiste uniquement. */
  artistId: number | null;
  title: string;
  subtitle: string;
  /** Jusqu'à quatre pochettes, pour une mosaïque. */
  coverHashes: string[];
}

/** Miroir de `commands::home::HomeShelf`. */
export interface HomeShelf {
  title: string;
  mixes: HomeMix[];
}

/** Miroir de `commands::home::Home`. */
export interface Home {
  greeting: string;
  resume: TrackSummary[];
  shelves: HomeShelf[];
}

/** Miroir de `commands::collection::SuspectTrack`. */
export interface SuspectTrack {
  id: number;
  /** Ce qu'Onzer a écrit après identification. */
  title: string;
  artist: string | null;
  /** Ce que le fichier annonçait avant. */
  originalTitle: string;
  originalArtist: string | null;
}

/** Miroir de `db::playlists::PlaylistSummary`. */
export interface PlaylistSummary {
  id: number;
  name: string;
  kind: string;
  trackCount: number;
  /** Image choisie, ou à défaut la pochette du premier morceau. */
  coverHash: string | null;
  /** Une phrase libre, écrite par l'utilisateur. */
  description: string | null;
  updatedAt: number;
}

/** Miroir de `library::lyrics::LyricLine`. */
export interface LyricLine {
  atMs: number;
  text: string;
}

/** Miroir de `library::lyrics::Lyrics`. */
export interface Lyrics {
  /** Lignes horodatées, triées. Vide si les paroles ne sont pas synchronisées. */
  synced: LyricLine[];
  plain: string[];
}

/** Miroir de `db::repository::LibraryCounts`. */
export interface LibraryCounts {
  tracks: number;
  artists: number;
  albums: number;
  unavailable: number;
}

/** Miroir de `library::scanner::ScanProgress`. */
export interface ScanProgress {
  processed: number;
  total: number;
  imported: number;
  duplicates: number;
  failed: number;
  current: string;
}

/** Miroir de `library::scanner::ScanSummary`. */
export interface ScanSummary {
  total: number;
  imported: number;
  duplicates: number;
  failed: number;
  errors: string[];
}

/** Miroir de `audio::queue::QueueItem`. */
export interface QueueItem {
  trackId: number;
  title: string;
  artist: string | null;
  album: string | null;
  durationMs: number;
  relativePath: string;
  artworkHash: string | null;
}

/** Miroir de `audio::queue::RepeatMode`. */
export type RepeatMode = "off" | "all" | "one";

/** Miroir de `audio::tracking::PlaySource`. */
export type PlaySource =
  | "library"
  | "playlist"
  | "radio"
  | "reco"
  | "search"
  | "queue"
  | "shuffle";

/** Miroir de `audio::PlaybackSnapshot`. */
export interface PlaybackSnapshot {
  current: QueueItem | null;
  queue: QueueItem[];
  queueIndex: number | null;
  isPlaying: boolean;
  positionMs: number;
  durationMs: number;
  volume: number;
  repeat: RepeatMode;
  shuffle: boolean;
}

/** Miroir de `commands::playback::PlaybackTick`. */
export interface PlaybackTick {
  positionMs: number;
  isPlaying: boolean;
}

/** Miroir de `reco::bandit::Strategy`. */
export type RecoStrategy =
  | "similarity"
  | "affinity"
  | "context"
  | "transition"
  | "discovery"
  | "forgotten";

/** Miroir de `reco::engine::GeneratedTrack`. */
export interface GeneratedTrack {
  trackId: number;
  strategy: RecoStrategy;
  /** Phrase affichable : pourquoi ce morceau est là. */
  reason: string;
  score: number;
}

/** Miroir de `reco::engine::GeneratedPlaylist`. */
export interface GeneratedPlaylist {
  sessionId: number;
  kind: string;
  title: string;
  subtitle: string;
  tracks: GeneratedTrack[];
  poolSize: number;
  /** Part de la bibliothèque déjà analysée, entre 0 et 1. */
  analyzedRatio: number;
}

/** Miroir de `analysis::worker::AnalysisProgress`. */
export interface AnalysisProgress {
  analyzed: number;
  pending: number;
  failed: number;
  total: number;
}

/** Miroir de `identify::worker::IdentificationProgress`. */
export interface IdentificationProgress {
  identified: number;
  pending: number;
  /** Morceaux absents des bases publiques. Ce n'est pas un échec. */
  notFound: number;
  /** Correspondances trouvées mais refusées par la corroboration. */
  rejected: number;
  failed: number;
  total: number;
}

/** Miroir de `commands::identify::IdentificationStatus`. */
export interface IdentificationStatus {
  /** La clé AcoustID est-elle renseignée ? La clé elle-même ne revient jamais. */
  configured: boolean;
  /** Aperçu masqué, du genre `dp7d••••••` : de quoi repérer un collage tronqué. */
  keyHint: string | null;
  /** Dernière erreur de service — ce qui explique une jauge figée. */
  lastError: string | null;
  progress: IdentificationProgress;
}

// ── Statistiques ──────────────────────────────────────────────────────

/**
 * Miroir de `stats::Period`.
 *
 * Une énumération Rust à données se sérialise en objet étiqueté :
 * `Period::Days(30)` devient `{ days: 30 }`, `Period::AllTime` devient
 * `"allTime"`.
 */
export type StatsPeriod = { days: number } | "allTime";

export interface ArtistStat {
  id: number;
  name: string;
  plays: number;
  listenedMs: number;
}

export interface TrackStat {
  id: number;
  title: string;
  artist: string | null;
  artworkHash: string | null;
  plays: number;
  listenedMs: number;
}

export interface AlbumStat {
  id: number;
  title: string;
  artist: string | null;
  artworkHash: string | null;
  plays: number;
  listenedMs: number;
}

export interface HourSlice {
  hour: number;
  plays: number;
  listenedMs: number;
}

export interface Behaviour {
  totalPlays: number;
  qualifiedPlays: number;
  earlySkipRate: number;
  completionRate: number;
  avgCompletion: number;
  peakHour: number | null;
  weekendShare: number;
  manualShare: number;
}

export interface Totals {
  listenedMs: number;
  distinctTracks: number;
  distinctArtists: number;
  sessions: number;
  longestSessionMs: number;
  discoveries: number;
}

export interface Persona {
  title: string;
  description: string;
}

/** Miroir de `stats::Wrapped`. */
export interface Wrapped {
  periodLabel: string;
  totals: Totals;
  behaviour: Behaviour;
  topArtists: ArtistStat[];
  topTracks: TrackStat[];
  topAlbums: AlbumStat[];
  clock: HourSlice[];
  persona: Persona;
  /** Faux quand l'historique est trop mince pour être parlant. */
  hasEnoughData: boolean;
}

/** Doit correspondre à `commands::library::SCAN_PROGRESS_EVENT`. */
const SCAN_PROGRESS_EVENT = "library://scan-progress";
/** Doit correspondre à `commands::playback::STATE_EVENT`. */
const PLAYBACK_STATE_EVENT = "playback://state";
/** Doit correspondre à `commands::playback::TICK_EVENT`. */
const PLAYBACK_TICK_EVENT = "playback://tick";

export const ipc = {
  appStatus: (): Promise<AppStatus> => invoke<AppStatus>("app_status"),

  mediaKeysStatus: (): Promise<MediaKeysStatus> =>
    invoke<MediaKeysStatus>("media_keys_status"),

  /** Réessaie après une autorisation accordée, sans redémarrer. */
  retryMediaKeys: (): Promise<MediaKeysStatus> =>
    invoke<MediaKeysStatus>("retry_media_keys"),

  setLibraryRoot: (path: string): Promise<void> =>
    invoke<void>("set_library_root", { path }),

  importFolder: (folder: string): Promise<ScanSummary> =>
    invoke<ScanSummary>("import_folder", { folder }),

  listTracks: (limit = 200, offset = 0): Promise<TrackSummary[]> =>
    invoke<TrackSummary[]>("list_tracks", { limit, offset }),

  searchTracks: (query: string): Promise<TrackSummary[]> =>
    invoke<TrackSummary[]>("search_tracks", { query }),

  libraryCounts: (): Promise<LibraryCounts> => invoke<LibraryCounts>("library_counts"),

  /** Retourne le nombre de morceaux devenus indisponibles. */
  refreshAvailability: (): Promise<number> => invoke<number>("refresh_availability"),

  artworkDataUri: (hash: string): Promise<string | null> =>
    invoke<string | null>("artwork_data_uri", { hash }),

  /** S'abonne à la progression d'un import. Retourne la fonction de désabonnement. */
  onScanProgress: (handler: (progress: ScanProgress) => void): Promise<UnlistenFn> =>
    listen<ScanProgress>(SCAN_PROGRESS_EVENT, (event) => handler(event.payload)),

  // ── Lecture ───────────────────────────────────────────────────────────
  // Toutes ces commandes retournent l'instantané mis à jour : l'interface
  // réagit sans attendre le prochain battement du backend.

  playTracks: (
    trackIds: number[],
    startAt = 0,
    source: PlaySource = "library",
  ): Promise<PlaybackSnapshot> =>
    invoke<PlaybackSnapshot>("play_tracks", { trackIds, startAt, source }),

  togglePlayback: (): Promise<PlaybackSnapshot> => invoke<PlaybackSnapshot>("toggle_playback"),
  nextTrack: (): Promise<PlaybackSnapshot> => invoke<PlaybackSnapshot>("next_track"),
  previousTrack: (): Promise<PlaybackSnapshot> => invoke<PlaybackSnapshot>("previous_track"),
  stopPlayback: (): Promise<PlaybackSnapshot> => invoke<PlaybackSnapshot>("stop_playback"),
  playbackState: (): Promise<PlaybackSnapshot> => invoke<PlaybackSnapshot>("playback_state"),

  /** Ajoute à la fin de la file, sans interrompre l'écoute en cours. */
  enqueueTracks: (trackIds: number[]): Promise<PlaybackSnapshot> =>
    invoke<PlaybackSnapshot>("enqueue_tracks", { trackIds }),

  jumpInQueue: (index: number): Promise<PlaybackSnapshot> =>
    invoke<PlaybackSnapshot>("jump_in_queue", { index }),
  seekTo: (positionMs: number): Promise<PlaybackSnapshot> =>
    invoke<PlaybackSnapshot>("seek_to", { positionMs }),
  setVolume: (volume: number): Promise<PlaybackSnapshot> =>
    invoke<PlaybackSnapshot>("set_volume", { volume }),
  setRepeat: (mode: RepeatMode): Promise<PlaybackSnapshot> =>
    invoke<PlaybackSnapshot>("set_repeat", { mode }),
  setShuffle: (shuffle: boolean): Promise<PlaybackSnapshot> =>
    invoke<PlaybackSnapshot>("set_shuffle", { shuffle }),

  /** Changement de morceau ou de file. Peu fréquent, charge complète. */
  onPlaybackState: (handler: (state: PlaybackSnapshot) => void): Promise<UnlistenFn> =>
    listen<PlaybackSnapshot>(PLAYBACK_STATE_EVENT, (event) => handler(event.payload)),

  /** Battement de position, quatre fois par seconde. Charge minimale. */
  onPlaybackTick: (handler: (tick: PlaybackTick) => void): Promise<UnlistenFn> =>
    listen<PlaybackTick>(PLAYBACK_TICK_EVENT, (event) => handler(event.payload)),

  // ── Moteur de recommandation ─────────────────────────────────────────
  // Chaque génération lance la lecture immédiatement : demander une radio
  // puis devoir cliquer sur « lecture » serait une étape de trop.

  startRadio: (seedTrackId: number, length?: number): Promise<GeneratedPlaylist> =>
    invoke<GeneratedPlaylist>("start_radio", { seedTrackId, length }),

  startForNow: (length?: number): Promise<GeneratedPlaylist> =>
    invoke<GeneratedPlaylist>("start_for_now", { length }),

  startForgotten: (length?: number): Promise<GeneratedPlaylist> =>
    invoke<GeneratedPlaylist>("start_forgotten", { length }),

  startLoved: (length?: number): Promise<GeneratedPlaylist> =>
    invoke<GeneratedPlaylist>("start_loved", { length }),

  startNeverPlayed: (length?: number): Promise<GeneratedPlaylist> =>
    invoke<GeneratedPlaylist>("start_never_played", { length }),

  startArtistMix: (artistId: number, length?: number): Promise<GeneratedPlaylist> =>
    invoke<GeneratedPlaylist>("start_artist_mix", { artistId, length }),

  /** Contenu de la page d'accueil : reprise, mix du jour, mix de goût. */
  home: (): Promise<Home> => invoke<Home>("home"),

  /** Catégories d'ambiance, calculées sur les quartiles de la bibliothèque. */
  categories: (): Promise<Category[]> => invoke<Category[]>("categories"),

  categoryTracks: (key: string): Promise<TrackSummary[]> =>
    invoke<TrackSummary[]>("category_tracks", { key }),

  /**
   * Artistes absents de la bibliothèque, suggérés par ListenBrainz.
   *
   * Sur action explicite : seuls des identifiants MusicBrainz d'artistes
   * quittent la machine — pas un titre, pas une écoute.
   */
  discoverArtists: (): Promise<Suggestion[]> =>
    invoke<Suggestion[]>("discover_artists"),

  /** Titres d'artistes déjà aimés, absents de la bibliothèque. */
  discoverTracks: (): Promise<TrackSuggestion[]> =>
    invoke<TrackSuggestion[]>("discover_tracks"),

  listArtists: (): Promise<ArtistSummary[]> =>
    invoke<ArtistSummary[]>("list_artists"),

  artistTracks: (artistId: number): Promise<TrackSummary[]> =>
    invoke<TrackSummary[]>("artist_tracks", { artistId }),

  /** Retire de la bibliothèque. Le fichier sur le disque n'est pas touché. */
  removeTrack: (trackId: number): Promise<void> =>
    invoke<void>("remove_track", { trackId }),

  /** Morceaux homonymes de durée voisine — deux versions, sans doute. */
  nearDuplicates: (): Promise<NearDuplicate[]> =>
    invoke<NearDuplicate[]>("near_duplicates"),

  /** Déclare qu'un groupe n'est pas un doublon. Réversible : c'est un réglage. */
  ignoreDuplicateGroup: (groupKey: string): Promise<void> =>
    invoke<void>("ignore_duplicate_group", { groupKey }),

  /**
   * Compare un fichier produit par `spotdl save` à la bibliothèque.
   *
   * Onzer lit et compare. Il ne télécharge rien.
   */
  comparePlaylistFile: (path: string): Promise<PlaylistComparison> =>
    invoke<PlaylistComparison>("compare_playlist_file", { path }),

  /**
   * Compare une liste collée à la main.
   *
   * Accepte un CSV exporté, un copier-coller « Artiste - Titre », ou le JSON
   * d'un fichier `.spotdl`. Le format est reconnu tout seul.
   */
  comparePlaylistText: (text: string): Promise<PlaylistComparison> =>
    invoke<PlaylistComparison>("compare_playlist_text", { text }),

  /**
   * Écrit une liste de requêtes dans le dossier de dépôt, et rend sa boucle.
   *
   * Sert aux recommandations comme aux manquants d'une playlist : la question
   * posée — « comment je récupère tout ça ? » — est la même.
   */
  exportQueries: (queries: string[], fileName: string): Promise<ExportedList> =>
    invoke<ExportedList>("export_queries", { queries, fileName }),

  /** La commande qui produit le fichier à comparer. */
  playlistSaveCommand: (url: string): Promise<string> =>
    invoke<string>("playlist_save_command", { url }),

  spotifyStatus: (): Promise<SpotifyStatus> => invoke<SpotifyStatus>("spotify_status"),

  setSpotifyCredentials: (clientId: string, clientSecret: string): Promise<void> =>
    invoke<void>("set_spotify_credentials", { clientId, clientSecret }),

  analysisProgress: (): Promise<AnalysisProgress> =>
    invoke<AnalysisProgress>("analysis_progress"),

  reanalyzeLibrary: (): Promise<number> => invoke<number>("reanalyze_library"),

  // ── Identification par empreinte acoustique ──────────────────────────

  identificationStatus: (): Promise<IdentificationStatus> =>
    invoke<IdentificationStatus>("identification_status"),

  setAcoustidKey: (key: string): Promise<void> =>
    invoke<void>("set_acoustid_key", { key }),

  retryIdentifications: (): Promise<number> => invoke<number>("retry_identifications"),

  reidentifyLibrary: (): Promise<number> => invoke<number>("reidentify_library"),

  // ── Statistiques ─────────────────────────────────────────────────────
  // Un seul aller-retour pour toute la page : la découper ferait clignoter
  // les sections, chacune arrivant à son rythme.

  wrapped: (period: StatsPeriod = "allTime", top = 10): Promise<Wrapped> =>
    invoke<Wrapped>("wrapped", { period, top }),

  // ── Collection ───────────────────────────────────────────────────────

  createPlaylist: (name: string): Promise<number> =>
    invoke<number>("create_playlist", { name }),
  listPlaylists: (): Promise<PlaylistSummary[]> =>
    invoke<PlaylistSummary[]>("list_playlists"),
  renamePlaylist: (playlistId: number, name: string): Promise<void> =>
    invoke<void>("rename_playlist", { playlistId, name }),
  deletePlaylist: (playlistId: number): Promise<void> =>
    invoke<void>("delete_playlist", { playlistId }),
  addToPlaylist: (playlistId: number, trackIds: number[]): Promise<number> =>
    invoke<number>("add_to_playlist", { playlistId, trackIds }),
  removeFromPlaylist: (playlistId: number, position: number): Promise<void> =>
    invoke<void>("remove_from_playlist", { playlistId, position }),
  playlistTracks: (playlistId: number): Promise<TrackSummary[]> =>
    invoke<TrackSummary[]>("playlist_tracks", { playlistId }),

  /** Bascule le favori et retourne le nouvel état. */
  toggleLoved: (trackId: number): Promise<boolean> =>
    invoke<boolean>("toggle_loved", { trackId }),
  lovedTracks: (): Promise<TrackSummary[]> => invoke<TrackSummary[]>("loved_tracks"),

  trackLyrics: (trackId: number): Promise<Lyrics> =>
    invoke<Lyrics>("track_lyrics", { trackId }),
  setTrackLyrics: (trackId: number, raw: string): Promise<Lyrics> =>
    invoke<Lyrics>("set_track_lyrics", { trackId, raw }),

  /**
   * Cherche les paroles sur LRCLIB.
   *
   * Sur action explicite uniquement : Onzer reste un lecteur hors ligne, et
   * rien ne part sur le réseau sans que l'utilisateur l'ait demandé.
   */
  fetchLyrics: (trackId: number): Promise<Lyrics> =>
    invoke<Lyrics>("fetch_lyrics", { trackId }),

  /** Lance la récupération en lot. Retourne le nombre de morceaux à traiter. */
  fetchMissingLyrics: (): Promise<number> => invoke<number>("fetch_missing_lyrics"),

  lyricsProgress: (): Promise<LyricsProgress> =>
    invoke<LyricsProgress>("lyrics_progress"),

  artworkProgress: (): Promise<ArtworkProgress> =>
    invoke<ArtworkProgress>("artwork_progress"),

  /**
   * Cherche l'identité d'un morceau dans plusieurs catalogues à la fois.
   *
   * Rien n'est écrit : c'est une proposition. Le réseau n'est sollicité qu'à
   * cet appel, déclenché par un clic.
   */
  metadataCandidates: (trackId: number): Promise<MetadataCandidate[]> =>
    invoke<MetadataCandidate[]>("metadata_candidates", { trackId }),

  /** Applique la fiche retenue : tags du fichier, base et pochette. */
  applyCandidate: (trackId: number, candidate: MetadataCandidate): Promise<void> =>
    invoke<void>("apply_candidate", { trackId, candidate }),

  /**
   * Rapatrie la vignette d'une fiche sous forme de `data:` URI.
   *
   * La politique de sécurité du contenu interdit au webview d'aller chercher
   * une image au-dehors — et c'est très bien ainsi. Les octets passent par le
   * cœur Rust.
   */
  candidatePreview: (url: string): Promise<string | null> =>
    invoke<string | null>("candidate_preview", { url }),

  preferences: (): Promise<Preferences> => invoke<Preferences>("preferences"),

  /**
   * Active ou éteint la complétion des métadonnées en ligne.
   *
   * Éteinte, l'interface se tait **et** le cœur refuse : un réglage qui ne
   * tiendrait qu'à ce que l'on affiche ne serait qu'une décoration.
   */
  setOnlineCompletion: (enabled: boolean): Promise<void> =>
    invoke<void>("set_online_completion", { enabled }),

  /**
   * Vide la bibliothèque pour la reconstruire depuis le dépôt.
   *
   * Les fichiers ne sont pas supprimés : ils sont déplacés dans `_Ancien`, à la
   * racine, rangés comme ils l'étaient. L'historique d'écoute et les playlists
   * survivent.
   */
  rebuildLibrary: (): Promise<RebuildReport> => invoke<RebuildReport>("rebuild_library"),

  /** Combien de morceaux affichent encore un tiret à la place de leur album. */
  missingAlbums: (): Promise<AlbumProgress> => invoke<AlbumProgress>("missing_albums"),

  /** Complète les albums manquants. Retourne le nombre de morceaux à traiter. */
  fillMissingAlbums: (): Promise<number> => invoke<number>("fill_missing_albums"),

  /** Va chercher les pochettes manquantes. Retourne le nombre à traiter. */
  fetchMissingArtwork: (): Promise<number> => invoke<number>("fetch_missing_artwork"),

  /**
   * Corrige à la main un morceau mal identifié.
   *
   * Les paroles sont effacées au passage : elles appartenaient à l'ancien
   * titre, et les garder ferait afficher celles d'un autre morceau.
   */
  setPlaylistCover: (playlistId: number, sourcePath: string): Promise<void> =>
    invoke<void>("set_playlist_cover", { playlistId, sourcePath }),

  clearPlaylistCover: (playlistId: number): Promise<void> =>
    invoke<void>("clear_playlist_cover", { playlistId }),

  setPlaylistDescription: (playlistId: number, description: string): Promise<void> =>
    invoke<void>("set_playlist_description", { playlistId, description }),

  /** Note personnelle attachée à un morceau. Reste en base, jamais dans le fichier. */
  trackNote: (trackId: number): Promise<string | null> =>
    invoke<string | null>("track_note", { trackId }),

  setTrackNote: (trackId: number, note: string): Promise<void> =>
    invoke<void>("set_track_note", { trackId, note }),

  correctTrack: (
    trackId: number,
    title: string,
    artist: string | null,
    album: string | null,
  ): Promise<void> =>
    invoke<void>("correct_track", { trackId, title, artist, album }),

  /** Morceaux dont l'identification contredit les tags d'origine du fichier. */
  suspectTracks: (): Promise<SuspectTrack[]> =>
    invoke<SuspectTrack[]>("suspect_tracks"),

  restoreOriginalTags: (trackId: number): Promise<void> =>
    invoke<void>("restore_original_tags", { trackId }),
};

/** Millisecondes → « 1 240 » minutes, sans décimale. */
export function toMinutes(milliseconds: number): number {
  return Math.round(milliseconds / 60_000);
}

/** Millisecondes → « 4 h 12 ». */
export function formatDurationLong(milliseconds: number): string {
  const minutes = toMinutes(milliseconds);
  const hours = Math.floor(minutes / 60);

  if (hours === 0) return `${minutes} min`;
  return `${hours} h ${(minutes % 60).toString().padStart(2, "0")}`;
}

/**
 * Horodatage → « 12 mars 2025 ».
 *
 * Les dates récentes sont exprimées en relatif : « aujourd'hui » se lit plus
 * vite qu'une date qu'il faut comparer mentalement à celle du jour.
 */
export function formatDate(milliseconds: number): string {
  const days = Math.floor((Date.now() - milliseconds) / 86_400_000);

  if (days <= 0) return "aujourd'hui";
  if (days === 1) return "hier";
  if (days < 7) return `il y a ${days} jours`;

  return new Date(milliseconds).toLocaleDateString("fr-FR", {
    day: "numeric",
    month: "short",
    year: "numeric",
  });
}

/** Millisecondes → « 3:42 ». */
export function formatDuration(milliseconds: number): string {
  const totalSeconds = Math.round(milliseconds / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}
