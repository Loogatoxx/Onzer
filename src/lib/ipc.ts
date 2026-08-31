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

/** Doit correspondre à `commands::library::SCAN_PROGRESS_EVENT`. */
const SCAN_PROGRESS_EVENT = "library://scan-progress";
/** Doit correspondre à `commands::playback::STATE_EVENT`. */
const PLAYBACK_STATE_EVENT = "playback://state";
/** Doit correspondre à `commands::playback::TICK_EVENT`. */
const PLAYBACK_TICK_EVENT = "playback://tick";

export const ipc = {
  appStatus: (): Promise<AppStatus> => invoke<AppStatus>("app_status"),

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

  analysisProgress: (): Promise<AnalysisProgress> =>
    invoke<AnalysisProgress>("analysis_progress"),

  reanalyzeLibrary: (): Promise<number> => invoke<number>("reanalyze_library"),
};

/** Millisecondes → « 3:42 ». */
export function formatDuration(milliseconds: number): string {
  const totalSeconds = Math.round(milliseconds / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}
