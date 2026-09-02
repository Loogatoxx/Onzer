import { useEffect, useState } from "react";

/**
 * Largeur en dessous de laquelle l'interface bascule en disposition mobile.
 *
 * 768 px : au-delà, la barre latérale et le panneau de droite ont la place de
 * coexister avec une liste lisible ; en dessous, ils mangent tout.
 */
const SEUIL = 768;

/**
 * L'écran est-il étroit ?
 *
 * # Pourquoi la largeur et non le système
 *
 * On pourrait demander à Tauri s'il tourne sur Android. Ce serait répondre à
 * côté : ce qui décide de la disposition, c'est la place disponible, pas le
 * système d'exploitation. Une fenêtre de bureau réduite a exactement le même
 * problème qu'un téléphone, et la même solution lui convient.
 *
 * C'est aussi ce qui rend la disposition mobile **vérifiable sur le Mac** :
 * il suffit de rétrécir la fenêtre.
 */
export function useIsMobile(): boolean {
  const [mobile, setMobile] = useState(
    () => typeof window !== "undefined" && window.innerWidth < SEUIL,
  );

  useEffect(() => {
    const media = window.matchMedia(`(max-width: ${SEUIL - 1}px)`);
    const suivre = (event: MediaQueryListEvent) => setMobile(event.matches);

    setMobile(media.matches);
    media.addEventListener("change", suivre);
    return () => media.removeEventListener("change", suivre);
  }, []);

  return mobile;
}
