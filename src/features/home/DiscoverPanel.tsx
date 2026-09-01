import { useState } from "react";

import { Icon } from "@/components/Icon";
import { CommandBox } from "@/features/sync/SyncView";
import {
  formatDuration,
  ipc,
  type ExportedList,
  type Suggestion,
  type TrackSuggestion,
} from "@/lib/ipc";

/**
 * Ce que tu n'as pas encore.
 *
 * # Pourquoi c'est le seul endroit d'Onzer qui parle de l'extérieur
 *
 * Le moteur de recommandation ne connaît que la bibliothèque. Il sait très bien
 * dire quoi y réécouter ; il ne peut pas, par construction, parler de ce qui
 * n'y est pas.
 *
 * # Deux questions, deux sources
 *
 * | Question | Source | Pourquoi celle-là |
 * |---|---|---|
 * | Quels **titres** me manquent ? | Discographies MusicBrainz | Complètes et fiables |
 * | Quels **artistes** essayer ? | ListenBrainz | Proximité fondée sur les écoutes réelles |
 *
 * Le choix des titres mérite une explication. ListenBrainz sait dire quels
 * enregistrements se ressemblent — à condition que quelqu'un les ait écoutés
 * chez eux. Interrogé sur cette bibliothèque, il répond **systématiquement une
 * liste vide** : le rap francophone y est trop peu représenté. La discographie,
 * elle, est complète : MusicBrainz connaît 194 enregistrements de Damso.
 * Comparer ce catalogue à ce qu'on possède répond exactement à la question,
 * sans rien inventer.
 *
 * # Pourquoi un bouton et non un chargement automatique
 *
 * Onzer est un lecteur hors ligne. Interroger un service à l'ouverture de la
 * page se ferait dans ton dos ; un bouton, non.
 */
export function DiscoverPanel() {
  const [tab, setTab] = useState<"tracks" | "artists">("tracks");
  const [tracks, setTracks] = useState<TrackSuggestion[] | null>(null);
  const [artists, setArtists] = useState<Suggestion[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [exported, setExported] = useState<ExportedList | null>(null);

  async function search() {
    setLoading(true);
    setError(null);

    try {
      if (tab === "tracks") {
        setExported(null);
        const found = await ipc.discoverTracks();
        setTracks(found);
        if (found.length === 0) {
          setError("Rien à proposer : tes artistes sont déjà complets, ou absents des bases publiques.");
        }
      } else {
        const found = await ipc.discoverArtists();
        setArtists(found);
        if (found.length === 0) {
          setError("Aucune suggestion : tes artistes sont peu représentés dans les bases publiques.");
        }
      }
    } catch (cause) {
      setError(String(cause));
    } finally {
      setLoading(false);
    }
  }

  const current = tab === "tracks" ? tracks : artists;

  /**
   * Écrit les suggestions dans le dossier de dépôt.
   *
   * Le même chemin que les manquants d'une playlist : la question posée est la
   * même, et deux réponses différentes obligeraient à réapprendre deux fois la
   * même chose.
   */
  function copy(text: string, key: string) {
    void navigator.clipboard.writeText(text);
    setCopied(key);
    setTimeout(() => setCopied(null), 2000);
  }

  async function exportList() {
    if (tracks === null || tracks.length === 0) return;

    try {
      setExported(
        await ipc.exportQueries(
          tracks.map((track) => track.query),
          "_recommandations",
        ),
      );
    } catch (cause) {
      setError(String(cause));
    }
  }

  return (
    <section>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex gap-1 rounded-full bg-elevated p-1">
          <Tab active={tab === "tracks"} onClick={() => setTab("tracks")}>
            Titres
          </Tab>
          <Tab active={tab === "artists"} onClick={() => setTab("artists")}>
            Artistes
          </Tab>
        </div>

        <button
          type="button"
          disabled={loading}
          onClick={() => void search()}
          className="flex items-center gap-2 rounded-full bg-ink px-5 py-2 text-[13px] font-semibold text-base transition-opacity hover:opacity-90 disabled:opacity-40"
        >
          <span className={loading ? "animate-spin" : ""}>
            <Icon name={loading ? "repeat" : "sparkle"} size={15} />
          </span>
          {loading ? "Recherche…" : current === null ? "Chercher" : "Actualiser"}
        </button>
      </div>

      {current === null && !loading && (
        <p className="mt-4 max-w-2xl text-[13px] leading-relaxed text-ink-faint">
          {tab === "tracks"
            ? "Onzer compare la discographie complète de tes artistes préférés à ce que tu possèdes, et te dit ce qui manque."
            : "Onzer demande à ListenBrainz quels artistes ressemblent à ceux que tu écoutes le plus, et écarte ceux que tu as déjà."}{" "}
          Seuls des noms et des identifiants quittent ta machine, et uniquement
          sur ce clic.
        </p>
      )}

      {error !== null && <p className="mt-4 text-[13px] text-warn">{error}</p>}

      {tab === "tracks" && tracks !== null && tracks.length > 0 && (
        <>
          {exported === null ? (
            <button
              type="button"
              onClick={() => void exportList()}
              className="mt-5 flex items-center gap-2 rounded-full bg-elevated px-5 py-2 text-[13px] font-semibold text-ink transition-colors hover:bg-raised"
            >
              <Icon name="folder" size={15} />
              Exporter la liste et sa commande
            </button>
          ) : (
            <>
              <p className="mt-5 text-[12px] text-ink-muted">
                {exported.count} requêtes écrites dans{" "}
                <span className="font-mono text-ink-faint">{exported.path}</span>
              </p>

              {/* spotdl d'abord : quand son accès fonctionne, il apporte les
                  métadonnées et la pochette d'un coup — il n'y a plus rien à
                  rattraper derrière. */}
              <CommandBox
                title="Tout récupérer, métadonnées comprises"
                command={exported.spotdlCommand}
                copied={copied === "spotdl"}
                onCopy={() => copy(exported.spotdlCommand, "spotdl")}
                note="spotdl tague les fichiers et récupère les pochettes lui-même. Quand son accès à Spotify fonctionne, c'est la voie la plus complète : il ne reste rien à rattraper."
              />

              <CommandBox
                title="Repasser sur les manquants"
                command={exported.command}
                copied={copied === "ytdlp"}
                onCopy={() => copy(exported.command, "ytdlp")}
                note="yt-dlp aspire le son brut et ne dépend d'aucun accès à Spotify — à lancer pour ce que spotdl n'a pas pu récupérer. Les fichiers arrivent nommés « Artiste - Titre.mp3 » et Onzer les identifie ensuite tout seul."
                quiet
              />
            </>
          )}

          <ul className="mt-6 divide-y divide-line">
            {tracks.map((track, index) => (
              <li key={`${track.query}-${index}`} className="flex items-center gap-4 py-3">
                <span className="numerals w-7 shrink-0 text-right text-[13px] text-ink-faint">
                  {index + 1}
                </span>

                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[15px] font-medium text-ink">
                    {track.title}
                  </span>
                  <span className="block truncate text-[13px] text-ink-muted">
                    {track.artist}
                  </span>
                </span>

                {track.durationMs !== null && (
                  <span className="numerals shrink-0 text-[13px] text-ink-faint">
                    {formatDuration(track.durationMs)}
                  </span>
                )}
              </li>
            ))}
          </ul>
        </>
      )}

      {tab === "artists" && artists !== null && artists.length > 0 && (
        <>
          <div className="mt-5 grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
            {artists.map((suggestion, index) => (
              <div
                key={suggestion.mbid}
                className="flex items-center gap-3 rounded-lg bg-surface p-3"
              >
                {/* Pas de portrait : les récupérer supposerait d'aller les
                    chercher chez un tiers, pour un ornement. */}
                <span className="numerals display w-7 shrink-0 text-center text-xl text-ink-faint">
                  {index + 1}
                </span>

                <div className="min-w-0 flex-1">
                  <p className="truncate text-[15px] font-semibold text-ink">
                    {suggestion.name}
                  </p>
                  <p className="truncate text-[12px] text-ink-faint">
                    {suggestion.reason}
                  </p>
                </div>
              </div>
            ))}
          </div>

          <p className="mt-3 text-[11px] leading-relaxed text-ink-faint">
            Onzer ne télécharge rien : à toi de les chercher où tu as l'habitude.
            Dépose ensuite les fichiers dans <span className="font-mono">_Inbox</span>,
            ils seront rangés tout seuls.
          </p>
        </>
      )}
    </section>
  );
}

function Tab({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`rounded-full px-4 py-1.5 text-[13px] font-semibold transition-colors ${
        active ? "bg-ink text-base" : "text-ink-muted hover:text-ink"
      }`}
    >
      {children}
    </button>
  );
}
