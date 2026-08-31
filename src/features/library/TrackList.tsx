import { formatDuration, type TrackSummary } from "@/lib/ipc";
import { Artwork } from "./Artwork";

/**
 * Liste des morceaux.
 *
 * Volontairement sobre à ce stade : le travail graphique viendra plus tard.
 * Un morceau indisponible (SSD débranché, fichier déplacé à la main) est
 * grisé, jamais masqué — il n'a pas disparu (ADR-006).
 */
export function TrackList({ tracks }: { tracks: TrackSummary[] }) {
  if (tracks.length === 0) {
    return (
      <p className="py-16 text-center text-sm text-ink-muted">
        Aucun morceau à afficher.
      </p>
    );
  }

  return (
    <ul className="divide-y divide-line">
      {tracks.map((track) => (
        <li
          key={track.id}
          className={`flex items-center gap-3 px-4 py-2.5 transition-colors hover:bg-surface ${
            track.isAvailable ? "" : "opacity-40"
          }`}
        >
          <Artwork hash={track.artworkHash} />

          <div className="min-w-0 flex-1">
            <p className="truncate text-sm text-ink">{track.title}</p>
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

          <span className="shrink-0 text-xs uppercase text-ink-faint">
            {track.format}
          </span>
          <span className="w-12 shrink-0 text-right text-xs tabular-nums text-ink-muted">
            {formatDuration(track.durationMs)}
          </span>
        </li>
      ))}
    </ul>
  );
}
