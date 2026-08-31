import { formatDuration, type TrackSummary } from "@/lib/ipc";
import { Artwork } from "./Artwork";

interface TrackListProps {
  tracks: TrackSummary[];
  /** Morceau en cours de lecture, mis en évidence. */
  currentTrackId: number | null;
  isPlaying: boolean;
  /** Lance la lecture à partir de cette position de la liste. */
  onPlay: (index: number) => void;
}

/**
 * Liste des morceaux.
 *
 * Volontairement sobre à ce stade : le travail graphique viendra plus tard.
 * Un morceau indisponible (SSD débranché, fichier déplacé à la main) est
 * grisé et non cliquable, mais **jamais masqué** — il n'a pas disparu (ADR-006).
 */
export function TrackList({ tracks, currentTrackId, isPlaying, onPlay }: TrackListProps) {
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

        return (
          <li key={track.id}>
            <button
              type="button"
              disabled={!track.isAvailable}
              onDoubleClick={() => onPlay(index)}
              onClick={() => onPlay(index)}
              className={`flex w-full items-center gap-3 px-4 py-2.5 text-left transition-colors hover:bg-surface disabled:cursor-not-allowed ${
                track.isAvailable ? "" : "opacity-40"
              } ${isCurrent ? "bg-surface" : ""}`}
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
              </div>

              {!track.isAvailable && (
                <span className="shrink-0 rounded border border-warn/30 px-1.5 py-0.5 text-[10px] text-warn">
                  hors ligne
                </span>
              )}

              <span className="shrink-0 text-xs uppercase text-ink-faint">{track.format}</span>
              <span className="w-12 shrink-0 text-right text-xs tabular-nums text-ink-muted">
                {formatDuration(track.durationMs)}
              </span>
            </button>
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
