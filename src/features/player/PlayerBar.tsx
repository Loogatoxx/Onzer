import { useEffect, useState } from "react";

import { Artwork } from "@/features/library/Artwork";
import { formatDuration, type PlaybackSnapshot, type RepeatMode } from "@/lib/ipc";

interface PlayerBarProps {
  state: PlaybackSnapshot;
  onToggle: () => void;
  onNext: () => void;
  onPrevious: () => void;
  onSeek: (positionMs: number) => void;
  onVolume: (volume: number) => void;
  onShuffle: (shuffle: boolean) => void;
  onRepeat: () => void;
}

/**
 * Barre de lecture.
 *
 * Volontairement sobre : le travail graphique viendra plus tard. Elle est
 * néanmoins complète fonctionnellement — lecture, navigation, déplacement dans
 * le morceau, volume, aléatoire et répétition.
 */
export function PlayerBar(props: PlayerBarProps) {
  const { state } = props;
  const track = state.current;

  if (track === null) {
    return null;
  }

  return (
    <div className="shrink-0 border-t border-line bg-surface">
      <Seekbar
        positionMs={state.positionMs}
        durationMs={state.durationMs || track.durationMs}
        onSeek={props.onSeek}
      />

      {/* Disposition : commandes à gauche, morceau à droite.
          La main part naturellement vers la gauche pour agir, et l'œil vers la
          droite pour lire — l'information suit le sens de lecture. */}
      <div className="flex items-center gap-4 px-4 py-2.5">
        <div className="flex shrink-0 items-center gap-1">
          <IconButton
            label={state.shuffle ? "Aléatoire : activé" : "Aléatoire : désactivé"}
            active={state.shuffle}
            onClick={() => props.onShuffle(!state.shuffle)}
          >
            <path d="M16 3h5v5M4 20 21 3M21 16v5h-5M15 15l6 6M4 4l5 5" />
          </IconButton>

          <IconButton label="Précédent" onClick={props.onPrevious}>
            <path d="M19 20 9 12l10-8v16ZM5 19V5" />
          </IconButton>

          <button
            type="button"
            aria-label={state.isPlaying ? "Pause" : "Lecture"}
            onClick={props.onToggle}
            className="mx-1 flex h-9 w-9 items-center justify-center rounded-full bg-ink text-base transition-transform hover:scale-105"
          >
            <svg viewBox="0 0 24 24" className="h-4 w-4" fill="currentColor" aria-hidden>
              {state.isPlaying ? (
                <path d="M6 4h4v16H6zM14 4h4v16h-4z" />
              ) : (
                <path d="M6 3l14 9-14 9V3z" />
              )}
            </svg>
          </button>

          <IconButton label="Suivant" onClick={props.onNext}>
            <path d="m5 4 10 8-10 8V4ZM19 5v14" />
          </IconButton>

          <RepeatButton mode={state.repeat} onClick={props.onRepeat} />
        </div>

        <div className="flex w-28 shrink-0 items-center gap-2">
          <svg
            viewBox="0 0 24 24"
            className="h-3.5 w-3.5 shrink-0 text-ink-faint"
            fill="currentColor"
            aria-hidden
          >
            <path d="M11 5 6 9H2v6h4l5 4V5z" />
          </svg>
          <input
            type="range"
            min={0}
            max={1}
            step={0.01}
            value={state.volume}
            aria-label="Volume"
            onChange={(event) => props.onVolume(Number(event.target.value))}
            className="h-1 w-full cursor-pointer appearance-none rounded-full bg-elevated accent-accent"
          />
        </div>

        {/* Le morceau en cours occupe l'espace restant, aligné à droite. */}
        <div className="flex min-w-0 flex-1 items-center justify-end gap-3">
          <div className="min-w-0 text-right">
            <p className="truncate text-sm text-ink">{track.title}</p>
            <p className="truncate text-xs text-ink-muted">
              {track.artist ?? "Artiste inconnu"}
            </p>
          </div>
          <Artwork hash={track.artworkHash} />
        </div>
      </div>
    </div>
  );
}

/**
 * Tolérance de confirmation d'un déplacement.
 *
 * Le backend publie sa position quatre fois par seconde. Tant que la position
 * reçue n'est pas revenue près de la cible, on considère que le déplacement
 * n'est pas encore confirmé.
 */
const SEEK_CONFIRM_TOLERANCE_MS = 700;

/** Au-delà, on rend la main : un format qui refuse le déplacement ne
 *  confirmerait jamais, et le curseur resterait bloqué sur la cible. */
const SEEK_CONFIRM_TIMEOUT_MS = 2000;

/**
 * Barre de progression cliquable.
 *
 * Deux protections contre le clignotement, pour des raisons différentes :
 *
 * * pendant le glissement, l'affichage suit le doigt et **ignore les battements
 *   du backend**, sans quoi la poignée sauterait en arrière à chaque
 *   rafraîchissement ;
 * * après le relâchement, la **cible est maintenue** jusqu'à ce que le backend
 *   confirme. Le thread audio traite le déplacement de façon asynchrone : sans
 *   ce maintien, le curseur reviendrait visiblement à l'ancienne position avant
 *   de sauter à la bonne.
 */
function Seekbar({
  positionMs,
  durationMs,
  onSeek,
}: {
  positionMs: number;
  durationMs: number;
  onSeek: (positionMs: number) => void;
}) {
  const [dragging, setDragging] = useState<number | null>(null);
  const [pending, setPending] = useState<number | null>(null);

  const shown = dragging ?? pending ?? positionMs;
  const ratio = durationMs > 0 ? Math.min(1, Math.max(0, shown / durationMs)) : 0;

  // Le glissement se termine où qu'aille la souris, y compris hors de la barre.
  useEffect(() => {
    if (dragging === null) return;

    const stop = () => {
      onSeek(dragging);
      setPending(dragging);
      setDragging(null);
    };

    window.addEventListener("pointerup", stop, { once: true });
    return () => window.removeEventListener("pointerup", stop);
  }, [dragging, onSeek]);

  // Le backend a rejoint la cible : on lui rend la main.
  useEffect(() => {
    if (pending !== null && Math.abs(positionMs - pending) < SEEK_CONFIRM_TOLERANCE_MS) {
      setPending(null);
    }
  }, [positionMs, pending]);

  // Filet de sécurité si la confirmation n'arrive jamais.
  useEffect(() => {
    if (pending === null) return;
    const timer = setTimeout(() => setPending(null), SEEK_CONFIRM_TIMEOUT_MS);
    return () => clearTimeout(timer);
  }, [pending]);

  function positionFromEvent(event: React.PointerEvent<HTMLDivElement>): number {
    const bounds = event.currentTarget.getBoundingClientRect();
    const fraction = (event.clientX - bounds.left) / bounds.width;
    return Math.round(Math.min(1, Math.max(0, fraction)) * durationMs);
  }

  return (
    <div className="flex items-center gap-2.5 px-4 pt-2">
      <span className="w-9 shrink-0 text-right text-[11px] tabular-nums text-ink-faint">
        {formatDuration(shown)}
      </span>

      <div
        role="slider"
        aria-label="Position dans le morceau"
        aria-valuemin={0}
        aria-valuemax={durationMs}
        aria-valuenow={shown}
        tabIndex={0}
        onPointerDown={(event) => setDragging(positionFromEvent(event))}
        onPointerMove={(event) => {
          if (dragging !== null) setDragging(positionFromEvent(event));
        }}
        className="group h-3 flex-1 cursor-pointer py-1"
      >
        <div className="h-1 overflow-hidden rounded-full bg-elevated">
          <div
            className="h-full rounded-full bg-gradient-to-r from-accent to-accent-alt"
            style={{ width: `${ratio * 100}%` }}
          />
        </div>
      </div>

      <span className="w-9 shrink-0 text-[11px] tabular-nums text-ink-faint">
        {formatDuration(durationMs)}
      </span>
    </div>
  );
}

const REPEAT_LABELS: Record<RepeatMode, string> = {
  off: "Répétition : désactivée",
  all: "Répétition : toute la file",
  one: "Répétition : ce morceau uniquement",
};

/**
 * Bouton de répétition, à trois états.
 *
 * Le mode « ce morceau » a sa **propre icône**, avec un 1 dans la boucle.
 * Un simple changement de couleur ne suffisait pas : rien ne distinguait
 * visuellement « toute la file » de « ce morceau », et la différence de
 * comportement ne se manifeste qu'à la fin d'un titre — soit plusieurs minutes
 * plus tard.
 */
function RepeatButton({ mode, onClick }: { mode: RepeatMode; onClick: () => void }) {
  return (
    <IconButton label={REPEAT_LABELS[mode]} active={mode !== "off"} onClick={onClick}>
      <path d="m17 2 4 4-4 4" />
      <path d="M3 11v-1a4 4 0 0 1 4-4h14" />
      <path d="m7 22-4-4 4-4" />
      <path d="M21 13v1a4 4 0 0 1-4 4H3" />
      {mode === "one" && <path d="M11 10h1v4" />}
    </IconButton>
  );
}

function IconButton({
  label,
  active = false,
  onClick,
  children,
}: {
  label: string;
  active?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={`flex h-8 w-8 items-center justify-center rounded-lg transition-colors hover:bg-elevated ${
        active ? "text-accent" : "text-ink-muted"
      }`}
    >
      <svg
        viewBox="0 0 24 24"
        className="h-4 w-4"
        fill="none"
        stroke="currentColor"
        strokeWidth={2}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden
      >
        {children}
      </svg>
    </button>
  );
}
