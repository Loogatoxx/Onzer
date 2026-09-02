import { useCallback, useEffect, useRef, useState } from "react";

import { ipc, type Lyrics } from "@/lib/ipc";

/**
 * Index de la ligne chantée à cet instant.
 *
 * C'est le pendant exact de `Lyrics::line_at` côté Rust. La logique est
 * dupliquée en connaissance de cause : l'alternative serait un aller-retour IPC
 * **quatre fois par seconde**, pour une recherche dichotomique de six lignes.
 */
export function lineAt(lyrics: Lyrics, positionMs: number): number | null {
  const lines = lyrics.synced;
  if (lines.length === 0) return null;

  const first = lines[0];
  if (first === undefined || positionMs < first.atMs) return null;

  let low = 0;
  let high = lines.length;
  while (low < high) {
    const middle = (low + high) >> 1;
    const line = lines[middle];
    if (line !== undefined && line.atMs <= positionMs) low = middle + 1;
    else high = middle;
  }

  return low - 1;
}

/**
 * État des paroles d'un morceau.
 *
 * # Pourquoi un hook plutôt qu'un composant
 *
 * Les paroles s'affichent à deux endroits — le panneau latéral et la vue en
 * grand — et rien n'y est identique **sauf la logique** : chargement, ligne
 * courante, recherche en ligne, saisie manuelle. Dupliquer cette logique
 * garantirait qu'une correction n'atteigne qu'un des deux.
 */
/** Au-delà, on cesse d'attendre et on le dit. */
const DELAI_RECHERCHE_MS = 15_000;

export function useLyrics(trackId: number | null, positionMs: number) {
  const [lyrics, setLyrics] = useState<Lyrics | null>(null);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [searching, setSearching] = useState(false);

  /** La ligne en cours, pour la faire défiler d'elle-même. */
  const activeLine = useRef<HTMLParagraphElement>(null);

  useEffect(() => {
    setLyrics(null);
    setEditing(false);
    setDraft("");
    setError(null);
    setSearching(false);

    if (trackId === null) return;

    let active = true;
    void ipc
      .trackLyrics(trackId)
      .then((loaded) => {
        if (active) setLyrics(loaded);
      })
      .catch(() => {
        if (active) setLyrics({ synced: [], plain: [] });
      });

    return () => {
      active = false;
    };
  }, [trackId]);

  const current = lyrics === null ? null : lineAt(lyrics, positionMs);

  // La ligne courante se recentre d'elle-même. Sans cela, il faudrait faire
  // défiler à la main pendant qu'on écoute — l'inverse de ce qu'on attend.
  useEffect(() => {
    activeLine.current?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [current]);

  const save = useCallback(async () => {
    if (trackId === null) return;

    try {
      setLyrics(await ipc.setTrackLyrics(trackId, draft));
      setEditing(false);
    } catch (cause) {
      setError(String(cause));
    }
  }, [trackId, draft]);

  /**
   * Va chercher les paroles sur LRCLIB.
   *
   * Sur clic explicite : Onzer est un lecteur hors ligne, et rien ne part sur
   * le réseau sans que l'utilisateur l'ait demandé.
   */
  const search = useCallback(async () => {
    if (trackId === null) return;

    setSearching(true);
    setError(null);

    // # Pourquoi une limite de temps
    //
    // Un service lent ou injoignable laissait le bouton sur « Recherche… »
    // indéfiniment. Quinze secondes suffisent largement à LRCLIB quand il
    // répond ; au-delà, ce n'est plus de l'attente, c'est du silence. Mieux
    // vaut dire qu'on n'a pas trouvé que ne rien dire du tout.
    const abandon = new Promise<never>((_, rejeter) =>
      setTimeout(
        () => rejeter(new Error("Paroles introuvables : le service n'a pas répondu.")),
        DELAI_RECHERCHE_MS,
      ),
    );

    try {
      const found = await Promise.race([ipc.fetchLyrics(trackId), abandon]);
      if (found.synced.length === 0 && found.plain.length === 0) {
        setError("Aucune parole trouvée pour ce morceau.");
      } else {
        setLyrics(found);
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setSearching(false);
    }
  }, [trackId]);

  /**
   * Cale les paroles à l'oreille, sur ce morceau seulement.
   *
   * Une trentaine de secondes : le modèle écoute le morceau en entier. Le
   * bouton reste donc désactivé pendant, avec son propre état — se contenter
   * de `searching` mélangerait deux attentes qui n'ont ni la même durée ni la
   * même explication.
   */
  const [syncing, setSyncing] = useState(false);

  const sync = useCallback(async () => {
    if (trackId === null) return;

    setSyncing(true);
    setError(null);

    try {
      setLyrics(await ipc.syncTrack(trackId));
    } catch (cause) {
      setError(String(cause));
    } finally {
      setSyncing(false);
    }
  }, [trackId]);

  const isEmpty =
    lyrics !== null && lyrics.synced.length === 0 && lyrics.plain.length === 0;

  return {
    lyrics,
    current,
    activeLine,
    isEmpty,
    editing,
    setEditing,
    draft,
    setDraft,
    error,
    searching,
    syncing,
    sync,
    save,
    search,
  };
}
