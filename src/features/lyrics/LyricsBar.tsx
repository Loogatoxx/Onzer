import { useEffect, useState } from "react";

import { Icon } from "@/components/Icon";
import { ipc, type AdoptionReport, type LyricsProgress } from "@/lib/ipc";

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

export function LyricsBar({ onChanged }: { onChanged: () => void }) {
  const [progress, setProgress] = useState<LyricsProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [adoption, setAdoption] = useState<AdoptionReport | null>(null);

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
            void ipc.syncLyrics().catch((cause: unknown) => setError(String(cause)));
          }}
          className="shrink-0 rounded-full bg-raised px-3 py-1 text-[11px] font-semibold text-ink-muted transition-colors hover:text-ink disabled:opacity-40"
        >
          {progress.running ? "En cours…" : "Synchroniser"}
        </button>
      </div>

      {/* Le téléchargeur dépose ses `.lrc` au dépôt, qui n'accepte que
          l'audio : ils y restent seuls jusqu'à ce qu'on les rapproche. */}
      <button
        type="button"
        disabled={progress.running}
        onClick={() => {
          setError(null);
          setAdoption(null);
          void ipc
            .adoptSidecars()
            .then((report) => {
              setAdoption(report);
              if (report.adopted > 0) onChanged();
            })
            .catch((cause: unknown) => setError(String(cause)));
        }}
        className="mt-2 w-full rounded-full bg-raised/60 px-3 py-1 text-[11px] text-ink-faint transition-colors hover:text-ink disabled:opacity-40"
      >
        Rattacher les .lrc du dépôt
      </button>

      {progress.running && (
        <div className="mt-2 h-1 overflow-hidden rounded-full bg-raised">
          <div
            className="h-full rounded-full bg-accent transition-[width] duration-500"
            style={{ width: `${ratio * 100}%` }}
          />
        </div>
      )}

      {error !== null && <p className="mt-2 text-[11px] text-danger">{error}</p>}

      {adoption !== null && (
        <p className="mt-2 text-[11px] leading-relaxed text-ink-muted">
          {adoption.adopted} fichier{adoption.adopted > 1 ? "s" : ""} .lrc
          rattaché{adoption.adopted > 1 ? "s" : ""} à leur morceau
          {adoption.orphans > 0 &&
            `, ${adoption.orphans} sans morceau correspondant — ils restent au dépôt`}
          .
        </p>
      )}

      {!progress.running && (
        <p className="mt-1.5 text-[11px] leading-relaxed text-ink-faint">
          Onzer regarde d&apos;abord dans les fichiers : beaucoup portent déjà
          leur synchronisation dans une zone de tags que peu de lecteurs
          lisent. Ce n&apos;est qu&apos;ensuite, et seulement si la complétion
          en ligne est active, qu&apos;il interroge LRCLIB pour le reste.
        </p>
      )}
    </div>
  );
}
