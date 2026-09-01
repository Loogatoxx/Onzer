import { useEffect, useState } from "react";

import { Icon } from "@/components/Icon";
import { ipc, type ArtworkProgress } from "@/lib/ipc";

/**
 * Récupération des pochettes manquantes.
 *
 * # Pourquoi une passe séparée de l'identification
 *
 * L'ouvrier d'identification part de l'**empreinte acoustique**, et son verdict
 * est définitif : un morceau marqué introuvable ne sera jamais repris. Or une
 * pochette absente n'a souvent rien à voir avec une empreinte introuvable — le
 * morceau peut être parfaitement identifié et son album simplement dépourvu
 * d'image, ou n'avoir jamais eu d'album du tout.
 *
 * Cette passe part donc du **texte** : artiste et titre, cherchés dans
 * MusicBrainz, puis la pochette de la parution retenue. Elle n'écrit que
 * l'image — jamais le titre ni l'artiste, qui ne sont pas ce qu'on répare ici.
 */
const POLL_MS = 2000;

export function ArtworkBar() {
  const [progress, setProgress] = useState<ArtworkProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const read = () => {
      void ipc.artworkProgress().then(setProgress).catch(() => undefined);
    };

    read();
    const timer = setInterval(read, POLL_MS);
    return () => clearInterval(timer);
  }, []);

  if (progress === null || progress.total === 0) {
    return null;
  }

  const missing = progress.total - progress.withArtwork;
  const ratio = progress.withArtwork / progress.total;

  if (missing === 0 && !progress.running) {
    return null;
  }

  return (
    <div className="rounded-xl bg-elevated px-3 py-2">
      <div className="flex items-center gap-2.5">
        <span
          className={`shrink-0 ${progress.running ? "animate-pulse text-accent" : "text-ink-faint"}`}
        >
          <Icon name="library" size={16} />
        </span>

        <div className="min-w-0 flex-1">
          <p className="truncate text-xs text-ink">Pochettes</p>
          <p className="truncate text-[11px] text-ink-faint">
            {progress.running
              ? `${progress.withArtwork}/${progress.total} récupérées…`
              : `${missing} morceau${missing > 1 ? "x" : ""} sans pochette`}
          </p>
        </div>

        <button
          type="button"
          disabled={progress.running}
          onClick={() => {
            setError(null);
            void ipc.fetchMissingArtwork().catch((cause: unknown) => setError(String(cause)));
          }}
          className="shrink-0 rounded-full bg-raised px-3 py-1 text-[11px] font-semibold text-ink-muted transition-colors hover:text-ink disabled:opacity-40"
        >
          {progress.running ? "En cours…" : "Récupérer en ligne"}
        </button>
      </div>

      {progress.running && (
        <div className="mt-2 h-1 overflow-hidden rounded-full bg-raised">
          <div
            className="h-full rounded-full bg-accent transition-[width] duration-500"
            style={{ width: `${ratio * 100}%` }}
          />
        </div>
      )}

      {error !== null && <p className="mt-2 text-[11px] text-danger">{error}</p>}

      {!progress.running && (
        <p className="mt-1.5 text-[11px] leading-relaxed text-ink-faint">
          Onzer cherche l'artiste et le titre dans MusicBrainz, puis récupère la
          pochette de sa parution. Rien d'autre n'est modifié.
        </p>
      )}
    </div>
  );
}
