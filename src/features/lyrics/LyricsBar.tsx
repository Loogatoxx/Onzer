import { useEffect, useState } from "react";

import { Icon } from "@/components/Icon";
import { ipc, type LyricsProgress } from "@/lib/ipc";

/**
 * Récupération des paroles manquantes.
 *
 * # Pourquoi ce bandeau existe
 *
 * Un relevé des fichiers de la bibliothèque a montré que **pas un seul** ne
 * portait de paroles dans ses tags. Les chercher un par un depuis le panneau de
 * lecture serait décourageant : ce bouton fait la bibliothèque entière.
 *
 * # Pourquoi la synchronisation compte à part
 *
 * « A des paroles » et « a des paroles qui défilent » ne sont pas la même
 * question. Une bibliothèque téléchargée par deemix porte **toutes** ses
 * paroles et **aucune** synchronisation : ne compter que la présence du texte
 * faisait déclarer le travail terminé sur 1378 morceaux que la passe n'avait
 * jamais regardés.
 *
 * Le bandeau annonce donc ce qui manque réellement, et la passe va le
 * chercher — mesuré sur quinze morceaux au hasard, LRCLIB a la version
 * horodatée de treize d'entre eux.
 *
 * # Pourquoi ce n'est pas automatique
 *
 * Onzer est un lecteur hors ligne. Interroger un service à chaque import se
 * ferait dans le dos de l'utilisateur ; un bouton, non. Ce qui part se limite à
 * l'artiste, au titre, à l'album et à la durée.
 */
const POLL_MS = 2000;

export function LyricsBar() {
  const [progress, setProgress] = useState<LyricsProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const read = () => {
      void ipc.lyricsProgress().then(setProgress).catch(() => undefined);
    };

    read();
    const timer = setInterval(read, POLL_MS);
    return () => clearInterval(timer);
  }, []);

  if (progress === null || progress.total === 0) {
    return null;
  }

  // Ce qui manque, c'est la synchronisation : un morceau au texte brut n'est
  // pas fini, il est à moitié fait.
  const missing = progress.total - progress.withSynced;
  const ratio = progress.withSynced / progress.total;
  const plain = progress.withLyrics - progress.withSynced;

  // Rien à faire : le bandeau s'efface plutôt que d'afficher une jauge pleine.
  if (missing === 0 && !progress.running) {
    return null;
  }

  return (
    <div className="rounded-xl bg-elevated px-3 py-2">
      <div className="flex items-center gap-2.5">
        <span className={`shrink-0 ${progress.running ? "animate-pulse text-accent" : "text-ink-faint"}`}>
          <Icon name="lyrics" size={16} />
        </span>

        <div className="min-w-0 flex-1">
          <p className="truncate text-xs text-ink">Paroles</p>
          <p className="truncate text-[11px] text-ink-faint">
            {progress.running
              ? `${progress.withSynced}/${progress.total} synchronisées…`
              : plain > 0
                ? `${plain} morceau${plain > 1 ? "x" : ""} sans synchronisation`
                : `${missing} morceau${missing > 1 ? "x" : ""} sans paroles`}
          </p>
        </div>

        <button
          type="button"
          disabled={progress.running}
          onClick={() => {
            setError(null);
            void ipc.fetchMissingLyrics().catch((cause: unknown) => setError(String(cause)));
          }}
          className="shrink-0 rounded-full bg-raised px-3 py-1 text-[11px] font-semibold text-ink-muted transition-colors hover:text-ink disabled:opacity-40"
        >
          {progress.running ? "En cours…" : plain > 0 ? "Synchroniser" : "Récupérer en ligne"}
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
          Onzer interroge LRCLIB avec l&apos;artiste, le titre et la durée, et
          retient la version horodatée quand elle existe. Les paroles trouvées
          sont écrites dans les fichiers : elles ne dépendront plus du réseau.
          Un texte brut déjà présent n&apos;est jamais remplacé par un autre
          texte brut.
        </p>
      )}
    </div>
  );
}
