import { useEffect, useRef, useState } from "react";

/**
 * Fait apparaître un élément lorsqu'il entre dans le champ de vision.
 *
 * # Pourquoi un observateur plutôt qu'un écouteur de défilement
 *
 * Un `onScroll` se déclenche des dizaines de fois par seconde et impose de
 * mesurer la position de chaque section — travail qui bloque le fil principal
 * et fait saccader le défilement, précisément ce qu'on cherche à rendre
 * agréable. `IntersectionObserver` est évalué par le navigateur hors de ce fil.
 *
 * L'observation cesse dès le premier passage : une section déjà vue n'a pas à
 * disparaître si l'on remonte.
 */
export function useReveal<T extends HTMLElement>(delayMs = 0) {
  const ref = useRef<T>(null);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    const element = ref.current;
    if (element === null) return;

    // Respecte le réglage d'accessibilité du système : une animation
    // d'apparition peut être pénible, voire nauséeuse, pour certaines personnes.
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      setVisible(true);
      return;
    }

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting !== true) return;

        window.setTimeout(() => setVisible(true), delayMs);
        observer.disconnect();
      },
      // La section se révèle un peu avant d'être pleinement visible :
      // l'animation est déjà en cours quand l'œil arrive dessus.
      { threshold: 0.15, rootMargin: "0px 0px -8% 0px" },
    );

    observer.observe(element);
    return () => observer.disconnect();
  }, [delayMs]);

  return {
    ref,
    className: `reveal${visible ? " reveal-visible" : ""}`,
  };
}
