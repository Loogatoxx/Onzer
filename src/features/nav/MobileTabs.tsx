import { Icon, type IconName } from "@/components/Icon";
import type { Route } from "@/features/nav/Sidebar";

/**
 * Barre d'onglets du bas, sur écran étroit.
 *
 * # Pourquoi en bas et non sur le côté
 *
 * La barre latérale fait 240 px : sur un écran de six pouces, c'est la moitié
 * de la largeur pour de la navigation. En bas, elle ne coûte que 56 px de
 * hauteur — et surtout elle tombe **sous le pouce**, seul endroit qu'on
 * atteint sans changer la prise du téléphone.
 *
 * # Pourquoi ces cinq-là
 *
 * Ce sont les destinations qu'on vise directement, sans passer par une autre.
 * Les playlists, les albums, un artiste précis se rejoignent depuis la
 * bibliothèque ; les mettre ici obligerait à choisir lesquels, et ce choix ne
 * nous appartient pas.
 */
const ONGLETS: { key: string; icon: IconName; label: string; route: Route }[] = [
  { key: "home", icon: "home", label: "Accueil", route: { kind: "home" } },
  { key: "search", icon: "search", label: "Recherche", route: { kind: "library" } },
  { key: "library", icon: "library", label: "Bibliothèque", route: { kind: "library" } },
  { key: "artists", icon: "artist", label: "Artistes", route: { kind: "artists" } },
  { key: "more", icon: "more", label: "Plus", route: { kind: "more" } },
];

export function MobileTabs({
  active,
  onNavigate,
  onSearch,
}: {
  /** Clé de l'onglet en cours, telle que `ONGLETS` les nomme. */
  active: string;
  onNavigate: (route: Route) => void;
  /** La recherche n'est pas une page mais un mode : elle a son propre chemin. */
  onSearch: () => void;
}) {
  return (
    <nav
      aria-label="Navigation principale"
      className="flex shrink-0 items-stretch border-t border-line bg-surface pb-[env(safe-area-inset-bottom)]"
    >
      {ONGLETS.map((onglet) => {
        const courant = onglet.key === active;

        return (
          <button
            key={onglet.key}
            type="button"
            aria-current={courant ? "page" : undefined}
            onClick={() => {
              if (onglet.key === "search") onSearch();
              else onNavigate(onglet.route);
            }}
            className={`flex flex-1 flex-col items-center gap-1 py-2 transition-colors ${
              courant ? "text-ink" : "text-ink-faint"
            }`}
          >
            <Icon name={onglet.icon} size={21} />
            <span className="text-[10px] font-medium leading-none">{onglet.label}</span>
          </button>
        );
      })}
    </nav>
  );
}
