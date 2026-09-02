import { Icon, type IconName } from "@/components/Icon";
import type { PlaylistSummary } from "@/lib/ipc";
import type { Route } from "@/features/nav/Sidebar";

/**
 * Le reste de l'application, sur écran étroit.
 *
 * # Pourquoi une page et non des onglets supplémentaires
 *
 * Cinq onglets tiennent en bas d'un téléphone ; huit n'y tiennent pas, et
 * réduire les libellés pour les faire entrer revient à les rendre illisibles.
 * Les destinations qu'on visite **de temps en temps** — les statistiques, les
 * favoris, les playlists, les réglages — se rangent donc derrière une porte,
 * pendant que les quatre qu'on ouvre chaque jour gardent leur place.
 *
 * C'est le même arbitrage que la barre latérale du bureau, qui met la
 * collection sous la navigation plutôt qu'à côté.
 */
export function MoreView({
  playlists,
  onNavigate,
  onCreatePlaylist,
}: {
  playlists: PlaylistSummary[];
  onNavigate: (route: Route) => void;
  onCreatePlaylist: () => void;
}) {
  const destinations: { icon: IconName; label: string; hint: string; route: Route }[] = [
    {
      icon: "heart",
      label: "Titres likés",
      hint: "Ce que tu as aimé",
      route: { kind: "loved" },
    },
    {
      icon: "stats",
      label: "Statistiques",
      hint: "Ton année en chiffres",
      route: { kind: "stats" },
    },
    {
      icon: "sparkle",
      label: "Ce qui me manque",
      hint: "Comparer avec une playlist",
      route: { kind: "sync" },
    },
    {
      icon: "settings",
      label: "Réglages",
      hint: "Complétion, identification, remise à zéro",
      route: { kind: "settings" },
    },
  ];

  return (
    <div className="px-6 pb-10 pt-6">
      <h1 className="display-xl text-[clamp(1.75rem,7vw,2.5rem)] text-ink">Plus</h1>

      <ul className="mt-6 space-y-1">
        {destinations.map((destination) => (
          <li key={destination.label}>
            <button
              type="button"
              onClick={() => onNavigate(destination.route)}
              className="flex w-full items-center gap-4 rounded-xl px-3 py-3 text-left transition-colors hover:bg-surface"
            >
              <span className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-elevated text-ink-muted">
                <Icon name={destination.icon} size={19} />
              </span>

              <span className="min-w-0 flex-1">
                <span className="block truncate text-[15px] font-medium text-ink">
                  {destination.label}
                </span>
                <span className="block truncate text-[12px] text-ink-faint">
                  {destination.hint}
                </span>
              </span>

              <span className="shrink-0 text-ink-faint">
                <Icon name="chevronRight" size={16} />
              </span>
            </button>
          </li>
        ))}
      </ul>

      <div className="mt-9 flex items-center justify-between">
        <h2 className="display text-[clamp(1.15rem,4.5vw,1.4rem)] text-ink">Mes playlists</h2>

        <button
          type="button"
          aria-label="Nouvelle playlist"
          onClick={onCreatePlaylist}
          className="flex h-9 w-9 items-center justify-center rounded-full bg-elevated text-ink-muted transition-colors hover:text-ink"
        >
          <Icon name="plus" size={18} />
        </button>
      </div>

      {playlists.length === 0 ? (
        <p className="mt-3 text-[13px] leading-relaxed text-ink-faint">
          Aucune playlist. Crée-en une avec le +, ou ajoute un morceau depuis le
          menu de sa ligne.
        </p>
      ) : (
        <ul className="mt-3 space-y-1">
          {playlists.map((playlist) => (
            <li key={playlist.id}>
              <button
                type="button"
                onClick={() =>
                  onNavigate({ kind: "playlist", id: playlist.id, name: playlist.name })
                }
                className="flex w-full items-center gap-4 rounded-xl px-3 py-2.5 text-left transition-colors hover:bg-surface"
              >
                <span className="flex h-11 w-11 shrink-0 items-center justify-center rounded-lg bg-elevated text-ink-faint">
                  <Icon name="playlist" size={17} />
                </span>

                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[15px] text-ink">{playlist.name}</span>
                  <span className="numerals block text-[12px] text-ink-faint">
                    {playlist.trackCount} titre{playlist.trackCount > 1 ? "s" : ""}
                  </span>
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
