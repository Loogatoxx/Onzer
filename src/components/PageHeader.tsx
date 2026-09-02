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
  /** Quand il est fourni, la pochette devient remplaçable au clic. */
  onPickCover?: () => void;
  /** Quand elle est fournie, la ligne de description devient modifiable. */
  description?: string | null;
  onDescription?: (description: string) => void;
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
  onPickCover,
  description,
  onDescription,
}: PageHeaderProps) {
  const [draft, setDraft] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);

  function commit() {
    const name = draft?.trim() ?? "";
    if (name !== "" && name !== title) onRename?.(name);
    setDraft(null);
  }

  return (
    <header className="relative bg-gradient-to-b from-elevated/70 to-transparent px-6 pb-4 pt-6">
      {/* Sur un écran étroit, la pochette et le titre côte à côte laissent au
          titre une colonne de cent pixels. Ils s'empilent donc, pochette
          centrée au-dessus — la disposition que tous les lecteurs de téléphone
          ont fini par adopter, pour la même raison. */}
      <div className="flex flex-col items-center gap-4 text-center sm:flex-row sm:items-end sm:gap-6 sm:text-left">
        <div className="group relative shrink-0 overflow-hidden rounded-lg shadow-2xl shadow-black/50">
          {cover}

          {/* La pochette ne se remplace que là où elle appartient à
              l'utilisateur : une playlist. Un album garde la sienne. */}
          {onPickCover !== undefined && (
            <button
              type="button"
              onClick={onPickCover}
              className="absolute inset-0 flex flex-col items-center justify-center gap-1.5 bg-base/70 text-ink opacity-0 transition-opacity hover:opacity-100"
            >
              <Icon name="pencil" size={22} />
              <span className="text-[12px] font-semibold">Changer l'image</span>
            </button>
          )}
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

          {onDescription !== undefined &&
            (note === null ? (
              <button
                type="button"
                onClick={() => setNote(description ?? "")}
                className={`mt-3 block max-w-xl text-left text-[13px] leading-relaxed transition-colors hover:text-ink ${
                  description == null || description === ""
                    ? "text-ink-faint"
                    : "text-ink-muted"
                }`}
              >
                {description == null || description === ""
                  ? "Ajouter une description"
                  : description}
              </button>
            ) : (
              <input
                autoFocus
                value={note}
                onChange={(event) => setNote(event.target.value)}
                onBlur={() => {
                  onDescription(note);
                  setNote(null);
                }}
                onKeyDown={(event) => {
                  if (event.key === "Enter") event.currentTarget.blur();
                  if (event.key === "Escape") setNote(null);
                }}
                placeholder="Une phrase pour t'en souvenir"
                className="mt-3 w-full max-w-xl rounded-md bg-elevated px-2 py-1 text-[13px] text-ink placeholder:text-ink-faint focus:outline-none"
              />
            ))}

          <p className="mt-4 flex flex-wrap items-center gap-x-2 text-[13px] text-ink-muted">
            {meta}
          </p>
        </div>
      </div>

      <div className="mt-6 flex items-center justify-center gap-4 sm:justify-start">
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
  size = "h-40 w-40 sm:h-52 sm:w-52",
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
