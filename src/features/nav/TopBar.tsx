import { Icon, IconButton } from "@/components/Icon";

interface TopBarProps {
  query: string;
  onQuery: (query: string) => void;
  canGoBack: boolean;
  canGoForward: boolean;
  onBack: () => void;
  onForward: () => void;
  importing: boolean;
  onImport: () => void;
  panelOpen: boolean;
  onTogglePanel: () => void;
}

/**
 * Barre supérieure : navigation, recherche, import.
 *
 * # Pourquoi la recherche est ici et non dans la barre latérale
 *
 * Chercher, c'est changer ce qu'affiche la zone principale. Placer le champ
 * juste au-dessus de cette zone rend le lien évident ; dans la colonne de
 * gauche, il ressemblerait à un filtre de la navigation.
 *
 * La barre est **translucide et floutée** : quand la page défile, le contenu
 * passe dessous sans jamais donner l'impression de la traverser.
 */
export function TopBar(props: TopBarProps) {
  return (
    <header className="drag-region sticky top-0 z-20 flex shrink-0 items-center gap-3 bg-surface/80 px-4 py-3 backdrop-blur-xl">
      <div className="no-drag flex shrink-0 items-center gap-1">
        <NavArrow
          name="chevronLeft"
          label="Précédent"
          disabled={!props.canGoBack}
          onClick={props.onBack}
        />
        <NavArrow
          name="chevronRight"
          label="Suivant"
          disabled={!props.canGoForward}
          onClick={props.onForward}
        />
      </div>

      <div className="no-drag relative min-w-0 flex-1 max-w-md">
        <span className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-ink-faint">
          <Icon name="search" size={17} />
        </span>

        <input
          type="search"
          value={props.query}
          onChange={(event) => props.onQuery(event.target.value)}
          placeholder="Titre, artiste ou album"
          className="h-10 w-full rounded-full bg-elevated pl-10 pr-9 text-sm text-ink placeholder:text-ink-faint focus:outline focus:outline-1 focus:outline-ink-faint"
        />

        {props.query !== "" && (
          <button
            type="button"
            aria-label="Effacer la recherche"
            onClick={() => props.onQuery("")}
            className="absolute right-2.5 top-1/2 -translate-y-1/2 text-ink-faint transition-colors hover:text-ink"
          >
            <Icon name="close" size={15} />
          </button>
        )}
      </div>

      <div className="no-drag ml-auto flex shrink-0 items-center gap-2">
        <button
          type="button"
          disabled={props.importing}
          onClick={props.onImport}
          className="flex items-center gap-2 rounded-full bg-elevated px-4 py-2 text-[13px] font-semibold text-ink transition-colors hover:bg-raised disabled:opacity-40"
        >
          <Icon name="folder" size={16} />
          {props.importing ? "Import en cours…" : "Importer"}
        </button>

        <IconButton
          name="panelRight"
          label={props.panelOpen ? "Masquer le panneau" : "Afficher le panneau"}
          active={props.panelOpen}
          onClick={props.onTogglePanel}
        />
      </div>
    </header>
  );
}

/**
 * Flèche de navigation.
 *
 * Elle reste affichée même sans destination, en gris très effacé : la faire
 * disparaître décalerait tout le reste de la barre à chaque navigation.
 */
function NavArrow({
  name,
  label,
  disabled,
  onClick,
}: {
  name: "chevronLeft" | "chevronRight";
  label: string;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      className="flex h-8 w-8 items-center justify-center rounded-full bg-base/60 text-ink-muted transition-colors hover:text-ink disabled:text-ink-faint/40 disabled:hover:text-ink-faint/40"
    >
      <Icon name={name} size={17} />
    </button>
  );
}
