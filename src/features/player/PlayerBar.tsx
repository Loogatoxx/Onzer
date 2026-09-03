import { useEffect, useState } from "react";

import { Artwork } from "@/features/library/Artwork";
import { Icon, IconButton } from "@/components/Icon";
import { formatDuration, type PlaybackSnapshot, type RepeatMode } from "@/lib/ipc";

interface PlayerBarProps {
  state: PlaybackSnapshot;
  isLoved: boolean;
  panel: "closed" | "lyrics" | "queue";
  onToggle: () => void;
  onNext: () => void;
  onPrevious: () => void;
  onSeek: (positionMs: number) => void;
  onVolume: (volume: number) => void;
  onShuffle: (shuffle: boolean) => void;
  onRepeat: () => void;
  onToggleLoved: () => void;
  onOpenPanel: (tab: "lyrics" | "queue") => void;
  /**
   * Ouvre le grand lecteur.
   *
   * # Pourquoi la pochette est le bouton
   *
   * C'est déjà le geste sur téléphone, et c'est celui qu'on essaie d'instinct
   * sur un bureau : la pochette est la seule chose qui représente le morceau
   * en entier. Le grand lecteur existait pourtant, sans qu'aucun chemin n'y
   * mène depuis la barre du bas.
   */
  onOpenPlayer: () => void;
}

/**
 * Barre de lecture.
 *
 * # Pourquoi les commandes sont au centre
 *
 * Elles étaient à gauche jusqu'ici, ce qui est défendable : la main y va
 * naturellement. Mais la barre occupe toute la largeur de la fenêtre, et sur un
 * écran large les commandes se retrouvaient tassées dans un coin pendant que le
 * milieu restait vide.
 *
 * Au centre, elles gagnent une chose qu'aucune autre disposition ne permet : la
 * **barre de progression peut faire toute la largeur utile**. Sur un morceau de
 * quatre minutes, cela fait la différence entre viser une seconde et en viser
 * dix. Le morceau en cours reste à gauche, les réglages à droite : trois zones
 * de rôles distincts, et pas un pixel perdu.
 */
export function PlayerBar(props: PlayerBarProps) {
  const { state } = props;
  const track = state.current;

  if (track === null) {
    return null;
  }

  return (
    <footer className="shrink-0 px-2 pb-2">
      <div className="flex items-center gap-4 rounded-xl bg-surface px-4 py-3">
        {/* ── Ce qui joue ──────────────────────────────────────────────── */}
        <div className="flex w-[26%] min-w-0 shrink-0 items-center gap-3">
          <button
            type="button"
            title="Ouvrir le lecteur"
            aria-label="Ouvrir le lecteur"
            onClick={props.onOpenPlayer}
            className="shrink-0 rounded-md transition-transform duration-150 hover:scale-[1.04] active:scale-95"
          >
            <Artwork hash={track.artworkHash} className="h-14 w-14 rounded-md" />
          </button>

          <div className="min-w-0">
            <p className="truncate text-sm font-semibold text-ink">{track.title}</p>
            <p className="truncate text-xs text-ink-muted">
              {track.artist ?? "Artiste inconnu"}
            </p>
          </div>

          <button
            type="button"
            title={props.isLoved ? "Retirer des favoris" : "Ajouter aux favoris"}
            aria-label={props.isLoved ? "Retirer des favoris" : "Ajouter aux favoris"}
            onClick={props.onToggleLoved}
            className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-full transition-colors ${
              props.isLoved ? "text-accent" : "text-ink-faint hover:text-ink"
            }`}
          >
            <Icon name={props.isLoved ? "heartFilled" : "heart"} size={17} />
          </button>
        </div>

        {/* ── Commandes ────────────────────────────────────────────────── */}
        <div className="flex min-w-0 flex-1 flex-col items-center gap-1.5">
          <div className="flex items-center gap-2">
            <IconButton
              name="shuffle"
              label={state.shuffle ? "Aléatoire : activé" : "Aléatoire : désactivé"}
              active={state.shuffle}
              onClick={() => props.onShuffle(!state.shuffle)}
            />

            <IconButton name="previous" label="Précédent" onClick={props.onPrevious} />

            <button
              type="button"
              aria-label={state.isPlaying ? "Pause" : "Lecture"}
              title={state.isPlaying ? "Pause" : "Lecture"}
              onClick={props.onToggle}
              className="mx-1 flex h-10 w-10 items-center justify-center rounded-full bg-ink text-base transition-transform duration-150 hover:scale-[1.06] active:scale-95"
            >
              {/* Le triangle de lecture est optiquement décentré : sa masse est
                  à gauche, il faut le pousser d'un poil pour qu'il paraisse
                  centré dans le disque. */}
              <span className={state.isPlaying ? "" : "translate-x-[1px]"}>
                <Icon name={state.isPlaying ? "pause" : "play"} size={18} />
              </span>
            </button>

            <IconButton name="next" label="Suivant" onClick={props.onNext} />

            <RepeatButton mode={state.repeat} onClick={props.onRepeat} />
          </div>

          <Seekbar
            positionMs={state.positionMs}
            durationMs={state.durationMs || track.durationMs}
            onSeek={props.onSeek}
          />
        </div>

        {/* ── Réglages ─────────────────────────────────────────────────── */}
        <div className="flex w-[26%] shrink-0 items-center justify-end gap-1">
          <IconButton
            name="lyrics"
            label="Paroles"
            active={props.panel === "lyrics"}
            onClick={() => props.onOpenPanel("lyrics")}
          />
          <IconButton
            name="queue"
            label="File d'attente"
            active={props.panel === "queue"}
            onClick={() => props.onOpenPanel("queue")}
          />

          <VolumeControl volume={state.volume} onVolume={props.onVolume} />
        </div>
      </div>
    </footer>
  );
}

/**
 * Réglage du volume.
 *
 * L'icône est cliquable et coupe le son en mémorisant le niveau précédent :
 * couper puis rétablir ne doit pas obliger à retrouver son réglage au pixel.
 */
function VolumeControl({
  volume,
  onVolume,
}: {
  volume: number;
  onVolume: (volume: number) => void;
}) {
  const [beforeMute, setBeforeMute] = useState(0.8);
  const muted = volume === 0;

  return (
    <div className="ml-1 flex items-center gap-2">
      <button
        type="button"
        title={muted ? "Rétablir le son" : "Couper le son"}
        aria-label={muted ? "Rétablir le son" : "Couper le son"}
        onClick={() => {
          if (muted) {
            onVolume(beforeMute);
          } else {
            setBeforeMute(volume);
            onVolume(0);
          }
        }}
        className="text-ink-muted transition-colors hover:text-ink"
      >
        <Icon name={muted ? "volumeMute" : "volume"} size={17} />
      </button>

      <input
        type="range"
        min={0}
        max={1}
        step={0.01}
        value={volume}
        aria-label="Volume"
        onChange={(event) => onVolume(Number(event.target.value))}
        className="h-1 w-24 cursor-pointer appearance-none rounded-full bg-raised accent-ink"
      />
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
    <div className="flex w-full items-center gap-2.5">
      <span className="numerals w-9 shrink-0 text-right text-[11px] text-ink-faint">
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
        className="group relative h-3 flex-1 cursor-pointer py-1"
      >
        <div className="h-1 overflow-hidden rounded-full bg-raised">
          <div
            className="h-full rounded-full bg-ink-muted transition-colors group-hover:bg-accent"
            style={{ width: `${ratio * 100}%` }}
          />
        </div>

        {/* La poignée n'apparaît qu'au survol : en permanence, elle attirerait
            l'œil sur un réglage qu'on ne touche presque jamais. */}
        <span
          aria-hidden
          className="pointer-events-none absolute top-1/2 h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-ink opacity-0 transition-opacity group-hover:opacity-100"
          style={{ left: `${ratio * 100}%` }}
        />
      </div>

      <span className="numerals w-9 shrink-0 text-[11px] text-ink-faint">
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
    <IconButton
      name={mode === "one" ? "repeatOne" : "repeat"}
      label={REPEAT_LABELS[mode]}
      active={mode !== "off"}
      onClick={onClick}
    />
  );
}
