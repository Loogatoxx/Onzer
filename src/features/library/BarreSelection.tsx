import { useEffect, useRef, useState } from "react";

import { Icon } from "@/components/Icon";
import type { PlaylistSummary } from "@/lib/ipc";
import { useFermeture } from "@/lib/useFermeture";

/**
 * Ce qu'on fait d'une poignée de morceaux.
 *
 * # Pourquoi une barre plutôt qu'un menu
 *
 * Les actions portent sur une sélection qu'on est en train de constituer :
 * elles doivent rester visibles pendant qu'on coche, pas se cacher derrière un
 * appui de plus. Et le compte qu'elle affiche est la seule confirmation qu'on
 * a coché ce qu'on croyait.
 */
export function BarreSelection({
  nombre,
  playlists,
  onEnqueue,
  onPlayNext,
  onAddToPlaylist,
  onLove,
  onAll,
  onClose,
  sortie,
}: {
  nombre: number;
  playlists: PlaylistSummary[];
  onEnqueue: () => void;
  onPlayNext: () => void;
  onAddToPlaylist: (playlistId: number) => void;
  onLove: () => void;
  onAll: () => void;
  onClose: () => void;
  /** Vrai pendant qu'elle redescend : la sélection est déjà vide. */
  sortie: boolean;
}) {
  const [choixPlaylist, setChoixPlaylist] = useState(false);
  const monteChoix = useFermeture(choixPlaylist);
  const ancre = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!choixPlaylist) return;

    const fermer = (event: MouseEvent) => {
      if (!ancre.current?.contains(event.target as Node)) setChoixPlaylist(false);
    };
    document.addEventListener("mousedown", fermer);
    return () => document.removeEventListener("mousedown", fermer);
  }, [choixPlaylist]);

  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-0 z-40 flex justify-center px-3 pb-[calc(env(safe-area-inset-bottom)+5.5rem)] lg:pb-6">
      <div
        className={`${sortie ? "barre-descend" : "barre-monte"} pointer-events-auto flex max-w-full items-center gap-1 overflow-x-auto rounded-full bg-raised px-2 py-1.5 shadow-2xl shadow-black/60`}
      >
        <span className="numerals shrink-0 px-2 text-[13px] font-semibold text-ink">
          {nombre}
        </span>

        <Action name="queue" label="Ajouter à la file" onClick={onEnqueue} />
        <Action name="play" label="Lire ensuite" onClick={onPlayNext} />
        <Action name="heart" label="Ajouter aux favoris" onClick={onLove} />

        <div ref={ancre} className="relative shrink-0">
          <Action
            name="playlist"
            label="Ajouter à une playlist"
            onClick={() => setChoixPlaylist(!choixPlaylist)}
          />

          {monteChoix && (
            <div
              className={`${choixPlaylist ? "animate-surgir" : "animate-disparaitre"} vers-le-haut absolute bottom-11 right-0 max-h-64 w-56 overflow-y-auto rounded-lg border border-line bg-raised py-1 shadow-2xl shadow-black/60`}
            >
              {playlists.length === 0 ? (
                <p className="px-3 py-2 text-[12px] leading-snug text-ink-faint">
                  Aucune playlist. Crée-en une d&apos;abord.
                </p>
              ) : (
                playlists.map((playlist) => (
                  <button
                    key={playlist.id}
                    type="button"
                    onClick={() => {
                      onAddToPlaylist(playlist.id);
                      setChoixPlaylist(false);
                    }}
                    className="flex w-full items-center gap-2.5 px-3 py-2 text-left text-[13px] text-ink-muted transition-colors hover:bg-elevated hover:text-ink"
                  >
                    <Icon name="playlist" size={15} />
                    <span className="truncate">{playlist.name}</span>
                  </button>
                ))
              )}
            </div>
          )}
        </div>

        <span className="mx-1 h-5 w-px shrink-0 bg-line" />

        <Action name="check" label="Tout sélectionner" onClick={onAll} />
        <Action name="close" label="Quitter la sélection" onClick={onClose} />
      </div>
    </div>
  );
}

function Action({
  name,
  label,
  onClick,
}: {
  name: "queue" | "play" | "heart" | "playlist" | "check" | "close";
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      className="pression flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-ink-muted transition-colors hover:bg-elevated hover:text-ink"
    >
      <Icon name={name} size={17} />
    </button>
  );
}
