import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import { ipc } from "@/lib/ipc";

/**
 * Premier lancement : choix de la racine de bibliothèque.
 *
 * Le dossier proposé par défaut pointe vers le SSD externe, conformément à la
 * politique de stockage (ADR-009) : le volumineux ne va jamais sur le disque
 * interne.
 */
const DEFAULT_ROOT = "/Volumes/Lexar/Musique";

export function SetupView({ onReady }: { onReady: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function choose(path: string | null) {
    if (path === null) return;

    setBusy(true);
    setError(null);
    try {
      await ipc.setLibraryRoot(path);
      onReady();
    } catch (cause) {
      setError(String(cause));
      setBusy(false);
    }
  }

  async function browse() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Choisir le dossier de la bibliothèque",
    });
    await choose(typeof selected === "string" ? selected : null);
  }

  return (
    <div className="mx-auto w-full max-w-md text-center">
      <h2 className="text-xl font-semibold text-ink">Où ranger ta musique ?</h2>
      <p className="mt-2 text-sm leading-relaxed text-ink-muted">
        Onzer déplacera tes fichiers dans ce dossier et les rangera par
        artiste, année et album. Choisis un emplacement sur ton SSD.
      </p>

      <div className="mt-7 space-y-2.5">
        <button
          type="button"
          disabled={busy}
          onClick={() => void choose(DEFAULT_ROOT)}
          className="w-full rounded-xl bg-gradient-to-br from-accent to-accent-alt px-4 py-3 text-sm font-medium text-base transition-opacity hover:opacity-90 disabled:opacity-40"
        >
          Utiliser {DEFAULT_ROOT}
        </button>

        <button
          type="button"
          disabled={busy}
          onClick={() => void browse()}
          className="w-full rounded-xl border border-line bg-surface px-4 py-3 text-sm text-ink transition-colors hover:bg-elevated disabled:opacity-40"
        >
          Choisir un autre dossier…
        </button>
      </div>

      {error !== null && (
        <p className="mt-4 rounded-lg border border-danger/25 bg-danger/5 px-3 py-2 text-xs text-danger">
          {error}
        </p>
      )}

      <p className="mt-6 text-xs leading-relaxed text-ink-faint">
        Le dossier est créé s'il n'existe pas encore. Seuls les fichiers audio
        y sont déplacés — ta base de données et tes statistiques restent sur le
        disque interne.
      </p>
    </div>
  );
}
