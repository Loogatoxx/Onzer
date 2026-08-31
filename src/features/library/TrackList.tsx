import { formatDuration, type TrackSummary } from "@/lib/ipc";
import { Artwork } from "./Artwork";

interface TrackListProps {
  tracks: TrackSummary[];
  /** Morceau en cours de lecture, mis en évidence. */
  currentTrackId: number | null;
  isPlaying: boolean;
  /** Lance la lecture à partir de cette position de la liste. */
  onPlay: (index: number) => void;
  /** Lance une radio construite autour de ce morceau. */
  onRadio: (trackId: number) => void;
  /**
   * Raison de présence, quand la liste vient du moteur de recommandation.
   * Une recommandation inexplicable inspire la méfiance.
   */
  reasons?: Map<number, string>;
}

/**
 * Liste des morceaux.
 *
 * Volontairement sobre à ce stade : le travail graphique viendra plus tard.
 * Un morceau indisponible (SSD débranché, fichier déplacé à la main) est
 * grisé et non cliquable, mais **jamais masqué** — il n'a pas disparu (ADR-006).
 */
export function TrackList({
  tracks,
  currentTrackId,
  isPlaying,
  onPlay,
  onRadio,
  reasons,
}: TrackListProps) {
  if (tracks.length === 0) {
    return (
      <p className="py-16 text-center text-sm text-ink-muted">
        Aucun morceau à afficher.
      </p>
    );
  }

  return (
    <ul className="divide-y divide-line">
      {tracks.map((track, index) => {
        const isCurrent = track.id === currentTrackId;
        const reason = reasons?.get(track.id);

        return (
          <li
            key={track.id}
            className={`group flex items-center gap-3 px-4 py-2.5 transition-colors hover:bg-surface ${
              track.isAvailable ? "" : "opacity-40"
            } ${isCurrent ? "bg-surface" : ""}`}
          >
            <button
              type="button"
              disabled={!track.isAvailable}
              onClick={() => onPlay(index)}
              className="flex min-w-0 flex-1 items-center gap-3 text-left disabled:cursor-not-allowed"
            >
              <div className="relative shrink-0">
                <Artwork hash={track.artworkHash} />
                {isCurrent && (
                  <div className="absolute inset-0 flex items-center justify-center rounded bg-base/60">
                    <PlayingIndicator animated={isPlaying} />
                  </div>
                )}
              </div>

              <div className="min-w-0 flex-1">
                <p className={`truncate text-sm ${isCurrent ? "text-accent" : "text-ink"}`}>
                  {track.title}
                </p>
                <p className="truncate text-xs text-ink-muted">
                  {track.artist ?? "Artiste inconnu"}
                  {track.album !== null && (
                    <>
                      <span className="mx-1.5 text-ink-faint">·</span>
                      {track.album}
                      {track.year !== null && ` (${track.year})`}
                    </>
                  )}
                </p>
                {reason !== undefined && (
                  <p className="mt-0.5 truncate text-[11px] text-accent/70">{reason}</p>
                )}
              </div>
            </button>

            {!track.isAvailable && (
              <span className="shrink-0 rounded border border-warn/30 px-1.5 py-0.5 text-[10px] text-warn">
                hors ligne
              </span>
            )}

            {/* Le bouton radio n'apparaît qu'au survol : présent quand on le
                cherche, invisible le reste du temps. */}
            <button
              type="button"
              title={`Radio à partir de « ${track.title} »`}
              aria-label={`Radio à partir de ${track.title}`}
              disabled={!track.isAvailable}
              onClick={() => onRadio(track.id)}
              className="shrink-0 rounded-lg p-1.5 text-ink-faint opacity-0 transition-all hover:bg-elevated hover:text-accent focus:opacity-100 group-hover:opacity-100"
            >
              <svg
                viewBox="0 0 24 24"
                className="h-4 w-4"
                fill="none"
                stroke="currentColor"
                strokeWidth={1.8}
                strokeLinecap="round"
                aria-hidden
              >
                <circle cx="12" cy="12" r="2.5" />
                <path d="M8.5 15.5a5 5 0 0 1 0-7M15.5 8.5a5 5 0 0 1 0 7" />
                <path d="M5.5 18.5a9 9 0 0 1 0-13M18.5 5.5a9 9 0 0 1 0 13" />
              </svg>
            </button>

            <span className="shrink-0 text-xs uppercase text-ink-faint">{track.format}</span>
            <span className="w-12 shrink-0 text-right text-xs tabular-nums text-ink-muted">
              {formatDuration(track.durationMs)}
            </span>
          </li>
        );
      })}
    </ul>
  );
}

/** Trois barres qui s'animent tant que la lecture est en cours. */
function PlayingIndicator({ animated }: { animated: boolean }) {
  return (
    <div className="flex h-3.5 items-end gap-[2px]" aria-hidden>
      {[0, 150, 300].map((delay, index) => (
        <span
          key={delay}
          className="w-[3px] rounded-sm bg-accent"
          style={
            animated
              ? {
                  height: "100%",
                  animation: `onzer-bars 900ms ${delay}ms ease-in-out infinite`,
                }
              : { height: index === 1 ? "60%" : "35%" }
          }
        />
      ))}
    </div>
  );
}
