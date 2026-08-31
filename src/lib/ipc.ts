/**
 * Client IPC typé — l'unique porte d'entrée vers le backend Rust.
 *
 * Aucun composant ne doit appeler `invoke` directement : tout passe par ici,
 * ce qui garantit qu'un changement de contrat côté Rust ne casse qu'un seul
 * fichier.
 *
 * ⚠️ Ces types sont écrits à la main pour l'instant. Ils seront générés
 * automatiquement depuis Rust (via `specta`) dès que la surface de commandes
 * grandira — voir la note dans docs/ARCHITECTURE.md.
 */

import { invoke } from "@tauri-apps/api/core";

/** Miroir de `commands::system::AppStatus` côté Rust. */
export interface AppStatus {
  /** Version du schéma effectivement appliquée en base. */
  schemaVersion: number;
  databasePath: string;
  /** `null` au tout premier lancement. */
  libraryRoot: string | null;
  /**
   * Distingue « pas encore configurée » de « SSD débranché ».
   * Voir ADR-006.
   */
  libraryOnline: boolean;
  trackCount: number;
  eventCount: number;
}

export const ipc = {
  appStatus: (): Promise<AppStatus> => invoke<AppStatus>("app_status"),
};
