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
 * Le geste part-il d'une zone qui défile déjà horizontalement ?
 *
 * Les rangées de l'accueil — « Tes mix du jour », les reprises — se font
 * défiler du doigt, horizontalement. Un glissement qui commence sur elles leur
 * appartient : changer d'onglet **en plus** de les faire défiler serait deux
 * réponses à un seul geste.
 */
function dansUnDefilementHorizontal(cible: EventTarget | null): boolean {
  let noeud = cible instanceof Element ? cible : null;

  while (noeud !== null) {
    if (noeud.scrollWidth > noeud.clientWidth + 1) {
      const style = window.getComputedStyle(noeud).overflowX;
      if (style === "auto" || style === "scroll") return true;
    }
    noeud = noeud.parentElement;
  }

  return false;
}

/**
 * Passer d'un onglet à l'autre d'un glissement.
 *
 * # Pourquoi le toucher et non le pointeur
 *
 * `pointerup` paraissait le bon choix — un seul jeu d'événements pour la
 * souris et le doigt. Mais dès que le navigateur décide qu'un mouvement est un
 * défilement, il **annule** le pointeur : `pointercancel` part, `pointerup` ne
 * vient jamais, et le geste se perd en silence. Sur une liste qui défile,
 * c'est-à-dire partout, le glissement ne marchait tout simplement pas.
 *
 * `touchend` arrive dans tous les cas. Le geste ne servant qu'aux écrans
 * tactiles, il n'y a rien à perdre.
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
    onTouchStart: (event: React.TouchEvent) => {
      const doigt = event.touches[0];

      depart.current =
        doigt === undefined || event.touches.length > 1
          ? null
          : dansUnDefilementHorizontal(event.target)
            ? null
            : { x: doigt.clientX, y: doigt.clientY };
    },
    onTouchEnd: (event: React.TouchEvent) => {
      const origine = depart.current;
      depart.current = null;

      const doigt = event.changedTouches[0];
      if (origine === null || doigt === undefined) return;

      const dx = doigt.clientX - origine.x;
      const dy = doigt.clientY - origine.y;
      if (Math.abs(dx) < SEUIL || Math.abs(dx) < Math.abs(dy) * 2) return;

      const cible = PAGES[index + (dx < 0 ? 1 : -1)];
      if (cible !== undefined) naviguer(cible);
    },
    onTouchCancel: () => {
      depart.current = null;
    },
  };
}
