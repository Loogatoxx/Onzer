import { useMemo } from "react";

import { Artwork } from "@/features/library/Artwork";
import type { TrackSummary } from "@/lib/ipc";

/** Les trois façons de lire un même jeu de résultats. */
export type FiltreRecherche = "titres" | "artistes" | "albums";

const LIBELLES: Record<FiltreRecherche, string> = {
  titres: "Titres",
  artistes: "Artistes",
  albums: "Albums",
};

/**
 * Les trois filtres de la recherche.
 *
 * # Pourquoi ils ne relancent aucune requête
 *
 * Chercher « adele » rend des morceaux : ceux dont le titre correspond, mais
 * aussi ceux dont l'artiste ou l'album correspond. Les artistes et les albums
 * sont donc **déjà là**, dans ce que le cœur a renvoyé — les redemander
 * séparément coûterait deux requêtes de plus pour recalculer ce qu'on tient.
 *
 * # Pourquoi des filtres et non trois listes empilées
 *
 * Empiler « Artistes », puis « Albums », puis « Titres » sur un écran de
 * téléphone, c'est faire défiler deux sections pour atteindre celle qu'on
 * voulait. Le filtre demande un appui et rend l'écran entier à la réponse.
 */
export function BarreFiltres({
  actif,
  onChange,
  compte,
}: {
  actif: FiltreRecherche;
  onChange: (filtre: FiltreRecherche) => void;
  compte: Record<FiltreRecherche, number>;
}) {
  return (
    <div className="flex gap-2 px-6 pb-3">
      {(Object.keys(LIBELLES) as FiltreRecherche[]).map((filtre) => (
        <button
          key={filtre}
          type="button"
          disabled={compte[filtre] === 0}
          onClick={() => onChange(filtre)}
          className={`pression rounded-full px-3.5 py-1.5 text-[13px] font-medium transition-colors disabled:opacity-30 ${
            actif === filtre
              ? "bg-ink text-base"
              : "bg-elevated text-ink-muted hover:text-ink"
          }`}
        >
          {LIBELLES[filtre]}
          {compte[filtre] > 0 && (
            <span className="numerals ml-1.5 opacity-60">{compte[filtre]}</span>
          )}
        </button>
      ))}
    </div>
  );
}

/** Un artiste ou un album tiré des résultats, avec de quoi y aller. */
export interface Regroupement {
  cle: string;
  nom: string;
  detail: string;
  artworkHash: string | null;
  /** Un morceau du groupe : c'est par lui qu'on ouvre la page. */
  trackId: number;
}

/**
 * Regroupe les résultats par artiste et par album.
 *
 * L'ordre suit celui des résultats — le premier morceau trouvé décide de la
 * place de son artiste. Trier par nom paraîtrait plus propre et détruirait la
 * pertinence : ce que le cœur a jugé le plus proche de la requête tomberait au
 * milieu de la liste, entre deux inconnus.
 */
export function useRegroupements(resultats: TrackSummary[]) {
  return useMemo(() => {
    const artistes = new Map<string, Regroupement & { compte: number }>();
    const albums = new Map<string, Regroupement & { compte: number }>();

    for (const track of resultats) {
      if (track.artist !== null) {
        const existant = artistes.get(track.artist);
        if (existant === undefined) {
          artistes.set(track.artist, {
            cle: track.artist,
            nom: track.artist,
            detail: "",
            artworkHash: track.artworkHash,
            trackId: track.id,
            compte: 1,
          });
        } else {
          existant.compte += 1;
        }
      }

      // Deux albums peuvent porter le même nom : c'est l'identifiant qui les
      // sépare, jamais le titre.
      if (track.album !== null && track.albumId !== null) {
        const cle = String(track.albumId);
        const existant = albums.get(cle);
        if (existant === undefined) {
          albums.set(cle, {
            cle,
            nom: track.album,
            detail: track.artist ?? "Artiste inconnu",
            artworkHash: track.artworkHash,
            trackId: track.id,
            compte: 1,
          });
        } else {
          existant.compte += 1;
          // Une pochette vaut mieux que pas de pochette : le premier morceau
          // d'un album n'est pas toujours celui qui la porte.
          if (existant.artworkHash === null) existant.artworkHash = track.artworkHash;
        }
      }
    }

    const finir = (entrees: Map<string, Regroupement & { compte: number }>) =>
      [...entrees.values()].map((entree) => ({
        ...entree,
        detail:
          entree.detail === ""
            ? `${entree.compte} morceau${entree.compte > 1 ? "x" : ""}`
            : `${entree.detail} · ${entree.compte} morceau${entree.compte > 1 ? "x" : ""}`,
      }));

    return { artistes: finir(artistes), albums: finir(albums) };
  }, [resultats]);
}

/** La liste d'un regroupement — artistes ou albums. */
export function ListeRegroupements({
  entrees,
  rond,
  onOuvrir,
}: {
  entrees: Regroupement[];
  /** Les artistes se dessinent en rond, les albums en carré. */
  rond: boolean;
  onOuvrir: (trackId: number) => void;
}) {
  return (
    <ul className="px-1 pb-8 lg:px-3">
      {entrees.map((entree) => (
        <li key={entree.cle}>
          <button
            type="button"
            onClick={() => onOuvrir(entree.trackId)}
            className="pression flex w-full items-center gap-3 rounded-md px-1.5 py-2 text-left transition-colors hover:bg-elevated lg:px-3"
          >
            <Artwork
              hash={entree.artworkHash}
              className={`h-12 w-12 shrink-0 ${rond ? "rounded-full" : "rounded"}`}
            />
            <span className="min-w-0">
              <span className="block truncate text-[15px] font-medium leading-tight text-ink">
                {entree.nom}
              </span>
              <span className="block truncate text-[13px] leading-tight text-ink-muted">
                {entree.detail}
              </span>
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}
