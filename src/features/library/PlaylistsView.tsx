import { Icon } from "@/components/Icon";
import type { PlaylistSummary } from "@/lib/ipc";

/**
 * Les playlists, en liste.
 *
 * # Pourquoi une liste et non une grille
 *
 * Une playlist n'a pas de pochette qui la désigne — celle qu'Onzer affiche est
 * empruntée à son premier morceau, ou choisie à la main. C'est son **nom** qui
 * l'identifie, et un nom se lit mieux sur une ligne que sous une vignette.
 */
export function PlaylistsView({
  playlists,
  onOpen,
  onCreate,
}: {
  playlists: PlaylistSummary[];
  onOpen: (playlist: PlaylistSummary) => void;
  onCreate: () => void;
}) {
  return (
    <div className="px-6 pb-10">
      <div className="flex flex-wrap items-end justify-between gap-4 pt-2">
        <div>
          <p className="text-xs font-semibold uppercase tracking-[0.16em] text-ink-muted">
            Ta bibliothèque
          </p>
          <h1 className="display-xl mt-1 text-[clamp(1.75rem,5vw,2.75rem)] text-ink">
            {playlists.length === 0
              ? "Aucune playlist"
              : `${playlists.length} playlist${playlists.length > 1 ? "s" : ""}`}
          </h1>
        </div>

        <button
          type="button"
          onClick={onCreate}
          className="flex items-center gap-2 rounded-full bg-elevated px-4 py-2 text-[13px] font-semibold text-ink-muted transition-colors hover:text-ink"
        >
          <Icon name="plus" size={16} />
          Nouvelle playlist
        </button>
      </div>

      {playlists.length === 0 ? (
        <p className="mt-8 max-w-md text-[13px] leading-relaxed text-ink-faint">
          Une playlist se crée d&apos;un clic sur le bouton ci-dessus, ou depuis
          le menu d&apos;un morceau — c&apos;est souvent là que l&apos;envie
          vient.
        </p>
      ) : (
        <ul className="mt-6">
          {playlists.map((playlist) => (
            <li key={playlist.id}>
              <button
                type="button"
                onClick={() => onOpen(playlist)}
                className="flex w-full items-center gap-4 rounded-xl px-3 py-2.5 text-left transition-colors hover:bg-surface"
              >
                <span className="flex h-12 w-12 shrink-0 items-center justify-center rounded-lg bg-elevated text-ink-faint">
                  <Icon name="playlist" size={18} />
                </span>

                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[15px] font-medium text-ink">
                    {playlist.name}
                  </span>
                  <span className="numerals block text-[12px] text-ink-faint">
                    {playlist.trackCount} titre{playlist.trackCount > 1 ? "s" : ""}
                  </span>
                </span>

                <span className="shrink-0 text-ink-faint">
                  <Icon name="chevronRight" size={16} />
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
