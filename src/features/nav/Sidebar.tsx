import { useState } from "react";

import { Artwork } from "@/features/library/Artwork";
import { Icon } from "@/components/Icon";
import type { PlaylistSummary } from "@/lib/ipc";

/** Ce que l'application peut afficher dans sa zone principale. */
export type Route =
  | { kind: "home" }
  | { kind: "library" }
  | { kind: "loved" }
  | { kind: "stats" }
  /** Une playlist produite par le moteur. Absente de la barre latérale. */
  | { kind: "generated" }
  | { kind: "playlist"; id: number; name: string };

export function routeKey(route: Route): string {
  return route.kind === "playlist" ? `playlist:${route.id}` : route.kind;
}

interface SidebarProps {
  route: Route;
  onNavigate: (route: Route) => void;
  /**
   * Les playlists sont chargées par la coquille, pas ici.
   *
   * Le menu « Ajouter à » des lignes de morceaux a besoin de la même liste :
   * la charger deux fois garantirait qu'un ajout se voie d'un côté et pas de
   * l'autre.
   */
  playlists: PlaylistSummary[];
  onCreatePlaylist: (name: string) => void;
}

/**
 * Barre latérale : la colonne vertébrale de la navigation.
 *
 * # Pourquoi elle affiche des noms et pas seulement des pochettes
 *
 * Un simple rail d'icônes est élégant sur une capture d'écran, et pénible à
 * l'usage : rien ne distingue deux playlists dont les pochettes se ressemblent,
 * et il faut survoler chaque vignette pour retrouver la bonne. La largeur ici
 * n'est pas de l'espace perdu, c'est de la mémoire en moins à fournir.
 */
export function Sidebar({
  route,
  onNavigate,
  playlists,
  onCreatePlaylist,
}: SidebarProps) {
  const [draft, setDraft] = useState<string | null>(null);
  const active = routeKey(route);

  function create() {
    const name = draft?.trim() ?? "";
    setDraft(null);
    if (name !== "") onCreatePlaylist(name);
  }

  return (
    <aside className="flex w-60 shrink-0 flex-col gap-2">
      <nav className="rounded-xl bg-surface p-2">
        <NavItem
          icon="home"
          label="Accueil"
          active={active === "home"}
          onClick={() => onNavigate({ kind: "home" })}
        />
        <NavItem
          icon="library"
          label="Bibliothèque"
          active={active === "library" || active === "generated"}
          onClick={() => onNavigate({ kind: "library" })}
        />
        <NavItem
          icon="stats"
          label="Statistiques"
          active={active === "stats"}
          onClick={() => onNavigate({ kind: "stats" })}
        />
      </nav>

      <div className="flex min-h-0 flex-1 flex-col rounded-xl bg-surface">
        <div className="flex items-center justify-between px-4 pb-2 pt-3">
          <span className="text-xs font-semibold uppercase tracking-[0.14em] text-ink-faint">
            Ma collection
          </span>
          <button
            type="button"
            title="Nouvelle playlist"
            aria-label="Nouvelle playlist"
            onClick={() => setDraft("")}
            className="flex h-7 w-7 items-center justify-center rounded-full text-ink-muted transition-colors hover:bg-elevated hover:text-ink"
          >
            <Icon name="plus" size={17} />
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
          <LovedItem
            active={active === "loved"}
            onClick={() => onNavigate({ kind: "loved" })}
          />

          {draft !== null && (
            <div className="mt-1 px-1">
              <input
                autoFocus
                value={draft}
                placeholder="Nom de la playlist"
                onChange={(event) => setDraft(event.target.value)}
                onBlur={create}
                onKeyDown={(event) => {
                  if (event.key === "Enter") create();
                  if (event.key === "Escape") setDraft(null);
                }}
                className="w-full rounded-lg border border-line bg-base px-3 py-2 text-sm text-ink placeholder:text-ink-faint focus:border-accent focus:outline-none"
              />
            </div>
          )}

          {playlists.map((playlist) => (
            <PlaylistItem
              key={playlist.id}
              playlist={playlist}
              active={active === `playlist:${playlist.id}`}
              onClick={() =>
                onNavigate({ kind: "playlist", id: playlist.id, name: playlist.name })
              }
            />
          ))}

          {playlists.length === 0 && draft === null && (
            <p className="px-3 py-6 text-xs leading-relaxed text-ink-faint">
              Aucune playlist. Crée-en une avec le +, ou ajoute un morceau depuis
              le menu de sa ligne.
            </p>
          )}
        </div>
      </div>
    </aside>
  );
}

function NavItem({
  icon,
  label,
  active,
  onClick,
}: {
  icon: "home" | "library" | "stats";
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex w-full items-center gap-4 rounded-lg px-3 py-2.5 text-left text-sm font-semibold transition-colors ${
        active ? "text-ink" : "text-ink-muted hover:text-ink"
      }`}
    >
      <Icon name={icon} size={20} />
      {label}
    </button>
  );
}

/** Les favoris sont épinglés en tête : c'est la playlist qu'on ouvre le plus. */
function LovedItem({ active, onClick }: { active: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex w-full items-center gap-3 rounded-lg p-2 text-left transition-colors ${
        active ? "bg-elevated" : "hover:bg-elevated/60"
      }`}
    >
      <span className="flex h-11 w-11 shrink-0 items-center justify-center rounded bg-gradient-to-br from-accent to-accent-soft">
        <Icon name="heartFilled" size={18} className="text-base" />
      </span>
      <span className="min-w-0">
        <span className="block truncate text-sm font-semibold text-ink">
          Titres likés
        </span>
        <span className="block text-xs text-ink-faint">Playlist</span>
      </span>
    </button>
  );
}

function PlaylistItem({
  playlist,
  active,
  onClick,
}: {
  playlist: PlaylistSummary;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex w-full items-center gap-3 rounded-lg p-2 text-left transition-colors ${
        active ? "bg-elevated" : "hover:bg-elevated/60"
      }`}
    >
      <Artwork hash={playlist.coverHash} className="h-11 w-11 rounded" />
      <span className="min-w-0">
        <span className="block truncate text-sm font-semibold text-ink">
          {playlist.name}
        </span>
        <span className="block truncate text-xs text-ink-faint">
          Playlist · {playlist.trackCount} titre{playlist.trackCount > 1 ? "s" : ""}
        </span>
      </span>
    </button>
  );
}
