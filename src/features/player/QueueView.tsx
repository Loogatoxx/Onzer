import { useEffect, useRef, useState } from "react";

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
  onOpenPlaying,
  onOpenLibrary,
}: {
  state: PlaybackSnapshot;
  onJump: (position: number) => void;
  onRemove: (position: number) => void;
  onMove: (from: number, to: number) => void;
  /**
   * Ouvre l'écran de lecture.
   *
   * Le bloc « En cours » est bâti exactement comme les lignes du dessous —
   * pochette, titre, artiste — et celles-là répondent au doigt. Le seul qu'on
   * regarde vraiment était le seul mort.
   */
  onOpenPlaying: () => void;
  /** La bibliothèque, quand la file est vide et qu'il faut la remplir. */
  onOpenLibrary: () => void;
}) {
  const depart = (state.queueIndex ?? -1) + 1;
  const suite = state.queue.slice(depart);

  /**
   * Ce qu'on déplace, et où il en est.
   *
   * `pose` distingue les deux moitiés du geste : tant qu'il est faux, la ligne
   * colle au doigt ; une fois vrai, elle rejoint sa case d'arrivée.
   */
  const [glisse, setGlisse] = useState<{
    index: number;
    ecart: number;
    pose: boolean;
  } | null>(null);
  const origine = useRef(0);

  /* La liste réordonnée qui revient du cœur efface le déplacement : à cet
     instant précis, la ligne posée et la ligne réordonnée occupent le même
     pixel, et l'échange ne se voit pas. Le délai de sécurité ne sert que si
     la commande échoue — sans lui, la ligne resterait soulevée. */
  const signature = suite.map((item) => item.trackId).join(",");
  useEffect(() => {
    setGlisse(null);
  }, [signature]);

  useEffect(() => {
    if (glisse === null || !glisse.pose) return;
    const minuteur = setTimeout(() => setGlisse(null), 400);
    return () => clearTimeout(minuteur);
  }, [glisse]);

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

  /**
   * On lâche.
   *
   * # Pourquoi la ligne ne revient pas à sa case de départ
   *
   * Elle le faisait : le déplacement était effacé, la ligne repartait en
   * arrière sur cent soixante millisecondes pendant que l'ordre voyageait vers
   * le cœur, puis la nouvelle liste la faisait réapparaître ailleurs d'un
   * coup. Deux mouvements contradictoires pour un seul geste — on ne voyait
   * jamais le morceau se poser où on l'avait mis.
   *
   * Il est posé sur sa case d'arrivée, au pixel où la liste réordonnée le
   * dessinera. Quand elle arrive, l'échange est invisible.
   */
  function terminer() {
    if (glisse === null || glisse.pose) return;

    if (cible === null || cible === glisse.index) {
      // Rien n'a bougé : rien à attendre, la ligne se repose tout de suite.
      setGlisse(null);
      return;
    }

    onMove(depart + glisse.index, depart + cible);
    setGlisse({ index: glisse.index, ecart: (cible - glisse.index) * HAUTEUR, pose: true });
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
          <button
            type="button"
            onClick={onOpenPlaying}
            className="pression mt-2 flex w-full items-center gap-3 rounded-lg bg-elevated px-3 py-2 text-left hover:bg-raised"
          >
            <Artwork hash={state.current.artworkHash} className="h-10 w-10 shrink-0 rounded" />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-[15px] font-medium leading-tight text-accent">
                {state.current.title}
              </span>
              <span className="block truncate text-[13px] leading-tight text-ink-muted">
                {state.current.artist ?? "Artiste inconnu"}
              </span>
            </span>
          </button>
        </section>
      )}

      <section className="mt-8">
        <h2 className="text-[11px] font-semibold uppercase tracking-[0.14em] text-ink-faint">
          À suivre · {suite.length}
        </h2>

        {suite.length === 0 ? (
          <div className="mt-3">
            <p className="text-[13px] leading-relaxed text-ink-muted">
              Rien après celui-ci. Ajoute un album ou une playlist à la file
              depuis son bouton, ou « Lire ensuite » depuis un morceau.
            </p>
            {/* Le texte nomme deux gestes qui se font ailleurs : sans porte
                vers cet ailleurs, il ne fait que décrire. */}
            <button
              type="button"
              onClick={onOpenLibrary}
              className="pression mt-3 rounded-full bg-elevated px-4 py-2 text-[13px] font-medium text-ink hover:bg-raised"
            >
              Aller à la bibliothèque
            </button>
          </div>
        ) : (
          <ul className="mt-2 select-none">
            {suite.map((item, index) => {
              const saisie = glisse?.index === index;
              /* Deux moitiés du geste, deux comportements : tant qu'on tient,
                 la ligne suit le doigt sans transition — la moindre en ferait
                 un retard. Une fois lâchée, elle voyage jusqu'à sa case. */
              const colle = saisie && glisse !== null && !glisse.pose;
              const ecart = saisie && glisse !== null ? glisse.ecart : decalage(index);

              return (
              <li
                key={`${item.trackId}-${index}`}
                style={{
                  height: HAUTEUR,
                  // Un pour cent et demi d'échelle : la ligne décolle du plan
                  // au lieu de simplement changer de couleur.
                  transform: `translateY(${ecart}px) scale(${colle ? 1.015 : 1})`,
                  transition: colle
                    ? "background-color 140ms var(--ease-out-soft), box-shadow 140ms var(--ease-out-soft)"
                    : "transform 200ms var(--ease-out-soft), background-color 140ms var(--ease-out-soft), box-shadow 140ms var(--ease-out-soft)",
                  zIndex: saisie ? 10 : undefined,
                  position: "relative",
                }}
                className={`flex items-center gap-2.5 rounded-md pr-1 ${
                  colle ? "bg-raised shadow-2xl shadow-black/50" : ""
                }`}
              >
                {/* # Pourquoi une poignée, et pas la ligne entière
                    Toucher la ligne veut dire « joue celui-là ». Si le même
                    appui pouvait aussi la déplacer, il faudrait deviner
                    l'intention à quelques pixels près. */}
                <button
                  type="button"
                  aria-label="Déplacer"
                  className={`flex h-10 w-8 shrink-0 cursor-grab touch-none items-center justify-center transition-colors active:cursor-grabbing ${
                    colle ? "text-ink" : "text-ink-faint"
                  }`}
                  onPointerDown={(event) => {
                    event.currentTarget.setPointerCapture(event.pointerId);
                    origine.current = event.clientY;
                    setGlisse({ index, ecart: 0, pose: false });
                  }}
                  onPointerMove={(event) => {
                    if (glisse?.index !== index || glisse.pose) return;
                    setGlisse({ index, ecart: event.clientY - origine.current, pose: false });
                  }}
                  onPointerUp={terminer}
                  onPointerCancel={terminer}
                >
                  <Icon name="list" size={16} />
                </button>

                <button
                  type="button"
                  onClick={() => onJump(depart + index)}
                  className="pression flex min-w-0 flex-1 items-center gap-2.5 rounded-md text-left"
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
              );
            })}
          </ul>
        )}
      </section>
    </div>
  );
}
