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
 * Ce que le doigt déplace visiblement, rapporté à ce qu'il parcourt.
 *
 * La page ne suit pas le doigt au pixel près : elle **résiste**. C'est ce qui
 * distingue un geste en cours d'un geste accompli, et ce qui rend le retour
 * élastique lisible quand on relâche trop tôt.
 */
const RESISTANCE = 0.32;

/**
 * L'écart dont la page s'approche sans jamais l'atteindre.
 *
 * # Pourquoi une asymptote et non un plafond
 *
 * L'écart était simplement écrêté : jusqu'à quatre-vingt-dix pixels la page
 * suivait, puis elle se **figeait** sous un doigt qui continuait d'avancer. La
 * résistance passait de 0,32 à zéro d'un seul coup — un mur, là où l'on
 * attend un élastique. C'est très exactement ce qu'on ressent comme « brut » :
 * une matière qui change de nature en cours de geste.
 *
 * La tangente hyperbolique donne la même pente au départ et s'approche de la
 * limite sans l'atteindre. La page résiste de plus en plus, et jamais
 * brutalement.
 */
const ECART_MAX = 90;

/**
 * Vitesse, en pixels par milliseconde, à partir de laquelle un geste compte
 * même s'il est court.
 *
 * Un coup de pouce vif de soixante pixels est un geste parfaitement clair, et
 * il ne se passait rien : seule la distance était regardée. Une application
 * qui ignore l'élan oblige à faire de grands gestes lents — et c'est ce qui
 * donne l'impression qu'elle ne comprend pas.
 */
const VITESSE_MIN = 0.45;

/** En dessous, même lancé, ce n'est pas un geste mais un frémissement. */
const DISTANCE_MIN = 24;

/** D'où la nouvelle page arrive. */
export type Sens = "gauche" | "droite";

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
  naviguer: (route: Route, sens: Sens) => void,
  actif: boolean,
  /** Le déplacement à appliquer à la page pendant le geste, en pixels. */
  onEcart: (ecart: number) => void,
) {
  const depart = useRef<{ x: number; y: number } | null>(null);
  /**
   * Le geste a-t-il été reconnu comme horizontal ?
   *
   * Sans ce verrou, repasser sous les douze pixels en cours de mouvement
   * remettait l'écart à zéro : la page repartait en animation de retour
   * pendant que le doigt était encore posé, et le suivi décrochait.
   */
  const horizontal = useRef(false);
  /** Les deux dernières positions, pour mesurer l'élan au relâchement. */
  const dernier = useRef<{ x: number; t: number } | null>(null);

  const index = PAGES.findIndex((page) => page.kind === route.kind);
  const applicable = actif && index !== -1;

  if (!applicable) return {};

  /** Y a-t-il une page de ce côté ? Sinon le geste n'a nulle part où aller. */
  const existe = (dx: number) => PAGES[index + (dx < 0 ? 1 : -1)] !== undefined;

  return {
    onTouchStart: (event: React.TouchEvent) => {
      const doigt = event.touches[0];

      horizontal.current = false;
      dernier.current = null;
      depart.current =
        doigt === undefined || event.touches.length > 1
          ? null
          : dansUnDefilementHorizontal(event.target)
            ? null
            : { x: doigt.clientX, y: doigt.clientY };
    },
    /**
     * La page suit le doigt, amortie.
     *
     * Elle ne bouge que si le mouvement est déjà nettement horizontal : sinon
     * le simple fait de faire défiler une liste ferait trembler la page
     * latéralement à chaque pouce qui dévie.
     */
    onTouchMove: (event: React.TouchEvent) => {
      const origine = depart.current;
      const doigt = event.touches[0];
      if (origine === null || doigt === undefined) return;

      const dx = doigt.clientX - origine.x;
      const dy = doigt.clientY - origine.y;

      // Un repère tous les cinquante millisecondes, pas à chaque image : la
      // vitesse mesurée sur seize millisecondes est du bruit, pas un élan.
      const trace = dernier.current;
      if (trace === null || event.timeStamp - trace.t > 50) {
        dernier.current = { x: doigt.clientX, t: event.timeStamp };
      }

      // Une fois reconnu, le geste le reste : on ne redemande pas au doigt de
      // prouver son intention à chaque image.
      if (!horizontal.current) {
        if (Math.abs(dx) < 12 || Math.abs(dx) < Math.abs(dy) * 2) {
          onEcart(0);
          return;
        }
        horizontal.current = true;
      }

      // Sans page de ce côté, l'écart est divisé par trois : le geste répond,
      // mais son inutilité se sent avant qu'on n'aille au bout.
      const resistance = existe(dx) ? RESISTANCE : RESISTANCE / 3;
      onEcart(ECART_MAX * Math.tanh((dx * resistance) / ECART_MAX));
    },
    onTouchEnd: (event: React.TouchEvent) => {
      const origine = depart.current;
      depart.current = null;
      horizontal.current = false;
      onEcart(0);

      const doigt = event.changedTouches[0];
      if (origine === null || doigt === undefined) return;

      const dx = doigt.clientX - origine.x;
      const dy = doigt.clientY - origine.y;
      if (Math.abs(dx) < Math.abs(dy) * 2) return;

      // L'élan des derniers instants, et non la moyenne depuis le départ : un
      // geste qui hésite puis part vif est un geste vif.
      const trace = dernier.current;
      const duree = trace === null ? 0 : event.timeStamp - trace.t;
      const vitesse =
        trace === null || duree <= 0
          ? 0
          : Math.abs(doigt.clientX - trace.x) / duree;

      const assezLoin = Math.abs(dx) >= SEUIL;
      const assezVif = vitesse >= VITESSE_MIN && Math.abs(dx) >= DISTANCE_MIN;
      if (!assezLoin && !assezVif) return;

      const cible = PAGES[index + (dx < 0 ? 1 : -1)];
      // Le doigt part vers la gauche : la page suivante arrive **par la
      // droite**, dans le prolongement du mouvement. L'inverse la ferait
      // entrer à contresens du geste qui l'a appelée.
      if (cible !== undefined) naviguer(cible, dx < 0 ? "droite" : "gauche");
    },
    onTouchCancel: () => {
      depart.current = null;
      horizontal.current = false;
      onEcart(0);
    },
  };
}
