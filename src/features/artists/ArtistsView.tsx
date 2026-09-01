import { useEffect, useState } from "react";

import { Icon } from "@/components/Icon";
import { Artwork } from "@/features/library/Artwork";
import { ipc, type ArtistSummary } from "@/lib/ipc";

/**
 * Tous les artistes de la bibliothèque.
 *
 * # Pourquoi une page à part
 *
 * À l'échelle d'une bibliothèque personnelle, on se souvient de **qui** on veut
 * écouter bien avant de se souvenir de **quoi** : « mets du Népal » vient plus
 * naturellement que le titre exact d'un morceau.
 *
 * # Pourquoi des vignettes rondes
 *
 * C'est la convention qui distingue une personne d'un objet : un carré, c'est
 * un disque ; un rond, c'est quelqu'un. La forme évite d'avoir à écrire
 * « artiste » sur chaque case.
 *
 * L'image est la pochette de l'album dont on possède le plus de titres. Onzer
 * ne télécharge pas de portraits : cela supposerait d'aller les chercher chez
 * un tiers pour un ornement, et d'en gérer le cache.
 */
export function ArtistsView({
  onOpen,
}: {
  onOpen: (artist: ArtistSummary) => void;
}) {
  const [artists, setArtists] = useState<ArtistSummary[] | null>(null);
  const [query, setQuery] = useState("");

  useEffect(() => {
    void ipc.listArtists().then(setArtists).catch(() => setArtists([]));
  }, []);

  if (artists === null) {
    return <div className="px-6 py-16 text-sm text-ink-faint">Chargement…</div>;
  }

  const needle = query.trim().toLowerCase();
  const shown =
    needle === ""
      ? artists
      : artists.filter((artist) => artist.name.toLowerCase().includes(needle));

  return (
    <div className="px-6 pb-10 pt-6">
      <div className="flex flex-wrap items-baseline justify-between gap-4">
        <div>
          <h1 className="display-xl text-[clamp(1.75rem,4vw,2.75rem)] text-ink">
            Artistes
          </h1>
          <p className="mt-2 text-[13px] text-ink-muted">
            {artists.length} artiste{artists.length > 1 ? "s" : ""} dans ta
            bibliothèque
          </p>
        </div>

        {/* Filtre local, et non la recherche générale : on cherche ici parmi
            des noms qu'on possède, pas dans tout le catalogue. */}
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
          Aucun artiste ne correspond à « {query.trim()} ».
        </p>
      ) : (
        <div
          className="mt-6 grid gap-4"
          style={{ gridTemplateColumns: "repeat(auto-fill, minmax(10rem, 1fr))" }}
        >
          {shown.map((artist) => (
            <button
              key={artist.id}
              type="button"
              onClick={() => onOpen(artist)}
              className="group rounded-lg p-3 text-left transition-colors hover:bg-surface"
            >
              <div className="relative">
                <Artwork
                  hash={artist.coverHash}
                  className="aspect-square w-full rounded-full"
                />

                <span className="absolute bottom-1 right-1 flex h-10 w-10 items-center justify-center rounded-full bg-ink text-base opacity-0 shadow-xl shadow-black/40 transition-all duration-200 translate-y-2 group-hover:translate-y-0 group-hover:opacity-100">
                  <span className="translate-x-[1px]">
                    <Icon name="play" size={16} />
                  </span>
                </span>
              </div>

              <p className="mt-3 truncate text-[15px] font-semibold text-ink">
                {artist.name}
              </p>
              <p className="numerals mt-0.5 text-[12px] text-ink-faint">
                {artist.trackCount} titre{artist.trackCount > 1 ? "s" : ""}
              </p>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
