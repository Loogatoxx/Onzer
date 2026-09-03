import { useState } from "react";

import { Icon } from "@/components/Icon";

/**
 * Tout l'entretien de la bibliothèque, sous une seule porte.
 *
 * # Le problème que cette carte résout
 *
 * Six bandeaux — doublons, paroles, pochettes, albums, fichiers manquants,
 * écoute à l'oreille — s'empilaient en haut de la bibliothèque. Sur un
 * téléphone, il fallait faire défiler un écran entier avant d'atteindre le
 * premier morceau. Or on ouvre sa bibliothèque pour écouter, pas pour
 * l'entretenir.
 *
 * # Pourquoi les replier plutôt que les cacher
 *
 * Rangés dans les réglages, ils deviendraient invisibles : personne ne va
 * chercher une pochette manquante dans un menu. Repliés sous un compte — « 4
 * choses à vérifier » —, ils restent sous les yeux sans occuper la place.
 *
 * # Pourquoi le compte, et pas seulement un titre
 *
 * Un intitulé « Entretien » n'apprend rien : il faut ouvrir pour savoir s'il y
 * a quelque chose à faire. Le nombre répond à la question avant le clic, et
 * c'est justement ce qui permet de ne pas cliquer.
 */
export function MaintenanceCard({
  pending,
  children,
}: {
  /** Nombre de choses réellement à faire, tous outils confondus. */
  pending: number;
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(false);

  return (
    <div className="rounded-xl bg-elevated">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        className="pression flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left"
      >
        <span className={`shrink-0 ${pending > 0 ? "text-ink-muted" : "text-ink-faint"}`}>
          <Icon name="settings" size={16} />
        </span>

        <span className="min-w-0 flex-1">
          <span className="block truncate text-xs text-ink">Entretien</span>
          <span className="block truncate text-[11px] text-ink-faint">
            {pending === 0
              ? "Rien à faire pour l'instant"
              : `${pending} chose${pending > 1 ? "s" : ""} à vérifier`}
          </span>
        </span>

        <span
          className={`shrink-0 text-ink-faint transition-transform ${open ? "rotate-180" : ""}`}
        >
          <Icon name="chevronDown" size={16} />
        </span>
      </button>

      {open && <div className="space-y-2 px-2 pb-2">{children}</div>}
    </div>
  );
}
