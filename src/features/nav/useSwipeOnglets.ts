import { useRef } from "react";

import type { Route } from "@/features/nav/Sidebar";

/**
 * Les pages qu'un glissement horizontal traverse.
 *
 * # Pourquoi la recherche n'y est pas
 *
 * Elle est un **mode**, pas une page : elle ouvre un champ, prend le clavier
 * et attend qu'on écrive. Y atterrir par un geste qu'on n'a pas voulu — le
 * clavier qui surgit, la bibliothèque qui disparaît — est un accident, pas une
 * navigation. On y va en la visant.
 */
const PAGES: Route[] = [
  { kind: "home" },
  { kind: "library" },
  { kind: "artists" },
  { kind: "more" },
];

/** Distance minimale, en pixels, pour qu'un mouvement compte comme un geste. */
const SEUIL = 70;

/**
 * Passer d'un onglet à l'autre d'un glissement.
 *
 * # Pourquoi le rapport entre les deux axes, et pas seulement la distance
 *
 * Une liste se fait défiler du pouce, et un pouce ne descend jamais droit :
 * soixante-dix pixels vers le bas s'accompagnent facilement de trente sur le
 * côté. Sans la condition « au moins deux fois plus horizontal que vertical »,
 * un défilement un peu oblique changerait de page — le pire des défauts, celui
 * qu'on ne peut pas reproduire à volonté et qu'on finit par croire imaginaire.
 *
 * # Pourquoi il ne s'applique qu'aux quatre racines
 *
 * Ailleurs, l'horizontale est déjà prise : sur l'écran de lecture elle déplace
 * la tête de lecture. Deux gestes identiques pour deux actions différentes,
 * c'est un tirage au sort.
 */
export function useSwipeOnglets(
  route: Route,
  naviguer: (route: Route) => void,
  actif: boolean,
) {
  const depart = useRef<{ x: number; y: number } | null>(null);

  const index = PAGES.findIndex((page) => page.kind === route.kind);
  const applicable = actif && index !== -1;

  if (!applicable) return {};

  return {
    onPointerDown: (event: React.PointerEvent) => {
      depart.current = { x: event.clientX, y: event.clientY };
    },
    onPointerUp: (event: React.PointerEvent) => {
      const origine = depart.current;
      depart.current = null;
      if (origine === null) return;

      const dx = event.clientX - origine.x;
      const dy = event.clientY - origine.y;
      if (Math.abs(dx) < SEUIL || Math.abs(dx) < Math.abs(dy) * 2) return;

      const cible = PAGES[index + (dx < 0 ? 1 : -1)];
      if (cible !== undefined) naviguer(cible);
    },
    onPointerCancel: () => {
      depart.current = null;
    },
  };
}
