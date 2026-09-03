import { useEffect, useState } from "react";

/**
 * Garde un élément monté le temps de sa disparition.
 *
 * # Pourquoi il en faut un
 *
 * Tout ce qui apparaît dans l'application est monté par un `&&` et démonté de
 * la même façon. L'entrée est animée — le menu surgit, la barre monte — mais
 * la sortie n'existe pas : React retire le nœud, et l'élément **s'évapore**.
 *
 * La moitié de chaque interaction manquait donc. C'est la première chose qui
 * distingue une interface finie d'une interface qui fonctionne : ce qui part
 * doit partir quelque part, sans quoi l'œil enregistre une disparition et non
 * une fermeture.
 *
 * # L'usage
 *
 * ```tsx
 * const monte = useFermeture(ouvert);
 * return monte && <div className={ouvert ? "animate-surgir" : "animate-disparaitre"} />;
 * ```
 *
 * @param duree Doit couvrir l'animation de sortie. Plus court la coupe ; plus
 *   long laisse un nœud invisible en place, et donc un piège à clics.
 */
export function useFermeture(ouvert: boolean, duree = 120): boolean {
  const [monte, setMonte] = useState(ouvert);

  useEffect(() => {
    if (ouvert) {
      setMonte(true);
      return;
    }

    const minuteur = setTimeout(() => setMonte(false), duree);
    return () => clearTimeout(minuteur);
  }, [ouvert, duree]);

  return monte;
}
