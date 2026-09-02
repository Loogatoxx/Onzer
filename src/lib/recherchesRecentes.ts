const CLE = "onzer.recherches";
const MAX = 8;

/**
 * Les dernières recherches, du plus récent au plus ancien.
 *
 * # Pourquoi les retenir
 *
 * Un champ de recherche vide ne propose rien, alors qu'on cherche presque
 * toujours les mêmes artistes. Les rappeler évite de retaper « adèle » pour la
 * quatrième fois — c'est la seule chose qu'un champ vide peut offrir d'utile.
 *
 * # Pourquoi dans le navigateur et pas en base
 *
 * Ce sont des traces, pas des données. Les perdre ne coûte rien, et elles
 * n'ont rien à faire dans la bibliothèque de quelqu'un.
 */
export function recherchesRecentes(): string[] {
  try {
    const brut = localStorage.getItem(CLE);
    const liste: unknown = brut === null ? [] : JSON.parse(brut);
    return Array.isArray(liste) ? liste.filter((item): item is string => typeof item === "string") : [];
  } catch {
    return [];
  }
}

/** Retient une recherche, sans doublon, et oublie les plus anciennes. */
export function retenirRecherche(terme: string): void {
  const propre = terme.trim();
  if (propre === "") return;

  try {
    const sans = recherchesRecentes().filter(
      (ancien) => ancien.toLocaleLowerCase("fr") !== propre.toLocaleLowerCase("fr"),
    );
    localStorage.setItem(CLE, JSON.stringify([propre, ...sans].slice(0, MAX)));
  } catch {
    // Stockage refusé : la recherche marche quand même, sans mémoire.
  }
}

/** Oublie tout. */
export function oublierRecherches(): void {
  try {
    localStorage.removeItem(CLE);
  } catch {
    // Rien à faire : il n'y avait rien à oublier.
  }
}
