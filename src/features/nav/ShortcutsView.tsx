import { useEffect, useState } from "react";

import { Icon, type IconName } from "@/components/Icon";
import { ipc, type MediaKeysStatus } from "@/lib/ipc";

/**
 * Les raccourcis, en un seul endroit.
 *
 * # Pourquoi une page et non une infobulle par bouton
 *
 * Un raccourci ne s'apprend pas au moment où l'on en a besoin — à ce
 * moment-là, on a déjà pris la souris. Il s'apprend en le lisant une fois,
 * tranquillement. D'où une page qu'on parcourt, plutôt que des indications
 * dispersées qu'on ne croise jamais toutes.
 */
interface Shortcut {
  keys: string[];
  action: string;
  note?: string;
}

interface Group {
  title: string;
  icon: IconName;
  note?: string;
  shortcuts: Shortcut[];
}

const GROUPS: Group[] = [
  {
    title: "Touches multimédia",
    icon: "play",
    note: "Elles fonctionnent même quand Onzer n'est pas au premier plan — c'est le propre d'une touche de lecture.",
    shortcuts: [
      { keys: ["F8"], action: "Lecture / pause", note: "⏯ sur un clavier Apple" },
      { keys: ["F9"], action: "Morceau suivant", note: "⏭" },
      { keys: ["F7"], action: "Morceau précédent", note: "⏮" },
    ],
  },
  {
    title: "Lecture",
    icon: "pause",
    note: "Actifs dans la fenêtre, sauf pendant une saisie de texte.",
    shortcuts: [
      { keys: ["Espace"], action: "Lecture / pause" },
      {
        keys: ["→"],
        action: "Avancer de 5 secondes",
        note: "maintenue, l'avance s'élargit à 15 puis 30 secondes",
      },
      { keys: ["←"], action: "Reculer de 5 secondes" },
      { keys: ["⌘", "→"], action: "Morceau suivant" },
      { keys: ["⌘", "←"], action: "Morceau précédent" },
      { keys: ["↑"], action: "Monter le volume" },
      { keys: ["↓"], action: "Baisser le volume" },
      { keys: ["M"], action: "Couper ou rétablir le son" },
    ],
  },
  {
    title: "Le morceau en cours",
    icon: "heart",
    shortcuts: [
      { keys: ["L"], action: "Ajouter ou retirer des titres likés" },
      { keys: ["S"], action: "Lecture aléatoire" },
      { keys: ["R"], action: "Répétition : file, morceau, aucune" },
    ],
  },
  {
    title: "Navigation",
    icon: "home",
    shortcuts: [
      { keys: ["⌘", "F"], action: "Aller à la recherche" },
      { keys: ["⌘", "?"], action: "Afficher cette page" },
      { keys: ["Échap"], action: "Quitter la recherche ou fermer un menu" },
    ],
  },
];

export function ShortcutsView() {
  const [media, setMedia] = useState<MediaKeysStatus | null>(null);

  useEffect(() => {
    void ipc.mediaKeysStatus().then(setMedia).catch(() => undefined);
  }, []);

  return (
    <div className="px-6 pb-16 pt-8 sm:px-10">
      <header className="mx-auto max-w-3xl">
        <p className="text-xs font-semibold uppercase tracking-[0.16em] text-ink-faint">
          Aide-mémoire
        </p>
        <h1 className="display-xl mt-2 text-[clamp(1.75rem,4vw,2.75rem)] text-ink">
          Raccourcis
        </h1>
      </header>

      {media !== null && !media.working && (
        <div className="mx-auto mt-6 max-w-3xl rounded-xl bg-warn/10 p-4">
          <p className="text-[13px] font-semibold text-warn">
            Les touches F7, F8 et F9 ne répondent pas encore
          </p>
          <p className="mt-2 text-[12px] leading-relaxed text-ink-muted">
            macOS réserve les touches multimédia aux applications autorisées.
            Ouvre <span className="text-ink">Réglages Système → Confidentialité
            et sécurité → Accessibilité</span>, ajoute Onzer et coche-le. Les
            raccourcis de la fenêtre, eux, marchent déjà.
          </p>

          <button
            type="button"
            onClick={() => void ipc.retryMediaKeys().then(setMedia)}
            className="mt-3 rounded-full bg-elevated px-4 py-1.5 text-[12px] font-semibold text-ink transition-colors hover:bg-raised"
          >
            J&apos;ai autorisé, réessayer
          </button>

          {media.error != null && (
            <p className="mt-3 font-mono text-[11px] text-ink-faint">{media.error}</p>
          )}
        </div>
      )}

      <div className="mx-auto mt-10 max-w-3xl space-y-10">
        {GROUPS.map((group) => (
          <section key={group.title}>
            <h2 className="flex items-center gap-2.5 text-[15px] font-semibold text-ink">
              <span className="text-ink-faint">
                <Icon name={group.icon} size={17} />
              </span>
              {group.title}
            </h2>

            {group.note !== undefined && (
              <p className="mt-1.5 max-w-xl text-[12px] leading-relaxed text-ink-faint">
                {group.note}
              </p>
            )}

            <ul className="mt-4 divide-y divide-line">
              {group.shortcuts.map((shortcut) => (
                <li
                  key={shortcut.action}
                  className="flex items-center justify-between gap-6 py-2.5"
                >
                  <span className="min-w-0">
                    <span className="block text-[14px] text-ink">{shortcut.action}</span>
                    {shortcut.note !== undefined && (
                      <span className="block text-[12px] text-ink-faint">
                        {shortcut.note}
                      </span>
                    )}
                  </span>

                  <span className="flex shrink-0 items-center gap-1">
                    {shortcut.keys.map((key, index) => (
                      <span key={`${key}-${index}`} className="flex items-center gap-1">
                        {index > 0 && <span className="text-[11px] text-ink-faint">+</span>}
                        <Key label={key} />
                      </span>
                    ))}
                  </span>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </div>
  );
}

/**
 * Une touche.
 *
 * Dessinée comme une vraie — fond légèrement surélevé, coins arrondis, chasse
 * fixe. C'est ce qui la distingue d'un mot au milieu d'une phrase.
 */
function Key({ label }: { label: string }) {
  return (
    <kbd className="numerals inline-flex min-w-[1.9rem] items-center justify-center rounded-md bg-raised px-2 py-1 font-sans text-[12px] font-semibold text-ink-muted">
      {label}
    </kbd>
  );
}
