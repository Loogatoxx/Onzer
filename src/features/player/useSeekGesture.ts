import { useRef } from "react";

/** Au-delà, c'est un geste ; en deçà, c'est un appui qui a tremblé. */
const SEUIL_PX = 40;

/** Ce qu'un glissement d'un bout à l'autre de l'écran déplace. */
const SAUT_MS = 15_000;

/**
 * Glisser horizontalement sur le lecteur pour se déplacer dans le morceau.
 *
 * # Pourquoi un saut fixe et non une position absolue
 *
 * Traiter la barre comme une règle — le doigt à 30 % de la largeur donne 30 %
 * du morceau — serait juste sur la grande barre, et faux sur la petite : elle
 * fait quelques dizaines de pixels de haut et n'a aucune graduation. Un saut
 * de quinze secondes par glissement se comprend partout, et c'est exactement
 * ce qu'on veut quand on rate une phrase.
 *
 * # Pourquoi le seuil
 *
 * Un appui n'est jamais parfaitement immobile. Sans marge, ouvrir l'écran de
 * lecture déplacerait le morceau de quelques secondes au passage.
 */
export function useSeekGesture(
  positionMs: number,
  durationMs: number,
  onSeek: (positionMs: number) => void,
) {
  const depart = useRef<{ x: number; position: number } | null>(null);

  const commencer = (x: number) => {
    depart.current = { x, position: positionMs };
  };

  /** Vrai si le geste a été traité — l'appelant sait alors qu'il n'a rien à faire. */
  const finir = (x: number): boolean => {
    const origine = depart.current;
    depart.current = null;
    if (origine === null) return false;

    const ecart = x - origine.x;
    if (Math.abs(ecart) < SEUIL_PX) return false;

    const cible = origine.position + Math.sign(ecart) * SAUT_MS;
    onSeek(Math.min(Math.max(0, cible), Math.max(0, durationMs)));
    return true;
  };

  return {
    // # Pourquoi la souris et le doigt sont séparés
    //
    // Un écran tactile émet **les deux** familles d'événements : traiter les
    // deux ferait deux sauts pour un seul geste. Et l'on ne peut pas se
    // contenter du pointeur : dès que le navigateur décide qu'un mouvement est
    // un défilement, il annule le pointeur — `pointercancel` part, `pointerup`
    // ne vient jamais, et le geste se perd en silence.
    onPointerDown: (event: React.PointerEvent) => {
      if (event.pointerType === "mouse") commencer(event.clientX);
    },
    onPointerUp: (event: React.PointerEvent) => {
      if (event.pointerType !== "mouse") return;

      if (finir(event.clientX)) {
        // Le geste l'emporte sur le clic qu'il aurait aussi déclenché.
        event.preventDefault();
        event.stopPropagation();
      }
    },
    onTouchStart: (event: React.TouchEvent) => {
      const doigt = event.touches[0];
      if (doigt !== undefined && event.touches.length === 1) commencer(doigt.clientX);
      else depart.current = null;
    },
    onTouchEnd: (event: React.TouchEvent) => {
      const doigt = event.changedTouches[0];
      if (doigt === undefined) return;

      if (finir(doigt.clientX)) {
        event.preventDefault();
        event.stopPropagation();
      }
    },
    onTouchCancel: () => {
      depart.current = null;
    },
  };
}
