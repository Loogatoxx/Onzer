import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import { Icon } from "@/components/Icon";
import { DiscoverPanel } from "@/features/home/DiscoverPanel";
import {
  formatDuration,
  ipc,
  type PlaylistComparison,
  type SpotifyStatus,
} from "@/lib/ipc";

/**
 * Ce qui manque à la bibliothèque.
 *
 * # Deux façons de répondre à la même question
 *
 * | Point de départ | Ce qu'on obtient |
 * |---|---|
 * | Une playlist qu'on possède ailleurs | Les titres qu'on n'a pas |
 * | Ses propres goûts | Des titres et des artistes qu'on ne connaît pas encore |
 *
 * La seconde est placée en premier : elle fonctionne d'un seul clic, sans rien
 * installer ni configurer.
 *
 * # Ce qu'Onzer ne fait pas
 *
 * Il ne télécharge rien, n'embarque aucun téléchargeur, et n'exécute aucune
 * commande. Les formules proposées assemblent ce qu'on taperait autrement à la
 * main, pour l'outil qu'on utilise déjà.
 */
export function SyncView() {
  return (
    <div className="px-6 pb-16 pt-8 sm:px-10">
      <header className="mx-auto max-w-3xl">
        <p className="text-xs font-semibold uppercase tracking-[0.16em] text-ink-faint">
          Compléter la bibliothèque
        </p>
        <h1 className="display-xl mt-2 text-[clamp(1.75rem,4vw,2.75rem)] text-ink">
          Ce qui te manque
        </h1>
      </header>

      <div className="mx-auto mt-8 max-w-3xl">
        <section>
          <h2 className="display text-[clamp(1.15rem,2.4vw,1.6rem)] text-ink">
            D'après tes goûts
          </h2>
          <p className="mt-1 max-w-xl text-[13px] leading-relaxed text-ink-muted">
            Des titres et des artistes que tu n'as pas, déduits de ce que tu
            écoutes déjà.
          </p>

          <div className="mt-5">
            <DiscoverPanel />
          </div>
        </section>

        <div className="my-12 border-t border-line" />

        <FromPlaylist />
      </div>
    </div>
  );
}

/**
 * Comparaison à une playlist Spotify.
 *
 * # Pourquoi ça passe par un fichier
 *
 * Le premier jet interrogeait l'API Spotify avec des identifiants créés par
 * l'utilisateur. Spotify a répondu **403 — « Active premium subscription
 * required for the owner of the app »** : lire une playlist publique exige
 * maintenant un abonnement payant sur le compte propriétaire de l'application.
 *
 * `spotdl`, déjà installé, sait produire la liste sans rien télécharger et sans
 * aucun identifiant. Le partage des rôles ne change pas : son outil va chercher
 * la liste, Onzer la compare.
 */
function FromPlaylist() {
  const [url, setUrl] = useState("");
  const [saveCommand, setSaveCommand] = useState("");
  const [comparison, setComparison] = useState<PlaylistComparison | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [status, setStatus] = useState<SpotifyStatus | null>(null);
  const [editing, setEditing] = useState(false);

  useEffect(() => {
    void ipc.spotifyStatus().then(setStatus).catch(() => undefined);
  }, []);

  function copy(text: string, key: string) {
    void navigator.clipboard.writeText(text);
    setCopied(key);
    setTimeout(() => setCopied(null), 2000);
  }

  async function prepare() {
    setError(null);
    try {
      setSaveCommand(await ipc.playlistSaveCommand(url));
    } catch (cause) {
      setError(String(cause));
    }
  }

  async function pick() {
    const selected = await open({
      multiple: false,
      title: "Choisir le fichier produit par spotdl save",
      filters: [{ name: "Liste spotdl", extensions: ["spotdl", "json"] }],
    });
    if (typeof selected !== "string") return;

    setLoading(true);
    setError(null);

    try {
      setComparison(await ipc.comparePlaylistFile(selected));
    } catch (cause) {
      setError(String(cause));
    } finally {
      setLoading(false);
    }
  }

  return (
    <section>
      <h2 className="display text-[clamp(1.15rem,2.4vw,1.6rem)] text-ink">
        Depuis une playlist Spotify
      </h2>
      <p className="mt-1 max-w-xl text-[13px] leading-relaxed text-ink-muted">
        En deux temps : ton <span className="font-mono">spotdl</span> récupère la
        liste, Onzer la compare. Spotify ne laisse plus une application lire une
        playlist sans abonnement payant — ce détour contourne le mur sans rien
        forcer.
      </p>

      {/* ── 0. Les identifiants ──────────────────────────────────────── */}
      {status !== null && (!status.configured || editing) ? (
        <Credentials
          onSaved={() => {
            setEditing(false);
            void ipc.spotifyStatus().then(setStatus);
          }}
        />
      ) : (
        status?.configured === true && (
          <p className="mt-4 text-[11px] text-ink-faint">
            Identifiants Spotify enregistrés
            {status.idHint != null && (
              <span className="ml-1.5 font-mono">{status.idHint}</span>
            )}
            <span className="mx-1.5">·</span>
            ils seront glissés dans les commandes ci-dessous
            <button
              type="button"
              onClick={() => setEditing(true)}
              className="ml-2 underline underline-offset-2 transition-colors hover:text-ink"
            >
              modifier
            </button>
          </p>
        )
      )}

      {/* ── 1. Produire la liste ─────────────────────────────────────── */}
      <div className="mt-6 flex flex-wrap gap-3">
        <input
          type="text"
          value={url}
          spellCheck={false}
          onChange={(event) => setUrl(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") void prepare();
          }}
          placeholder="https://open.spotify.com/playlist/…"
          className="h-11 min-w-0 flex-1 rounded-full bg-elevated px-5 text-sm text-ink placeholder:text-ink-faint focus:outline focus:outline-1 focus:outline-ink-faint"
        />

        <button
          type="button"
          disabled={url.trim() === ""}
          onClick={() => void prepare()}
          className="rounded-full bg-elevated px-5 text-[13px] font-semibold text-ink transition-colors hover:bg-raised disabled:opacity-40"
        >
          Préparer la commande
        </button>
      </div>

      {saveCommand !== "" && (
        <div className="mt-4 rounded-xl bg-surface p-4">
          <div className="flex flex-wrap items-baseline justify-between gap-2">
            <p className="text-[13px] font-semibold text-ink">
              1. Lance ça dans un terminal
            </p>
            <button
              type="button"
              onClick={() => copy(saveCommand, "save")}
              className="rounded-full bg-elevated px-3 py-1 text-[11px] font-semibold text-ink-muted transition-colors hover:text-ink"
            >
              {copied === "save" ? "Copié" : "Copier"}
            </button>
          </div>

          <pre className="mt-3 overflow-x-auto rounded-lg bg-base p-3 font-mono text-[11px] leading-relaxed text-ink-muted">
            {saveCommand}
          </pre>

          <p className="mt-3 text-[11px] leading-relaxed text-ink-faint">
            <span className="font-mono">save</span> ne télécharge aucun audio :
            elle ne fait que rassembler les métadonnées. Sur une grande playlist,
            comptez plusieurs minutes.
          </p>

          {saveCommand.includes("--client-secret") && (
            <p className="mt-2 text-[11px] leading-relaxed text-warn">
              Cette ligne contient ton <em>client secret</em> en clair — c'est ce
              qu'attend spotdl. Ne la colle nulle part ailleurs que dans ton
              terminal.
            </p>
          )}
        </div>
      )}

      {/* ── 2. Comparer ──────────────────────────────────────────────── */}
      <div className="mt-4 flex flex-wrap items-center gap-3">
        <button
          type="button"
          disabled={loading}
          onClick={() => void pick()}
          className="flex items-center gap-2 rounded-full bg-ink px-5 py-2.5 text-[13px] font-semibold text-base transition-opacity hover:opacity-90 disabled:opacity-40"
        >
          <span className={loading ? "animate-spin" : ""}>
            <Icon name={loading ? "repeat" : "folder"} size={15} />
          </span>
          {loading ? "Comparaison…" : "2. Choisir le fichier obtenu"}
        </button>
      </div>

      {error !== null && (
        <p className="mt-4 rounded-lg bg-danger/10 px-4 py-3 text-[13px] leading-relaxed text-danger">
          {error}
        </p>
      )}

      {comparison !== null && (
        <div className="mt-8">
          <h3 className="display text-[clamp(1.05rem,2vw,1.35rem)] text-ink">
            {comparison.playlistName}
          </h3>
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
            <p className="mt-5 text-[15px] text-ok">
              Tu as déjà tout. Rien à récupérer.
            </p>
          ) : (
            <>
              {comparison.command !== "" && (
                <div className="mt-5 rounded-xl bg-surface p-4">
                  <div className="flex flex-wrap items-baseline justify-between gap-2">
                    <p className="text-[13px] font-semibold text-ink">
                      3. Récupérer les seuls manquants
                    </p>
                    <button
                      type="button"
                      onClick={() => copy(comparison.command, "download")}
                      className="rounded-full bg-elevated px-3 py-1 text-[11px] font-semibold text-ink-muted transition-colors hover:text-ink"
                    >
                      {copied === "download" ? "Copié" : "Copier"}
                    </button>
                  </div>

                  <pre className="mt-3 max-h-32 overflow-auto rounded-lg bg-base p-3 font-mono text-[11px] leading-relaxed text-ink-muted">
                    {comparison.command}
                  </pre>

                  <p className="mt-3 text-[11px] leading-relaxed text-ink-faint">
                    La sortie vise ton dossier de dépôt : ce que tu récupères y
                    atterrit, et le dédoublonnage, l'identification et le
                    rangement se font tout seuls. Onzer ne lance pas cette
                    commande — c'est ton outil, ton terminal, ta décision.
                  </p>
                </div>
              )}

              <ul className="mt-6 divide-y divide-line">
                {comparison.missing.map((track, index) => (
                  <li key={`${track.query}-${index}`} className="flex items-center gap-4 py-3">
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

                    {track.durationMs > 0 && (
                      <span className="numerals shrink-0 text-[13px] text-ink-faint">
                        {formatDuration(track.durationMs)}
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            </>
          )}
        </div>
      )}
    </section>
  );
}

/**
 * Saisie des identifiants d'application Spotify.
 *
 * # Pourquoi ils servent encore
 *
 * Onzer n'interroge plus l'API lui-même — Spotify le lui interdit sans
 * abonnement payant. Mais `spotdl`, interrogé anonymement, se fait éconduire
 * sur les grandes playlists : l'erreur remontée est un obscur
 * `'NoneType' object is not subscriptable` au fond de sa couche réseau.
 * Ses propres identifiants lèvent la limite.
 *
 * Onzer les stocke et les glisse dans les commandes qu'il propose. Il ne s'en
 * sert pour aucun appel.
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
    <div className="mt-5 rounded-xl bg-surface p-5">
      <p className="text-[15px] font-semibold text-ink">
        Deux identifiants à créer, une fois
      </p>

      <ol className="mt-3 space-y-1.5 text-[13px] leading-relaxed text-ink-muted">
        <li>
          1. Ouvre <span className="font-mono text-ink">developer.spotify.com/dashboard</span>
        </li>
        <li>2. « Create app » — n&apos;importe quel nom, n&apos;importe quelle description</li>
        <li>
          3. Recopie ici le <span className="text-ink">Client ID</span> et le{" "}
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
        Onzer ne s&apos;en sert pour aucun appel : il les glisse dans les
        commandes qu&apos;il te propose, pour que spotdl ne soit plus éconduit.
      </p>
    </div>
  );
}
