import { useEffect, useState } from "react";

import { Icon } from "@/components/Icon";
import { ipc, type AlbumProgress } from "@/lib/ipc";

/**
 * Complétion des albums manquants.
 *
 * # Pourquoi le tiret méritait sa propre passe
 *
 * Un morceau sans album affiche un **tiret**. Ce n'est pas une information,
 * c'est un trou : la ligne paraît incomplète sans dire pourquoi, et le morceau
 * n'apparaît nulle part dans la vue par albums. Sur cette bibliothèque, 93
 * lignes étaient dans ce cas.
 *
 * L'identification acoustique ne les répare pas — elle a déjà rendu son
 * verdict, et un verdict ne se rejoue pas. La pochette non plus : elle
 * s'attache à un album, et il n'y en avait pas.
 *
 * # Ce que la passe accepte d'écrire
 *
 * Mesuré sur les 91 morceaux concernés : **56 albums retrouvés** chez Deezer
 * avec un titre et un artiste concordants. Les 35 restants sont pour la
 * plupart des fichiers qui contiennent tout un projet — quatorze, vingt,
 * quarante-sept minutes — que le catalogue ne peut évidemment pas reconnaître
 * comme un morceau. Ceux-là relèvent du choix manuel, morceau par morceau,
 * par « Chercher ailleurs ».
 *
 * Ni le titre ni l'artiste ne sont touchés : ce n'est pas ce qu'on répare ici.
 */
const POLL_MS = 2000;

export function AlbumBar() {
  const [progress, setProgress] = useState<AlbumProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const read = () => {
      void ipc.missingAlbums().then(setProgress).catch(() => undefined);
    };

    read();
    const timer = setInterval(read, POLL_MS);
    return () => clearInterval(timer);
  }, []);

  if (progress === null || (progress.missing === 0 && !progress.running)) {
    return null;
  }

  return (
    <div className="rounded-xl bg-elevated px-3 py-2">
      <div className="flex items-center gap-2.5">
        <span
          className={`shrink-0 ${progress.running ? "animate-pulse text-accent" : "text-ink-faint"}`}
        >
          <Icon name="folder" size={16} />
        </span>

        <div className="min-w-0 flex-1">
          <p className="truncate text-xs text-ink">Albums</p>
          <p className="truncate text-[11px] text-ink-faint">
            {progress.running
              ? "complétion en cours…"
              : `${progress.missing} morceau${progress.missing > 1 ? "x" : ""} sans album`}
          </p>
        </div>

        <button
          type="button"
          disabled={progress.running}
          onClick={() => {
            setError(null);
            void ipc.fillMissingAlbums().catch((cause: unknown) => setError(String(cause)));
          }}
          className="shrink-0 rounded-full bg-raised px-3 py-1 text-[11px] font-semibold text-ink-muted transition-colors hover:text-ink disabled:opacity-40"
        >
          {progress.running ? "En cours…" : "Compléter"}
        </button>
      </div>

      {error !== null && <p className="mt-2 text-[11px] text-danger">{error}</p>}

      {!progress.running && (
        <p className="mt-1.5 text-[11px] leading-relaxed text-ink-faint">
          Onzer cherche l&apos;album chez Deezer et ne l&apos;écrit que si le
          titre et l&apos;artiste concordent. Pour le reste, « Chercher
          ailleurs » dans le menu d&apos;une ligne propose les autres
          catalogues.
        </p>
      )}
    </div>
  );
}
