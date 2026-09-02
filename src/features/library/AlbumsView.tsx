import { useEffect, useState } from "react";

import { Artwork } from "@/features/library/Artwork";
import { Icon } from "@/components/Icon";
import { ipc, type AlbumSummary } from "@/lib/ipc";

/**
 * Tous les albums, en grille de pochettes.
 *
 * # Pourquoi une grille et pas une liste
 *
 * Un album se reconnaît à sa pochette bien avant son titre — c'est la seule
 * partie de la bibliothèque où l'image porte plus d'information que le texte.
 * Une liste de noms d'albums obligerait à lire là où l'œil sait déjà.
 */
export function AlbumsView({
  onOpen,
}: {
  onOpen: (album: AlbumSummary) => void;
}) {
  const [albums, setAlbums] = useState<AlbumSummary[] | null>(null);
  const [query, setQuery] = useState("");

  useEffect(() => {
    void ipc.listAlbums().then(setAlbums).catch(() => setAlbums([]));
  }, []);

  if (albums === null) {
    return <p className="px-6 py-16 text-sm text-ink-faint">Lecture des albums…</p>;
  }

  const terme = query.trim().toLocaleLowerCase("fr");
  const shown =
    terme === ""
      ? albums
      : albums.filter(
          (album) =>
            album.title.toLocaleLowerCase("fr").includes(terme)
            || (album.artist ?? "").toLocaleLowerCase("fr").includes(terme),
        );

  return (
    <div className="px-6 pb-10">
      <div className="flex flex-wrap items-end justify-between gap-4 pt-2">
        <div>
          <p className="text-xs font-semibold uppercase tracking-[0.16em] text-ink-muted">
            Ta bibliothèque
          </p>
          <h1 className="display-xl mt-1 text-[clamp(1.75rem,5vw,2.75rem)] text-ink">
            {albums.length} album{albums.length > 1 ? "s" : ""}
          </h1>
        </div>

        <div className="relative">
          <span className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-ink-faint">
            <Icon name="search" size={15} />
          </span>
          <input
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Filtrer"
            className="h-9 w-52 rounded-full bg-elevated pl-9 pr-3 text-[13px] text-ink placeholder:text-ink-faint focus:outline focus:outline-1 focus:outline-ink-faint"
          />
        </div>
      </div>

      {shown.length === 0 ? (
        <p className="py-16 text-center text-sm text-ink-muted">
          Aucun album ne correspond à « {query.trim()} ».
        </p>
      ) : (
        <div
          className="mt-6 grid gap-4"
          style={{ gridTemplateColumns: "repeat(auto-fill, minmax(min(9rem, 45%), 1fr))" }}
        >
          {shown.map((album) => (
            <button
              key={album.id}
              type="button"
              onClick={() => onOpen(album)}
              className="group rounded-lg p-2 text-left transition-colors hover:bg-surface"
            >
              <Artwork
                hash={album.artworkHash}
                className="aspect-square w-full rounded-md"
              />

              <p className="mt-2.5 truncate text-[14px] font-medium text-ink">
                {album.title}
              </p>
              <p className="truncate text-[12px] text-ink-faint">
                {album.artist ?? "Artiste inconnu"}
                {album.year !== null && ` · ${album.year}`}
              </p>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
