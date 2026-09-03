import { useEffect, useState } from "react";

import { ipc } from "@/lib/ipc";

/**
 * Teintes déjà connues, par empreinte de pochette.
 *
 * Au niveau du module : on revient sans cesse sur les mêmes pages, et les douze
 * pistes d'un album partagent leur pochette. Le cœur a le sien aussi — celui-ci
 * évite en plus l'aller-retour.
 */
const connues = new Map<string, string | null>();

/**
 * La couleur d'une pochette, pour le voile de sa page.
 *
 * # Pourquoi elle arrive après coup, et pourquoi ce n'est pas grave
 *
 * Décoder une image demande quelques millisecondes : la page s'affiche donc
 * d'abord en gris, puis se teinte. La transition est douce et le mouvement va
 * dans le bon sens — du neutre vers l'identité de l'album. L'inverse, une page
 * colorée qui redeviendrait grise, se remarquerait ; celui-ci passe pour une
 * arrivée.
 */
export function useTeinte(hash: string | null): string | null {
  const [teinte, setTeinte] = useState<string | null>(() =>
    hash === null ? null : (connues.get(hash) ?? null),
  );

  useEffect(() => {
    if (hash === null) {
      setTeinte(null);
      return;
    }

    const deja = connues.get(hash);
    if (deja !== undefined) {
      setTeinte(deja);
      return;
    }

    let vivant = true;
    void ipc
      .artworkTint(hash)
      .then((couleur) => {
        connues.set(hash, couleur);
        if (vivant) setTeinte(couleur);
      })
      .catch(() => undefined);

    return () => {
      vivant = false;
    };
  }, [hash]);

  return teinte;
}
