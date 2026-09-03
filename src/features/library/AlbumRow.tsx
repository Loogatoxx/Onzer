import { useEffect, useState } from "react";

import { Artwork } from "@/features/library/Artwork";
import { ipc, type AlbumSummary } from "@/lib/ipc";

/**
 * Les albums d'un artiste, en rangée qui défile.
 *
 * # Pourquoi au-dessus des titres et non à la place
 *
 * On arrive sur la page d'un artiste pour deux raisons opposées : écouter un
 * morceau précis, ou parcourir ce qu'il a sorti. La rangée sert la seconde
 * sans coûter à la première — elle prend une hauteur de pochette, et les
 * titres restent juste en dessous.
 */
export function AlbumRow({
  artistId,
  onOpen,
}: {
  artistId: number;
  onOpen: (album: AlbumSummary) => void;
}) {
  const [albums, setAlbums] = useState<AlbumSummary[]>([]);

  useEffect(() => {
    let vivant = true;
    void ipc
      .artistAlbums(artistId)
      .then((liste) => {
        if (vivant) setAlbums(liste);
      })
      .catch(() => undefined);

    return () => {
      vivant = false;
    };
  }, [artistId]);

  // Un seul album, c'est la page elle-même : la rangée n'apprendrait rien.
  if (albums.length < 2) return null;

  return (
    <section className="px-6 pt-4">
      <h2 className="display text-[clamp(1.05rem,2.2vw,1.3rem)] text-ink">Ses albums</h2>

      <div className="mt-3 flex gap-4 overflow-x-auto pb-2">
        {albums.map((album) => (
          <button
            key={album.id}
            type="button"
            onClick={() => onOpen(album)}
            className="pression w-32 shrink-0 text-left hover:opacity-80"
          >
            <Artwork hash={album.artworkHash} className="aspect-square w-full rounded-md" />

            <p className="mt-2 truncate text-[13px] font-medium text-ink">{album.title}</p>
            <p className="numerals truncate text-[11px] text-ink-faint">
              {album.year ?? "—"} · {album.trackCount} titre{album.trackCount > 1 ? "s" : ""}
            </p>
          </button>
        ))}
      </div>
    </section>
  );
}
