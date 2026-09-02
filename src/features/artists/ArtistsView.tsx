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
/** Les dispositions possibles de la grille d'artistes. */
type Disposition = "grandes" | "petites" | "liste";

const CLE_DISPOSITION = "onzer.artistes.disposition";

/**
 * Le sélecteur de disposition.
 *
 * # Pourquoi trois et pas deux
 *
 * Quatre cents artistes en grandes bulles font défiler longtemps ; en liste,
 * on lit vite mais on ne reconnaît plus les visages. Les petites bulles sont
 * le compromis, et personne ne peut décider à la place de l'autre lequel des
 * trois lui convient.
 */
function Disposition({
  valeur,
  onChange,
}: {
  valeur: Disposition;
  onChange: (valeur: Disposition) => void;
}) {
  const choix: { cle: Disposition; icone: "library" | "artist" | "queue"; titre: string }[] = [
    { cle: "grandes", icone: "artist", titre: "Grandes bulles" },
    { cle: "petites", icone: "library", titre: "Petites bulles" },
    { cle: "liste", icone: "queue", titre: "En liste" },
  ];

  return (
    <div className="flex items-center gap-1 rounded-full bg-elevated p-1">
      {choix.map((option) => (
        <button
          key={option.cle}
          type="button"
          title={option.titre}
          aria-label={option.titre}
          aria-pressed={valeur === option.cle}
          onClick={() => onChange(option.cle)}
          className={`flex h-7 w-7 items-center justify-center rounded-full transition-colors ${
            valeur === option.cle ? "bg-raised text-ink" : "text-ink-faint hover:text-ink-muted"
          }`}
        >
          <Icon name={option.icone} size={15} />
        </button>
      ))}
    </div>
  );
}

export function ArtistsView({
  onOpen,
}: {
  onOpen: (artist: ArtistSummary) => void;
}) {
  const [artists, setArtists] = useState<ArtistSummary[] | null>(null);
  const [query, setQuery] = useState("");

  /**
   * Comment on veut les voir.
   *
   * Le choix est retenu d'une visite à l'autre : c'est une préférence, pas une
   * décision qu'on veut reprendre à chaque fois. Il vit dans le navigateur
   * plutôt qu'en base — perdre l'affichage préféré ne coûte qu'un clic, et il
   * n'a rien à faire dans la bibliothèque de quelqu'un.
   */
  const [disposition, setDisposition] = useState<Disposition>(() => {
    try {
      const retenu = localStorage.getItem(CLE_DISPOSITION);
      if (retenu === "liste" || retenu === "petites" || retenu === "grandes") return retenu;
    } catch {
      // Navigation privée, stockage refusé : la valeur par défaut suffit.
    }
    return "grandes";
  });

  useEffect(() => {
    try {
      localStorage.setItem(CLE_DISPOSITION, disposition);
    } catch {
      // Sans mémoire, le choix ne dure que la session. Ce n'est pas grave.
    }
  }, [disposition]);

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

        <Disposition valeur={disposition} onChange={setDisposition} />

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
      ) : disposition === "liste" ? (
        <ul className="mt-6">
          {shown.map((artist) => (
            <li key={artist.id}>
              <button
                type="button"
                onClick={() => onOpen(artist)}
                className="flex w-full items-center gap-4 rounded-lg px-3 py-2 text-left transition-colors hover:bg-surface"
              >
                <Artwork hash={artist.coverHash} className="h-12 w-12 shrink-0 rounded-full" />

                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[15px] font-medium text-ink">
                    {artist.name}
                  </span>
                  <span className="numerals block text-[12px] text-ink-faint">
                    {artist.trackCount} titre{artist.trackCount > 1 ? "s" : ""}
                  </span>
                </span>

                <span className="shrink-0 text-ink-faint">
                  <Icon name="chevronRight" size={16} />
                </span>
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <div
          className="mt-6 grid gap-4"
          style={{
            gridTemplateColumns: `repeat(auto-fill, minmax(${
              disposition === "grandes" ? "10rem" : "6.5rem"
            }, 1fr))`,
          }}
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
