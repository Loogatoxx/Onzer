import { useEffect, useState } from "react";

import { Icon } from "@/components/Icon";
import { formatDuration, ipc, type PlaylistComparison, type SpotifyStatus } from "@/lib/ipc";

/**
 * Comparer une playlist Spotify à la bibliothèque.
 *
 * # Ce que cette page fait, et ce qu'elle ne fait pas
 *
 * Elle répond à « qu'est-ce qui me manque ? ». Onzer lit une liste de titres,
 * la confronte à ce qu'il possède, et affiche les absents. **Il ne télécharge
 * rien** et n'embarque aucun téléchargeur.
 *
 * La formule proposée pour le terminal assemble ce que l'utilisateur taperait
 * autrement à la main, pour l'outil qu'il utilise déjà. Ce qu'il en fait, et
 * depuis quelle source, reste sa décision.
 */
export function SyncView() {
  const [status, setStatus] = useState<SpotifyStatus | null>(null);
  const [url, setUrl] = useState("");
  const [comparison, setComparison] = useState<PlaylistComparison | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    void ipc.spotifyStatus().then(setStatus).catch(() => undefined);
  }, []);

  async function compare() {
    setLoading(true);
    setError(null);
    setComparison(null);

    try {
      setComparison(await ipc.comparePlaylist(url));
    } catch (cause) {
      setError(String(cause));
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="px-6 pb-16 pt-8 sm:px-10">
      <header className="mx-auto max-w-3xl">
        <p className="text-xs font-semibold uppercase tracking-[0.16em] text-ink-faint">
          Compléter la bibliothèque
        </p>
        <h1 className="display-xl mt-2 text-[clamp(1.75rem,4vw,2.75rem)] text-ink">
          Ce qui te manque
        </h1>
        <p className="mt-3 max-w-xl text-[15px] leading-relaxed text-ink-muted">
          Colle le lien d'une playlist Spotify publique. Onzer la compare à ta
          bibliothèque et te dit ce que tu n'as pas — il ne télécharge rien.
        </p>
      </header>

      <div className="mx-auto mt-8 max-w-3xl">
        {status !== null && !status.configured ? (
          <Credentials onSaved={() => void ipc.spotifyStatus().then(setStatus)} />
        ) : (
          <>
            <div className="flex flex-wrap gap-3">
              <input
                type="text"
                value={url}
                spellCheck={false}
                onChange={(event) => setUrl(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void compare();
                }}
                placeholder="https://open.spotify.com/playlist/…"
                className="h-11 min-w-0 flex-1 rounded-full bg-elevated px-5 text-sm text-ink placeholder:text-ink-faint focus:outline focus:outline-1 focus:outline-ink-faint"
              />

              <button
                type="button"
                disabled={loading || url.trim() === ""}
                onClick={() => void compare()}
                className="flex items-center gap-2 rounded-full bg-ink px-6 text-[13px] font-semibold text-base transition-opacity hover:opacity-90 disabled:opacity-40"
              >
                <span className={loading ? "animate-spin" : ""}>
                  <Icon name={loading ? "repeat" : "search"} size={15} />
                </span>
                {loading ? "Lecture…" : "Comparer"}
              </button>
            </div>

            <p className="mt-2 text-[11px] text-ink-faint">
              Identifiants enregistrés
              {status?.idHint != null && (
                <span className="ml-1.5 font-mono">{status.idHint}</span>
              )}
              <button
                type="button"
                onClick={() => setStatus({ configured: false, idHint: null })}
                className="ml-2 underline underline-offset-2 transition-colors hover:text-ink"
              >
                modifier
              </button>
            </p>
          </>
        )}

        {error !== null && (
          <p className="mt-4 rounded-lg bg-danger/10 px-4 py-3 text-[13px] leading-relaxed text-danger">
            {error}
          </p>
        )}

        {comparison !== null && (
          <section className="mt-10">
            <h2 className="display text-[clamp(1.15rem,2.4vw,1.6rem)] text-ink">
              {comparison.playlistName}
            </h2>
            <p className="numerals mt-1 text-[13px] text-ink-muted">
              {comparison.present} / {comparison.total} déjà dans ta bibliothèque
              {comparison.missing.length > 0 && (
                <>
                  <span className="mx-1.5 text-ink-faint">·</span>
                  <span className="text-warn">
                    {comparison.missing.length} manquant
                    {comparison.missing.length > 1 ? "s" : ""}
                  </span>
                </>
              )}
            </p>

            {comparison.missing.length === 0 ? (
              <p className="mt-6 text-[15px] text-ok">
                Tu as déjà tout. Rien à télécharger.
              </p>
            ) : (
              <>
                {comparison.command !== "" && (
                  <div className="mt-6 rounded-xl bg-surface p-4">
                    <div className="flex flex-wrap items-baseline justify-between gap-2">
                      <p className="text-[13px] font-semibold text-ink">
                        La formule, si tu veux automatiser
                      </p>
                      <button
                        type="button"
                        onClick={() => {
                          void navigator.clipboard.writeText(comparison.command);
                          setCopied(true);
                          setTimeout(() => setCopied(false), 2000);
                        }}
                        className="rounded-full bg-elevated px-3 py-1 text-[11px] font-semibold text-ink-muted transition-colors hover:text-ink"
                      >
                        {copied ? "Copié" : "Copier"}
                      </button>
                    </div>

                    <pre className="mt-3 overflow-x-auto rounded-lg bg-base p-3 font-mono text-[11px] leading-relaxed text-ink-muted">
                      {comparison.command}
                    </pre>

                    <p className="mt-3 text-[11px] leading-relaxed text-ink-faint">
                      Onzer a écrit les liens manquants dans un fichier et vise le
                      dossier de dépôt : ce que tu récupères y atterrit, et le
                      dédoublonnage, l'identification et le rangement se font
                      tout seuls. Onzer ne lance pas cette commande — c'est ton
                      outil, ton terminal, ta décision.
                    </p>
                  </div>
                )}

                <ul className="mt-8 divide-y divide-line">
                  {comparison.missing.map((track, index) => (
                    <li
                      key={`${track.url}-${index}`}
                      className="flex items-center gap-4 py-3"
                    >
                      <span className="numerals w-8 shrink-0 text-right text-[13px] text-ink-faint">
                        {index + 1}
                      </span>

                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-[15px] font-medium text-ink">
                          {track.title}
                        </span>
                        <span className="block truncate text-[13px] text-ink-muted">
                          {track.artists.join(", ") || "Artiste inconnu"}
                          {track.album != null && (
                            <>
                              <span className="mx-1.5 text-ink-faint">·</span>
                              {track.album}
                            </>
                          )}
                        </span>
                      </span>

                      <span className="numerals shrink-0 text-[13px] text-ink-faint">
                        {formatDuration(track.durationMs)}
                      </span>
                    </li>
                  ))}
                </ul>
              </>
            )}
          </section>
        )}
      </div>
    </div>
  );
}

/**
 * Saisie des identifiants d'application.
 *
 * Même schéma que pour AcoustID : l'utilisateur fournit les siens, Onzer n'en
 * distribue aucun. Une clé partagée entre tous serait bannie dès le premier
 * usage intensif.
 */
function Credentials({ onSaved }: { onSaved: () => void }) {
  const [id, setId] = useState("");
  const [secret, setSecret] = useState("");
  const [error, setError] = useState<string | null>(null);

  async function save() {
    setError(null);
    try {
      await ipc.setSpotifyCredentials(id.trim(), secret.trim());
      onSaved();
    } catch (cause) {
      setError(String(cause));
    }
  }

  return (
    <div className="rounded-xl bg-surface p-5">
      <p className="text-[15px] font-semibold text-ink">
        Deux identifiants à créer, une fois
      </p>

      <ol className="mt-3 space-y-1.5 text-[13px] leading-relaxed text-ink-muted">
        <li>
          1. Ouvre <span className="font-mono text-ink">developer.spotify.com/dashboard</span>
        </li>
        <li>2. « Create app » — n'importe quel nom, n'importe quelle description</li>
        <li>3. Recopie ici le <span className="text-ink">Client ID</span> et le{" "}
          <span className="text-ink">Client Secret</span>
        </li>
      </ol>

      <div className="mt-4 space-y-2">
        <input
          type="text"
          value={id}
          spellCheck={false}
          onChange={(event) => setId(event.target.value)}
          placeholder="Client ID"
          className="h-10 w-full rounded-lg bg-base px-3 font-mono text-xs text-ink placeholder:text-ink-faint focus:outline focus:outline-1 focus:outline-accent"
        />
        <input
          type="password"
          value={secret}
          spellCheck={false}
          onChange={(event) => setSecret(event.target.value)}
          placeholder="Client Secret"
          className="h-10 w-full rounded-lg bg-base px-3 font-mono text-xs text-ink placeholder:text-ink-faint focus:outline focus:outline-1 focus:outline-accent"
        />
      </div>

      {error !== null && <p className="mt-3 text-[12px] text-danger">{error}</p>}

      <button
        type="button"
        disabled={id.trim() === "" || secret.trim() === ""}
        onClick={() => void save()}
        className="mt-4 rounded-full bg-ink px-5 py-2 text-[13px] font-semibold text-base transition-opacity hover:opacity-90 disabled:opacity-40"
      >
        Enregistrer
      </button>

      <p className="mt-4 text-[11px] leading-relaxed text-ink-faint">
        Ces identifiants ne donnent accès qu'à la lecture de playlists publiques.
        Onzer ne demande jamais ton mot de passe Spotify, et ne lit aucune donnée
        de ton compte.
      </p>
    </div>
  );
}
