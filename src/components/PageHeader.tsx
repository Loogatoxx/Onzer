import { useState } from "react";

import { Icon, type IconName } from "@/components/Icon";

interface PageHeaderProps {
  eyebrow: string;
  /**
   * Quand elle est fournie, la ligne du dessus devient un lien.
   *
   * # Pourquoi elle ne l'est pas toujours
   *
   * L'*eyebrow* porte d'ordinaire une catégorie — « Playlist », « Artiste »,
   * « Ambiance » — qui ne mène nulle part. Sur la page d'un album, elle porte
   * un **nom propre**, typographié comme une étiquette : le seul endroit de
   * l'application où le nom d'un artiste ne conduisait pas à l'artiste, alors
   * qu'il y conduit dans chaque ligne du tableau juste en dessous.
   */
  onEyebrow?: () => void;
  title: string;
  /** Ligne d'information sous le titre : nombre de titres, durée totale… */
  meta: React.ReactNode;
  /** Pochette, mosaïque ou tuile d'icône. */
  cover: React.ReactNode;
  /** `null` quand il n'y a rien à lire (bibliothèque vide). */
  onPlay: (() => void) | null;
  onShuffle?: () => void;
  /**
   * Ajoute toute la collection à la file d'attente.
   *
   * # Pourquoi ici, à côté de la lecture
   *
   * « Ajouter à la file » existait morceau par morceau, dans le menu d'une
   * ligne. Mettre un album de quatorze titres à la suite demandait donc
   * quatorze allers-retours dans un menu — ou d'accepter que la lecture en
   * cours soit remplacée, ce qui n'est pas la même chose du tout.
   *
   * Le bouton se tient là où l'on regarde déjà pour lancer : ▶ remplace la
   * file, celui-ci la prolonge.
   */
  onEnqueue?: () => void;
  /**
   * Fait défiler la liste jusqu'au morceau en cours.
   *
   * Absent quand il n'y est pas : proposer d'aller quelque part qui n'existe
   * pas dans cette liste ne mène nulle part, littéralement.
   */
  onLocate?: () => void;
  /** Ouvre la sélection multiple sur cette liste. */
  onSelectMode?: () => void;
  /**
   * Quand il est fourni, le titre devient modifiable au clic.
   *
   * Renommer là où le nom s'affiche évite une boîte de dialogue : on tape par
   * dessus, on valide, c'est fini.
   */
  onRename?: (name: string) => void;
  /** Boutons additionnels, placés après les commandes de lecture. */
  extra?: React.ReactNode;
  /**
   * Filtre la liste affichée, sans quitter la page.
   *
   * # Pourquoi il ne remplace pas la recherche générale
   *
   * Celle-ci cherche dans deux mille morceaux et remplace la page. Là, on est
   * déjà quelque part — dans une playlist de cinq cents titres, dans la
   * discographie d'un artiste — et l'on veut **écarter** ce qui ne correspond
   * pas, pas partir ailleurs. Ce sont deux gestes différents ; en confondre un
   * avec l'autre oblige à revenir sur ses pas.
   */
  filtre?: string;
  onFiltre?: (texte: string) => void;
  /**
   * La couleur du voile, tirée de la pochette.
   *
   * `null` laisse le gris : c'est le cas d'une pochette en noir et blanc, et
   * c'est le bon — la charte dit que les couleurs viennent des albums, pas
   * qu'il faut en inventer à ceux qui n'en ont pas.
   */
  teinte?: string | null;
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
  onEyebrow,
  title,
  meta,
  cover,
  onPlay,
  onShuffle,
  onEnqueue,
  onLocate,
  onSelectMode,
  onRename,
  extra,
  filtre,
  onFiltre,
  teinte,
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

  // Plus de `grain` : la trame ne sert qu'à casser un dégradé, et appliquée à
  // un aplat elle **est** le dégradé — son masque s'éteint vers le bas.
  return (
    <header
      className="fondu-tete relative px-6 pb-4 pt-6"
      // La transition adoucit l'arrivée : la page s'affiche en gris, puis
      // prend sa couleur quand la pochette a fini d'être lue.
      style={
        teinte == null
          ? undefined
          : ({
              "--voile": teinte,
              transition: "background-image 320ms var(--ease-out-soft)",
            } as React.CSSProperties)
      }
    >
      {/* Sur un écran étroit, la pochette et le titre côte à côte laissent au
          titre une colonne de cent pixels. Ils s'empilent donc, pochette
          centrée au-dessus — la disposition que tous les lecteurs de téléphone
          ont fini par adopter, pour la même raison. */}
      <div className="flex flex-col items-center gap-4 text-center sm:flex-row sm:items-end sm:gap-6 sm:text-left">
        {/* # Pourquoi la pochette n'a plus d'ombre portée

              `shadow-2xl` est un dégradé noir de cinquante pixels de flou. Posé
              sur un fond déjà quasi noir, il ne peut traverser que trois ou
              quatre niveaux de luminance : il dessine donc des **anneaux** au
              lieu d'une ombre, et c'est exactement l'escalier qu'on voyait
              derrière la grande pochette.

              Une ombre a besoin de clair pour assombrir. Ici il n'y en a pas :
              elle ne portait rien, et coûtait ses artefacts. La pochette se
              détache par son coin arrondi et par sa propre image. */}
        <div className="group relative shrink-0 overflow-hidden rounded-lg">
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
          {onEyebrow === undefined ? (
            <p className="text-xs font-semibold uppercase tracking-[0.16em] text-ink-muted">
              {eyebrow}
            </p>
          ) : (
            <button
              type="button"
              onClick={onEyebrow}
              className="text-xs font-semibold uppercase tracking-[0.16em] text-ink-muted transition-colors hover:text-ink hover:underline"
            >
              {eyebrow}
            </button>
          )}

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

          {/* `justify-center` sur mobile suit le titre, lui-même centré : un
              nom qui court sur trois lignes laissait les compteurs collés à
              gauche, sous un titre centré — la composition partait de travers.
              `break-words` sur le titre empêche par ailleurs un nom sans
              espace de déborder de l'écran. */}
          <p className="mt-4 flex flex-wrap items-center justify-center gap-x-2 text-[13px] text-ink-muted sm:justify-start">
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

        {onEnqueue !== undefined && (
          <HeaderAction
            name="queue"
            label="Ajouter à la file d'attente"
            onClick={onEnqueue}
          />
        )}

        {onLocate !== undefined && (
          <HeaderAction
            name="locate"
            label="Aller au morceau en cours"
            onClick={onLocate}
          />
        )}

        {onSelectMode !== undefined && (
          <HeaderAction
            name="check"
            label="Sélectionner des morceaux"
            onClick={onSelectMode}
          />
        )}

        {extra}

        {onFiltre !== undefined && (
          <FiltreListe valeur={filtre ?? ""} onChange={onFiltre} />
        )}
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
      className={`pression flex h-10 w-10 items-center justify-center rounded-full ${
        active ? "text-accent" : "text-ink-muted hover:text-ink"
      }`}
    >
      <Icon name={name} size={24} />
    </button>
  );
}

/**
 * Le champ qui écarte.
 *
 * Replié, c'est une loupe : un champ toujours ouvert dirait « il faut chercher
 * ici », alors que la plupart du temps on veut simplement lire la liste.
 * Déplié, il prend la place — et se referme dès qu'il est vide et qu'on le
 * quitte, pour ne pas laisser un filtre invisible en travers de la page.
 */
function FiltreListe({
  valeur,
  onChange,
}: {
  valeur: string;
  onChange: (texte: string) => void;
}) {
  const [ouvert, setOuvert] = useState(valeur !== "");

  if (!ouvert) {
    return (
      <button
        type="button"
        aria-label="Filtrer cette liste"
        title="Filtrer cette liste"
        onClick={() => setOuvert(true)}
        className="pression ml-auto flex h-10 w-10 items-center justify-center rounded-full text-ink-muted hover:text-ink"
      >
        <Icon name="search" size={20} />
      </button>
    );
  }

  return (
    <div className="ml-auto flex items-center gap-1.5 rounded-full bg-elevated px-3 py-1.5">
      <span className="shrink-0 text-ink-faint">
        <Icon name="search" size={15} />
      </span>
      <input
        autoFocus
        value={valeur}
        onChange={(event) => onChange(event.target.value)}
        onBlur={() => {
          if (valeur === "") setOuvert(false);
        }}
        onKeyDown={(event) => {
          if (event.key !== "Escape") return;
          onChange("");
          setOuvert(false);
        }}
        placeholder="Dans cette liste"
        className="w-32 bg-transparent text-[13px] text-ink placeholder:text-ink-faint focus:outline-none sm:w-44"
      />
      {valeur !== "" && (
        <button
          type="button"
          aria-label="Effacer le filtre"
          onClick={() => {
            onChange("");
            setOuvert(false);
          }}
          className="pression shrink-0 text-ink-faint hover:text-ink"
        >
          <Icon name="close" size={14} />
        </button>
      )}
    </div>
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
      className={`flex items-center justify-center bg-raised ${size}`}
    >
      <Icon name={name} size={56} className="text-ink-muted" />
    </div>
  );
}
