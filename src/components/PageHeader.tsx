import { useState } from "react";

import { Icon, type IconName } from "@/components/Icon";

interface PageHeaderProps {
  eyebrow: string;
  title: string;
  /** Ligne d'information sous le titre : nombre de titres, durée totale… */
  meta: React.ReactNode;
  /** Pochette, mosaïque ou tuile d'icône. */
  cover: React.ReactNode;
  /** `null` quand il n'y a rien à lire (bibliothèque vide). */
  onPlay: (() => void) | null;
  onShuffle?: () => void;
  /**
   * Quand il est fourni, le titre devient modifiable au clic.
   *
   * Renommer là où le nom s'affiche évite une boîte de dialogue : on tape par
   * dessus, on valide, c'est fini.
   */
  onRename?: (name: string) => void;
  /** Boutons additionnels, placés après les commandes de lecture. */
  extra?: React.ReactNode;
}

/**
 * En-tête d'une page de collection.
 *
 * # Le titre est l'image
 *
 * Un titre en 4,5 rem avec un crénage très serré n'est plus du texte : c'est un
 * bloc graphique. C'est ce qui permet de se passer d'illustration décorative et
 * de garder une interface sobre tout en donnant à chaque page une identité
 * immédiate.
 *
 * Le fond est un dégradé **gris sur gris**, à peine perceptible. Il ne sert
 * qu'à décoller l'en-tête de la liste qui suit ; toute couleur ici entrerait en
 * concurrence avec les pochettes, qui sont le seul endroit où la couleur a sa
 * place.
 */
export function PageHeader({
  eyebrow,
  title,
  meta,
  cover,
  onPlay,
  onShuffle,
  onRename,
  extra,
}: PageHeaderProps) {
  const [draft, setDraft] = useState<string | null>(null);

  function commit() {
    const name = draft?.trim() ?? "";
    if (name !== "" && name !== title) onRename?.(name);
    setDraft(null);
  }

  return (
    <header className="relative bg-gradient-to-b from-elevated/70 to-transparent px-6 pb-4 pt-6">
      <div className="flex items-end gap-6">
        <div className="shrink-0 overflow-hidden rounded-lg shadow-2xl shadow-black/50">
          {cover}
        </div>

        <div className="min-w-0 flex-1 pb-2">
          <p className="text-xs font-semibold uppercase tracking-[0.16em] text-ink-muted">
            {eyebrow}
          </p>

          {draft === null ? (
            <h1
              onClick={() => {
                if (onRename !== undefined) setDraft(title);
              }}
              title={onRename === undefined ? undefined : "Cliquer pour renommer"}
              className={`display-xl mt-2 break-words text-[clamp(2rem,5.5vw,4.25rem)] text-ink ${
                onRename === undefined ? "" : "cursor-text"
              }`}
            >
              {title}
            </h1>
          ) : (
            <input
              autoFocus
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              onBlur={commit}
              onKeyDown={(event) => {
                if (event.key === "Enter") commit();
                if (event.key === "Escape") setDraft(null);
              }}
              className="display-xl mt-2 w-full rounded-md bg-elevated px-2 text-[clamp(2rem,5.5vw,4.25rem)] text-ink outline-none"
            />
          )}

          <p className="mt-4 flex flex-wrap items-center gap-x-2 text-[13px] text-ink-muted">
            {meta}
          </p>
        </div>
      </div>

      <div className="mt-6 flex items-center gap-4">
        {onPlay !== null && (
          <button
            type="button"
            aria-label="Lire"
            title="Lire"
            onClick={onPlay}
            className="flex h-14 w-14 items-center justify-center rounded-full bg-ink text-base transition-transform duration-150 hover:scale-[1.04] active:scale-95"
          >
            <span className="translate-x-[1px]">
              <Icon name="play" size={22} />
            </span>
          </button>
        )}

        {onShuffle !== undefined && (
          <HeaderAction name="shuffle" label="Lire dans le désordre" onClick={onShuffle} />
        )}

        {extra}
      </div>
    </header>
  );
}

/**
 * Bouton secondaire d'en-tête.
 *
 * Plus grand que ceux de la barre de lecture : à côté d'un disque de 56 px, une
 * icône de 18 px paraîtrait accidentelle.
 */
export function HeaderAction({
  name,
  label,
  active = false,
  onClick,
}: {
  name: IconName;
  label: string;
  active?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active}
      onClick={onClick}
      className={`flex h-10 w-10 items-center justify-center rounded-full transition-colors ${
        active ? "text-accent" : "text-ink-muted hover:text-ink"
      }`}
    >
      <Icon name={name} size={24} />
    </button>
  );
}

/** Tuile de remplacement quand la collection n'a pas de pochette. */
export function CoverTile({
  name,
  size = "h-52 w-52",
}: {
  name: IconName;
  size?: string;
}) {
  return (
    <div
      className={`flex items-center justify-center bg-gradient-to-br from-raised to-elevated ${size}`}
    >
      <Icon name={name} size={56} className="text-ink-muted" />
    </div>
  );
}
