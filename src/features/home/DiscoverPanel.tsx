import { useState } from "react";

import { Icon } from "@/components/Icon";
import { ipc, type Suggestion } from "@/lib/ipc";

/**
 * Artistes à découvrir.
 *
 * # Pourquoi c'est le seul endroit d'Onzer qui parle de ce que tu n'as pas
 *
 * Le moteur de recommandation ne connaît que ta bibliothèque. Il sait très bien
 * te dire quoi y réécouter ; il ne peut pas, par construction, te parler de ce
 * qui n'y est pas. Suggérer un artiste absent suppose une source extérieure —
 * ici ListenBrainz, en données ouvertes, sans clé ni compte.
 *
 * # Pourquoi un bouton et non un chargement automatique
 *
 * Onzer est un lecteur hors ligne. Interroger un service à l'ouverture de la
 * page se ferait dans ton dos ; un bouton, non. Ce qui part se limite à des
 * identifiants d'artistes — pas un titre, pas une écoute, pas un horodatage.
 */
export function DiscoverPanel() {
  const [suggestions, setSuggestions] = useState<Suggestion[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function search() {
    setLoading(true);
    setError(null);

    try {
      const found = await ipc.discoverArtists();
      setSuggestions(found);
      if (found.length === 0) {
        setError("Aucune suggestion : tes artistes sont peu représentés dans les bases publiques.");
      }
    } catch (cause) {
      setError(String(cause));
    } finally {
      setLoading(false);
    }
  }

  return (
    <section className="mt-10">
      <div className="flex flex-wrap items-baseline justify-between gap-3">
        <h2 className="display text-[clamp(1.15rem,2.4vw,1.5rem)] text-ink">
          À découvrir ailleurs
        </h2>

        <button
          type="button"
          disabled={loading}
          onClick={() => void search()}
          className="flex items-center gap-2 rounded-full bg-elevated px-4 py-2 text-[13px] font-semibold text-ink transition-colors hover:bg-raised disabled:opacity-40"
        >
          <span className={loading ? "animate-spin" : ""}>
            <Icon name={loading ? "repeat" : "sparkle"} size={15} />
          </span>
          {loading ? "Recherche…" : suggestions === null ? "Chercher" : "Actualiser"}
        </button>
      </div>

      {suggestions === null && !loading && (
        <p className="mt-3 max-w-2xl text-[13px] leading-relaxed text-ink-faint">
          Onzer ne connaît que ta bibliothèque. Pour te proposer des artistes que
          tu n'as pas, il demande à ListenBrainz qui ressemble à ceux que tu
          écoutes le plus. Seuls des identifiants d'artistes quittent ta machine.
        </p>
      )}

      {error !== null && <p className="mt-3 text-[13px] text-warn">{error}</p>}

      {suggestions !== null && suggestions.length > 0 && (
        <>
          <div className="mt-4 grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
            {suggestions.map((suggestion, index) => (
              <div
                key={suggestion.mbid}
                className="flex items-center gap-3 rounded-lg bg-surface p-3"
              >
                {/* Pas de portrait : les récupérer supposerait d'aller les
                    chercher chez un tiers, pour un ornement. Le rang suffit à
                    donner du rythme à la grille. */}
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
