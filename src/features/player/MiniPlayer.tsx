import { Artwork } from "@/features/library/Artwork";
import { Icon } from "@/components/Icon";
import type { PlaybackSnapshot } from "@/lib/ipc";

/**
 * Le lecteur, réduit à ce qui tient au-dessus des onglets.
 *
 * # Ce qu'on garde, et pourquoi si peu
 *
 * La barre du bureau porte dix commandes : lecture, précédent, suivant,
 * position, volume, aléatoire, répétition, favori, paroles, file. Sur un
 * téléphone, elles deviendraient dix cibles de six millimètres côte à côte.
 *
 * Il reste donc **la pochette, le titre, et lecture/suivant** — ce qu'on fait
 * réellement d'un pouce, en marchant. Le reste se trouve dans l'écran de
 * lecture, qu'on ouvre en touchant la barre : c'est là qu'on a de la place et
 * qu'on regarde vraiment.
 *
 * # La ligne de progression
 *
 * Deux pixels, sans poignée. Elle n'est pas là pour qu'on s'en serve mais pour
 * qu'on sache où l'on en est — se déplacer dans le morceau demande de la
 * précision, donc un grand écran.
 */
export function MiniPlayer({
  state,
  onToggle,
  onNext,
  onOpen,
}: {
  state: PlaybackSnapshot;
  onToggle: () => void;
  onNext: () => void;
  /** Ouvre l'écran de lecture, où tout le reste se trouve. */
  onOpen: () => void;
}) {
  const track = state.current;
  if (track === null) return null;

  const ratio =
    state.durationMs > 0 ? Math.min(1, state.positionMs / state.durationMs) : 0;

  return (
    <div className="shrink-0 border-t border-line bg-elevated">
      <div className="h-[2px] bg-raised">
        <div
          className="h-full bg-accent transition-[width] duration-500"
          style={{ width: `${ratio * 100}%` }}
        />
      </div>

      <div className="flex items-center gap-3 px-3 py-2">
        <button
          type="button"
          onClick={onOpen}
          className="flex min-w-0 flex-1 items-center gap-3 text-left"
        >
          <Artwork hash={track.artworkHash} className="h-11 w-11 shrink-0 rounded-md" />

          <span className="min-w-0 flex-1">
            <span className="block truncate text-[13px] font-medium text-ink">
              {track.title}
            </span>
            <span className="block truncate text-[11px] text-ink-muted">
              {track.artist ?? "Artiste inconnu"}
            </span>
          </span>
        </button>

        <button
          type="button"
          aria-label={state.isPlaying ? "Pause" : "Lire"}
          onClick={onToggle}
          className="flex h-10 w-10 shrink-0 items-center justify-center text-ink"
        >
          <Icon name={state.isPlaying ? "pause" : "play"} size={22} />
        </button>

        <button
          type="button"
          aria-label="Morceau suivant"
          onClick={onNext}
          className="flex h-10 w-10 shrink-0 items-center justify-center text-ink-muted"
        >
          <Icon name="next" size={20} />
        </button>
      </div>
    </div>
  );
}
