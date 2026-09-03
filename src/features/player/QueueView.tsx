import { useRef, useState } from "react";

import { Artwork } from "@/features/library/Artwork";
import { Icon } from "@/components/Icon";
import { formatDuration, type PlaybackSnapshot } from "@/lib/ipc";

/** Hauteur d'une ligne, en pixels. Le calcul du glissement en dépend. */
const HAUTEUR = 56;

/**
 * La file d'attente, modifiable.
 *
 * # Pourquoi une page à elle, alors que « À suivre » existe
 *
 * « À suivre » répond à une question — qu'est-ce qui vient ? — et le fait avec
 * les lignes de la bibliothèque, qu'on sait lire d'un coup d'œil. Réordonner
 * en demande d'autres : une poignée qu'on attrape, une croix qu'on vise, et
 * surtout aucun appui long qui ouvrirait un menu au milieu d'un glissement.
 *
 * Les deux gestes ne tiennent pas sur la même ligne. Ils tiennent sur deux
 * pages.
 */
export function QueueView({
  state,
  onJump,
  onRemove,
  onMove,
}: {
  state: PlaybackSnapshot;
  onJump: (position: number) => void;
  onRemove: (position: number) => void;
  onMove: (from: number, to: number) => void;
}) {
  const depart = (state.queueIndex ?? -1) + 1;
  const suite = state.queue.slice(depart);

  /** Ce qu'on déplace, et où il en est. */
  const [glisse, setGlisse] = useState<{ index: number; ecart: number } | null>(null);
  const origine = useRef(0);

  /**
   * La destination, déduite de la distance parcourue.
   *
   * Les lignes ont toutes la même hauteur : le nombre de crans franchis est
   * une division. Mesurer la position de chaque ligne serait plus général et
   * inutile ici — et il faudrait la remesurer à chaque image.
   */
  const cible =
    glisse === null
      ? null
      : Math.max(
          0,
          Math.min(suite.length - 1, glisse.index + Math.round(glisse.ecart / HAUTEUR)),
        );

  function terminer() {
    if (glisse !== null && cible !== null && cible !== glisse.index) {
      onMove(depart + glisse.index, depart + cible);
    }
    setGlisse(null);
  }

  /** De combien cette ligne s'écarte pour laisser passer celle qu'on déplace. */
  function decalage(index: number): number {
    if (glisse === null || cible === null || index === glisse.index) return 0;
    if (glisse.index < cible && index > glisse.index && index <= cible) return -HAUTEUR;
    if (cible < glisse.index && index >= cible && index < glisse.index) return HAUTEUR;
    return 0;
  }

  return (
    <div className="px-4 pb-10 pt-6 lg:px-6">
      <p className="text-xs font-semibold uppercase tracking-[0.16em] text-ink-muted">
        Lecture
      </p>
      <h1 className="display-xl mt-2 text-[clamp(2rem,5.5vw,4.25rem)] text-ink">
        File d&apos;attente
      </h1>

      {state.current !== null && (
        <section className="mt-8">
          <h2 className="text-[11px] font-semibold uppercase tracking-[0.14em] text-ink-faint">
            En cours
          </h2>
          <div className="mt-2 flex items-center gap-3 rounded-lg bg-elevated px-3 py-2">
            <Artwork hash={state.current.artworkHash} className="h-10 w-10 shrink-0 rounded" />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-[15px] font-medium leading-tight text-accent">
                {state.current.title}
              </span>
              <span className="block truncate text-[13px] leading-tight text-ink-muted">
                {state.current.artist ?? "Artiste inconnu"}
              </span>
            </span>
          </div>
        </section>
      )}

      <section className="mt-8">
        <h2 className="text-[11px] font-semibold uppercase tracking-[0.14em] text-ink-faint">
          À suivre · {suite.length}
        </h2>

        {suite.length === 0 ? (
          <p className="mt-3 text-[13px] leading-relaxed text-ink-muted">
            Rien après celui-ci. Ajoute un album ou une playlist à la file
            depuis son bouton, ou « Lire ensuite » depuis un morceau.
          </p>
        ) : (
          <ul className="mt-2 select-none">
            {suite.map((item, index) => (
              <li
                key={`${item.trackId}-${index}`}
                style={{
                  height: HAUTEUR,
                  transform: `translateY(${
                    glisse?.index === index ? glisse.ecart : decalage(index)
                  }px)`,
                  // Celle qu'on déplace colle au doigt ; les autres glissent.
                  transition: glisse?.index === index ? "none" : "transform 160ms",
                  zIndex: glisse?.index === index ? 10 : undefined,
                  position: "relative",
                }}
                className={`flex items-center gap-2.5 rounded-md pr-1 ${
                  glisse?.index === index ? "bg-raised shadow-2xl shadow-black/50" : ""
                }`}
              >
                {/* # Pourquoi une poignée, et pas la ligne entière
                    Toucher la ligne veut dire « joue celui-là ». Si le même
                    appui pouvait aussi la déplacer, il faudrait deviner
                    l'intention à quelques pixels près. */}
                <button
                  type="button"
                  aria-label="Déplacer"
                  className="flex h-10 w-8 shrink-0 cursor-grab touch-none items-center justify-center text-ink-faint active:cursor-grabbing"
                  onPointerDown={(event) => {
                    event.currentTarget.setPointerCapture(event.pointerId);
                    origine.current = event.clientY;
                    setGlisse({ index, ecart: 0 });
                  }}
                  onPointerMove={(event) => {
                    if (glisse?.index !== index) return;
                    setGlisse({ index, ecart: event.clientY - origine.current });
                  }}
                  onPointerUp={terminer}
                  onPointerCancel={terminer}
                >
                  <Icon name="list" size={16} />
                </button>

                <button
                  type="button"
                  onClick={() => onJump(depart + index)}
                  className="flex min-w-0 flex-1 items-center gap-2.5 text-left"
                >
                  <Artwork hash={item.artworkHash} className="h-10 w-10 shrink-0 rounded" />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[15px] font-medium leading-tight text-ink">
                      {item.title}
                    </span>
                    <span className="block truncate text-[13px] leading-tight text-ink-muted">
                      {item.artist ?? "Artiste inconnu"}
                    </span>
                  </span>
                  <span className="numerals shrink-0 text-[12px] text-ink-faint">
                    {formatDuration(item.durationMs)}
                  </span>
                </button>

                <button
                  type="button"
                  aria-label={`Retirer ${item.title} de la file`}
                  onClick={() => onRemove(depart + index)}
                  className="pression flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-ink-faint transition-colors hover:bg-elevated hover:text-ink"
                >
                  <Icon name="close" size={15} />
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
