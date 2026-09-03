import { useEffect, useState } from "react";

import { Icon } from "@/components/Icon";
import { formatDuration, ipc, type RemotePlayback } from "@/lib/ipc";

/**
 * Ce que l'autre appareil joue, et de quoi le commander.
 *
 * # Pourquoi une télécommande et non deux lectures
 *
 * Deux appareils avec chacun ses haut-parleurs ne peuvent pas jouer le même
 * morceau ensemble : on entendrait un écho, et aucune horloge partagée ne le
 * rattraperait. Un seul tient le son ; l'autre le voit et le commande.
 *
 * # Pourquoi la position avance toute seule
 *
 * L'autre appareil publie sa position à chaque seconde. Entre deux, la barre
 * s'arrêterait — et une barre qui avance par à-coups d'une seconde se remarque
 * plus qu'une barre absente. On l'avance donc localement, et chaque nouvelle
 * publication la recale.
 */
export function Telecommande() {
  const [distant, setDistant] = useState<RemotePlayback | null>(null);
  const [position, setPosition] = useState(0);
  const [occupe, setOccupe] = useState(false);

  useEffect(() => {
    const promesse = ipc.onRemotePlayback((etat) => {
      setDistant(etat);
      if (etat !== null) setPosition(etat.positionMs);
    });
    return () => {
      void promesse.then((arreter) => arreter());
    };
  }, []);

  // L'horloge locale entre deux publications. Elle ne sert qu'à combler.
  useEffect(() => {
    if (distant === null || !distant.enLecture) return;

    const battement = setInterval(() => {
      setPosition((valeur) => Math.min(distant.dureeMs, valeur + 500));
    }, 500);

    return () => clearInterval(battement);
  }, [distant]);

  if (distant === null) return null;

  const ratio = distant.dureeMs > 0 ? Math.min(1, position / distant.dureeMs) : 0;

  function ordonner(action: "lecture" | "pause" | "suivant" | "precedent") {
    if (distant === null) return;
    setOccupe(true);
    void ipc
      .linkCommand(distant.appareil, action)
      .catch(() => undefined)
      .finally(() => setOccupe(false));
  }

  return (
    <section className="rounded-xl border border-accent/25 bg-accent/5 p-5">
      <p className="text-[11px] font-semibold uppercase tracking-[0.14em] text-accent">
        En cours sur {distant.appareil}
      </p>

      <p className="mt-2 truncate text-[15px] font-semibold text-ink">{distant.titre}</p>
      <p className="truncate text-[13px] text-ink-muted">
        {distant.artiste ?? "Artiste inconnu"}
      </p>

      <div className="mt-3 flex items-center gap-2.5">
        <span className="numerals shrink-0 text-[11px] text-ink-faint">
          {formatDuration(position)}
        </span>
        <span className="h-1 flex-1 overflow-hidden rounded-full bg-raised">
          <span
            className="block h-full origin-left rounded-full bg-accent transition-transform duration-[500ms] ease-linear"
            style={{ transform: `scaleX(${ratio})` }}
          />
        </span>
        <span className="numerals shrink-0 text-[11px] text-ink-faint">
          {formatDuration(distant.dureeMs)}
        </span>
      </div>

      <div className="mt-4 flex items-center justify-center gap-6">
        <button
          type="button"
          aria-label="Morceau précédent"
          disabled={occupe}
          onClick={() => ordonner("precedent")}
          className="pression flex h-10 w-10 items-center justify-center rounded-full text-ink-muted hover:text-ink disabled:opacity-40"
        >
          <Icon name="previous" size={20} />
        </button>

        <button
          type="button"
          aria-label={distant.enLecture ? "Mettre en pause" : "Reprendre la lecture"}
          disabled={occupe}
          onClick={() => ordonner(distant.enLecture ? "pause" : "lecture")}
          className="pression flex h-12 w-12 items-center justify-center rounded-full bg-ink text-base disabled:opacity-40"
        >
          <span className={distant.enLecture ? "" : "translate-x-[1px]"}>
            <Icon name={distant.enLecture ? "pause" : "play"} size={20} />
          </span>
        </button>

        <button
          type="button"
          aria-label="Morceau suivant"
          disabled={occupe}
          onClick={() => ordonner("suivant")}
          className="pression flex h-10 w-10 items-center justify-center rounded-full text-ink-muted hover:text-ink disabled:opacity-40"
        >
          <Icon name="next" size={20} />
        </button>
      </div>

      {/* Le son sort de l'autre appareil : le dire évite de chercher pourquoi
          on n'entend rien ici. */}
      <p className="mt-3 text-center text-[11px] leading-snug text-ink-faint">
        Le son sort de {distant.appareil}. Pour l&apos;entendre ici, synchronise
        puis touche « Reprendre ici ».
      </p>
    </section>
  );
}
