import { Artwork } from "@/features/library/Artwork";
import { Icon } from "@/components/Icon";
import { formatDuration, type PlaybackSnapshot } from "@/lib/ipc";

/**
 * L'écran de lecture : ce qu'on regarde quand on écoute.
 *
 * # Pourquoi il ne montre pas les paroles d'emblée
 *
 * Toucher le morceau en cours menait droit au texte. Or on n'ouvre pas toujours
 * un morceau pour le lire : on veut souvent juste **voir ce qui joue** — la
 * pochette en grand, l'album, la durée. Les paroles sont une intention à part,
 * elles méritent leur bouton.
 *
 * # Pourquoi la pochette occupe autant de place
 *
 * C'est la seule image de toute l'application. Le reste est du texte sur du
 * gris ; ici, la couleur vient du disque lui-même. La réduire pour caser des
 * commandes reviendrait à remplacer la seule chose qu'on regarde par des
 * choses qu'on cherche.
 */
export function NowPlayingView({
  state,
  isLoved,
  onToggle,
  onNext,
  onPrevious,
  onSeek,
  onToggleLoved,
  onOpenLyrics,
  onOpenArtist,
  onOpenAlbum,
}: {
  state: PlaybackSnapshot;
  isLoved: boolean;
  onToggle: () => void;
  onNext: () => void;
  onPrevious: () => void;
  onSeek: (positionMs: number) => void;
  onToggleLoved: () => void;
  onOpenLyrics: () => void;
  onOpenArtist: () => void;
  onOpenAlbum: () => void;
}) {
  const track = state.current;

  if (track === null) {
    return (
      <p className="px-6 py-20 text-center text-sm text-ink-muted">
        Lance un morceau pour le voir ici.
      </p>
    );
  }

  const ratio = state.durationMs > 0 ? state.positionMs / state.durationMs : 0;

  return (
    <div className="mx-auto flex w-full max-w-md flex-col px-6 pb-10 pt-6">
      <Artwork
        hash={track.artworkHash}
        className="aspect-square w-full rounded-2xl shadow-2xl shadow-black/50"
      />

      <div className="mt-7">
        <h1 className="display text-[clamp(1.4rem,5vw,2rem)] leading-tight text-ink">
          {track.title}
        </h1>

        <p className="mt-2 flex flex-wrap items-center gap-x-2 text-[15px] text-ink-muted">
          <button
            type="button"
            onClick={onOpenArtist}
            className="transition-colors hover:text-ink hover:underline"
          >
            {track.artist ?? "Artiste inconnu"}
          </button>

          {track.album !== null && (
            <>
              <span className="text-ink-faint">·</span>
              <button
                type="button"
                onClick={onOpenAlbum}
                className="transition-colors hover:text-ink hover:underline"
              >
                {track.album}
              </button>
            </>
          )}
        </p>
      </div>

      {/* ── Position ─────────────────────────────────────────────────── */}
      <div className="mt-7">
        <input
          type="range"
          min={0}
          max={Math.max(1, state.durationMs)}
          value={state.positionMs}
          onChange={(event) => onSeek(Number(event.target.value))}
          aria-label="Position dans le morceau"
          className="h-1 w-full cursor-pointer appearance-none rounded-full bg-raised accent-[var(--color-accent)]"
          style={{
            background: `linear-gradient(to right, var(--color-ink) ${ratio * 100}%, var(--color-raised) ${ratio * 100}%)`,
          }}
        />

        <div className="mt-1.5 flex justify-between text-[11px] text-ink-faint">
          <span className="numerals">{formatDuration(state.positionMs)}</span>
          <span className="numerals">{formatDuration(state.durationMs)}</span>
        </div>
      </div>

      {/* ── Commandes ────────────────────────────────────────────────── */}
      <div className="mt-6 flex items-center justify-center gap-8">
        <button
          type="button"
          aria-label="Morceau précédent"
          onClick={onPrevious}
          className="text-ink-muted transition-colors hover:text-ink"
        >
          <Icon name="previous" size={28} />
        </button>

        <button
          type="button"
          aria-label={state.isPlaying ? "Pause" : "Lire"}
          onClick={onToggle}
          className="flex h-16 w-16 items-center justify-center rounded-full bg-ink text-base transition-transform duration-150 active:scale-95"
        >
          <span className={state.isPlaying ? "" : "translate-x-[2px]"}>
            <Icon name={state.isPlaying ? "pause" : "play"} size={26} />
          </span>
        </button>

        <button
          type="button"
          aria-label="Morceau suivant"
          onClick={onNext}
          className="text-ink-muted transition-colors hover:text-ink"
        >
          <Icon name="next" size={28} />
        </button>
      </div>

      {/* ── Ce qu'on peut vouloir ensuite ────────────────────────────── */}
      <div className="mt-8 flex items-center justify-center gap-3">
        <button
          type="button"
          onClick={onToggleLoved}
          className={`flex items-center gap-2 rounded-full px-4 py-2.5 text-[13px] font-medium transition-colors ${
            isLoved ? "bg-elevated text-accent" : "bg-elevated text-ink-muted hover:text-ink"
          }`}
        >
          <Icon name={isLoved ? "heartFilled" : "heart"} size={16} />
          {isLoved ? "Aimé" : "J'aime"}
        </button>

        <button
          type="button"
          onClick={onOpenLyrics}
          className="flex items-center gap-2 rounded-full bg-elevated px-4 py-2.5 text-[13px] font-medium text-ink-muted transition-colors hover:text-ink"
        >
          <Icon name="lyrics" size={16} />
          Paroles
        </button>
      </div>
    </div>
  );
}
