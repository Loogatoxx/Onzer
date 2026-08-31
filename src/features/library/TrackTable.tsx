import { useEffect, useRef, useState } from "react";

import { Icon, IconButton } from "@/components/Icon";
import {
  formatDate,
  formatDuration,
  type PlaylistSummary,
  type TrackSummary,
} from "@/lib/ipc";
import { Artwork } from "./Artwork";

/**
 * Gabarit de colonnes, partagé par l'en-tête et les lignes.
 *
 * Il est défini **une seule fois** : deux grilles indépendantes finissent
 * toujours par se désaligner d'un pixel ou deux, et c'est exactement le genre
 * de détail qui fait qu'une interface paraît bricolée.
 *
 * Les colonnes disparaissent par ordre d'importance à mesure que la place
 * manque — la date d'ajout d'abord, l'album ensuite. Le titre, lui, ne
 * disparaît jamais.
 */
const GRID =
  "grid grid-cols-[1.75rem_minmax(0,1fr)_auto] lg:grid-cols-[1.75rem_minmax(0,2fr)_minmax(0,1.4fr)_auto] xl:grid-cols-[1.75rem_minmax(0,2fr)_minmax(0,1.4fr)_7rem_auto] items-center gap-4";

interface TrackTableProps {
  tracks: TrackSummary[];
  currentTrackId: number | null;
  isPlaying: boolean;
  /** Lance la lecture à partir de cette position de la liste. */
  onPlay: (index: number) => void;
  onRadio: (trackId: number) => void;
  onToggleLoved: (trackId: number) => void;
  /**
   * Favoris, tenus par la coquille.
   *
   * `TrackSummary.isLoved` sert à les amorcer, mais ne peut pas rester la
   * source : le même morceau peut être aimé depuis la barre de lecture, et
   * deux copies de la vérité finiraient par diverger sous les yeux de
   * l'utilisateur.
   */
  loved: ReadonlySet<number>;
  /** Playlists proposées dans le menu « Ajouter à ». */
  playlists: PlaylistSummary[];
  onAddToPlaylist: (playlistId: number, trackId: number) => void;
  /** Fourni uniquement dans une playlist : retirer la ligne à cette position. */
  onRemoveAt?: (position: number) => void;
  /**
   * Raison de présence, quand la liste vient du moteur de recommandation.
   * Une recommandation inexplicable inspire la méfiance.
   */
  reasons?: Map<number, string>;
  emptyMessage?: string;
}

export function TrackTable({
  tracks,
  currentTrackId,
  isPlaying,
  onPlay,
  onRadio,
  onToggleLoved,
  loved,
  playlists,
  onAddToPlaylist,
  onRemoveAt,
  reasons,
  emptyMessage = "Rien à afficher ici.",
}: TrackTableProps) {
  if (tracks.length === 0) {
    return (
      <p className="px-6 py-20 text-center text-sm text-ink-muted">{emptyMessage}</p>
    );
  }

  return (
    <div className="px-3 pb-8">
      <div
        className={`${GRID} sticky top-[64px] z-10 mb-1 border-b border-line bg-surface/85 px-3 py-2 text-[11px] font-semibold uppercase tracking-[0.14em] text-ink-faint backdrop-blur`}
      >
        <span className="text-center">#</span>
        <span>Titre</span>
        <span className="hidden lg:block">Album</span>
        <span className="hidden xl:block">Ajouté</span>
        <span className="flex justify-end pr-[4.5rem]">
          <Icon name="clock" size={15} />
        </span>
      </div>

      <ul>
        {tracks.map((track, index) => (
          <TrackRow
            key={`${track.id}-${index}`}
            track={track}
            index={index}
            isCurrent={track.id === currentTrackId}
            isPlaying={isPlaying}
            onPlay={() => onPlay(index)}
            onRadio={() => onRadio(track.id)}
            onToggleLoved={() => onToggleLoved(track.id)}
            isLoved={loved.has(track.id)}
            playlists={playlists}
            onAddToPlaylist={(playlistId) => onAddToPlaylist(playlistId, track.id)}
            {...(onRemoveAt === undefined ? {} : { onRemove: () => onRemoveAt(index) })}
            {...(reasons?.get(track.id) === undefined
              ? {}
              : { reason: reasons.get(track.id) as string })}
          />
        ))}
      </ul>
    </div>
  );
}

interface TrackRowProps {
  track: TrackSummary;
  index: number;
  isCurrent: boolean;
  isPlaying: boolean;
  onPlay: () => void;
  onRadio: () => void;
  onToggleLoved: () => void;
  isLoved: boolean;
  playlists: PlaylistSummary[];
  onAddToPlaylist: (playlistId: number) => void;
  onRemove?: () => void;
  reason?: string;
}

/**
 * Une ligne de morceau.
 *
 * Le numéro se change en bouton de lecture au survol. C'est la convention des
 * lecteurs modernes, et elle a une vraie raison d'être : elle évite un bouton
 * permanent par ligne, qui transformerait la liste en tableau de commandes.
 */
function TrackRow({
  track,
  index,
  isCurrent,
  isPlaying,
  onPlay,
  onRadio,
  onToggleLoved,
  isLoved,
  playlists,
  onAddToPlaylist,
  onRemove,
  reason,
}: TrackRowProps) {
  const unavailable = !track.isAvailable;

  return (
    <li
      onDoubleClick={() => {
        if (!unavailable) onPlay();
      }}
      className={`${GRID} group rounded-md px-3 py-2 transition-colors hover:bg-elevated ${
        isCurrent ? "bg-elevated/60" : ""
      } ${unavailable ? "opacity-40" : ""}`}
    >
      {/* ── Numéro / lecture ─────────────────────────────────────────── */}
      <div className="flex h-9 w-7 items-center justify-center">
        {isCurrent ? (
          <span className="group-hover:hidden">
            <PlayingIndicator animated={isPlaying} />
          </span>
        ) : (
          <span className="numerals text-sm text-ink-faint group-hover:hidden">
            {index + 1}
          </span>
        )}

        <button
          type="button"
          disabled={unavailable}
          aria-label={`Lire ${track.title}`}
          onClick={onPlay}
          className="hidden text-ink group-hover:flex disabled:cursor-not-allowed"
        >
          <Icon name={isCurrent && isPlaying ? "pause" : "play"} size={15} />
        </button>
      </div>

      {/* ── Titre ────────────────────────────────────────────────────── */}
      <div className="flex min-w-0 items-center gap-3">
        <Artwork hash={track.artworkHash} className="h-10 w-10 rounded" />

        <div className="min-w-0">
          <p
            className={`truncate text-[15px] font-medium ${
              isCurrent ? "text-accent" : "text-ink"
            }`}
          >
            {track.title}
          </p>
          <p className="truncate text-[13px] text-ink-muted">
            {track.artist ?? "Artiste inconnu"}
            {unavailable && <span className="ml-2 text-warn">hors ligne</span>}
          </p>
          {reason !== undefined && (
            <p className="mt-0.5 truncate text-[11px] text-ink-faint">{reason}</p>
          )}
        </div>
      </div>

      {/* ── Album ────────────────────────────────────────────────────── */}
      <p className="hidden min-w-0 truncate text-[13px] text-ink-muted lg:block">
        {track.album ?? "—"}
      </p>

      {/* ── Date d'ajout ─────────────────────────────────────────────── */}
      <p className="hidden truncate text-[13px] text-ink-faint xl:block">
        {formatDate(track.addedAt)}
      </p>

      {/* ── Actions ──────────────────────────────────────────────────── */}
      <div className="flex items-center justify-end gap-1">
        <button
          type="button"
          title={isLoved ? "Retirer des favoris" : "Ajouter aux favoris"}
          aria-label={isLoved ? "Retirer des favoris" : "Ajouter aux favoris"}
          onClick={onToggleLoved}
          className={`flex h-8 w-8 items-center justify-center rounded-full transition-all ${
            isLoved
              ? "text-accent"
              : "text-ink-faint opacity-0 hover:text-ink focus:opacity-100 group-hover:opacity-100"
          }`}
        >
          <Icon name={isLoved ? "heartFilled" : "heart"} size={16} />
        </button>

        <span className="numerals w-11 text-right text-[13px] text-ink-muted">
          {formatDuration(track.durationMs)}
        </span>

        <RowMenu
          track={track}
          playlists={playlists}
          onRadio={onRadio}
          onAddToPlaylist={onAddToPlaylist}
          {...(onRemove === undefined ? {} : { onRemove })}
        />
      </div>
    </li>
  );
}

/**
 * Menu contextuel d'une ligne.
 *
 * Il se ferme au clic extérieur **et** à la touche Échap : un menu qu'on ne
 * peut fermer qu'en cliquant précisément à côté est une petite tyrannie.
 */
function RowMenu({
  track,
  playlists,
  onRadio,
  onAddToPlaylist,
  onRemove,
}: {
  track: TrackSummary;
  playlists: PlaylistSummary[];
  onRadio: () => void;
  onAddToPlaylist: (playlistId: number) => void;
  onRemove?: () => void;
}) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;

    const close = (event: MouseEvent) => {
      if (!anchor.current?.contains(event.target as Node)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };

    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", escape);
    };
  }, [open]);

  function choose(action: () => void) {
    action();
    setOpen(false);
  }

  return (
    <div ref={anchor} className="relative">
      <IconButton
        name="more"
        label={`Autres actions pour ${track.title}`}
        active={open}
        size={16}
        onClick={() => setOpen((value) => !value)}
        className={open ? "" : "opacity-0 focus:opacity-100 group-hover:opacity-100"}
      />

      {open && (
        <div className="absolute right-0 top-9 z-30 w-60 overflow-hidden rounded-lg border border-line bg-raised py-1 shadow-2xl shadow-black/60">
          <MenuItem icon="radio" onClick={() => choose(onRadio)}>
            Lancer une radio
          </MenuItem>

          {onRemove !== undefined && (
            <MenuItem icon="trash" onClick={() => choose(onRemove)}>
              Retirer de cette playlist
            </MenuItem>
          )}

          {playlists.length > 0 && (
            <>
              <p className="mt-1 border-t border-line px-3 pb-1 pt-2 text-[11px] font-semibold uppercase tracking-[0.14em] text-ink-faint">
                Ajouter à
              </p>
              <div className="max-h-52 overflow-y-auto">
                {playlists.map((playlist) => (
                  <MenuItem
                    key={playlist.id}
                    icon="plus"
                    onClick={() => choose(() => onAddToPlaylist(playlist.id))}
                  >
                    {playlist.name}
                  </MenuItem>
                ))}
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}

function MenuItem({
  icon,
  onClick,
  children,
}: {
  icon: "radio" | "trash" | "plus";
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex w-full items-center gap-3 px-3 py-2 text-left text-[13px] text-ink-muted transition-colors hover:bg-elevated hover:text-ink"
    >
      <Icon name={icon} size={16} />
      <span className="truncate">{children}</span>
    </button>
  );
}

/** Trois barres qui s'animent tant que la lecture est en cours. */
function PlayingIndicator({ animated }: { animated: boolean }) {
  return (
    <span className="flex h-3.5 items-end gap-[2px]" aria-hidden>
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
    </span>
  );
}
