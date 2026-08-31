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

/** Doit correspondre à `commands::library::SCAN_PROGRESS_EVENT`. */
const SCAN_PROGRESS_EVENT = "library://scan-progress";

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
};

/** Millisecondes → « 3:42 ». */
export function formatDuration(milliseconds: number): string {
  const totalSeconds = Math.round(milliseconds / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}
