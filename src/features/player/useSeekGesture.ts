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

  return {
    onPointerDown: (event: React.PointerEvent) => {
      depart.current = { x: event.clientX, position: positionMs };
    },
    onPointerUp: (event: React.PointerEvent) => {
      const origine = depart.current;
      depart.current = null;
      if (origine === null) return;

      const ecart = event.clientX - origine.x;
      if (Math.abs(ecart) < SEUIL_PX) return;

      // Le geste l'emporte sur le clic qu'il aurait aussi déclenché.
      event.preventDefault();
      event.stopPropagation();

      const cible = origine.position + Math.sign(ecart) * SAUT_MS;
      onSeek(Math.min(Math.max(0, cible), Math.max(0, durationMs)));
    },
  };
}
