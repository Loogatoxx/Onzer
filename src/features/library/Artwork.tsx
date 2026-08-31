import { useEffect, useState } from "react";

import { ipc } from "@/lib/ipc";

/**
 * Vignette de pochette.
 *
 * Les images sont mises en cache par empreinte au niveau du module : les douze
 * pistes d'un album partagent la même pochette, et il serait absurde de la
 * redemander douze fois au backend.
 *
 * ⚠️ Provisoire, comme la commande qu'elle appelle : les *data URI* conviennent
 * à une liste, pas à une grille de plusieurs centaines de pochettes.
 */
const cache = new Map<string, Promise<string | null>>();

function load(hash: string): Promise<string | null> {
  const cached = cache.get(hash);
  if (cached !== undefined) return cached;

  const pending = ipc.artworkDataUri(hash).catch(() => null);
  cache.set(hash, pending);
  return pending;
}

interface ArtworkProps {
  hash: string | null;
  /**
   * Dimensions et arrondi. Portés par l'appelant plutôt que fixés ici : la même
   * pochette sert de vignette de 40 px dans une liste et d'affiche de 300 px
   * dans le panneau de lecture.
   */
  className?: string;
}

export function Artwork({ hash, className = "h-10 w-10 rounded" }: ArtworkProps) {
  const [source, setSource] = useState<string | null>(null);

  useEffect(() => {
    if (hash === null) {
      setSource(null);
      return;
    }

    let active = true;
    void load(hash).then((uri) => {
      // Le composant a pu être démonté, ou le morceau changé, entre-temps.
      if (active) setSource(uri);
    });

    return () => {
      active = false;
    };
  }, [hash]);

  if (source === null) {
    // Le creux garde exactement la place de l'image : sans lui, la mise en page
    // sauterait au moment où la pochette arrive.
    return <div aria-hidden className={`shrink-0 bg-elevated ${className}`} />;
  }

  return <img src={source} alt="" className={`shrink-0 object-cover ${className}`} />;
}
