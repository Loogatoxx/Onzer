import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import {
  ipc,
  type LibraryCounts,
  type ScanProgress,
  type GeneratedPlaylist,
  type ScanSummary,
  type TrackSummary,
} from "@/lib/ipc";
import { DiscoverBar } from "@/features/discover/DiscoverBar";
import { PlayerBar } from "@/features/player/PlayerBar";
import { usePlayback } from "@/features/player/usePlayback";
import { TrackList } from "./TrackList";

/** Délai avant de lancer une recherche, pour ne pas requêter à chaque frappe. */
const SEARCH_DEBOUNCE_MS = 200;

export function LibraryView({ libraryRoot }: { libraryRoot: string }) {
  const [tracks, setTracks] = useState<TrackSummary[]>([]);
  const [counts, setCounts] = useState<LibraryCounts | null>(null);
  const [query, setQuery] = useState("");
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const [summary, setSummary] = useState<ScanSummary | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [playlist, setPlaylist] = useState<GeneratedPlaylist | null>(null);

  const playback = usePlayback();
  const importing = progress !== null;

  /**
   * Raison de présence de chaque morceau de la playlist générée.
   *
   * Reconstruite à chaque changement plutôt que stockée : la playlist est déjà
   * la source de vérité, la dupliquer inviterait la désynchronisation.
   */
  const reasons =
    playlist === null
      ? undefined
      : new Map(playlist.tracks.map((track) => [track.trackId, track.reason]));

  /** Une playlist générée remplace l'affichage de la bibliothèque. */
  async function showGenerated(generated: GeneratedPlaylist) {
    setError(null);
    setQuery("");
    setPlaylist(generated);

    // La lecture a démarré côté backend : on s'assure que la barre apparaisse
    // immédiatement, sans attendre le premier changement de morceau.
    playback.refresh();

    try {
      const ids = generated.tracks.map((track) => track.trackId);
      const loaded = await ipc.listTracks(1000);
      // On réordonne selon la playlist : `listTracks` renvoie l'ordre de la
      // bibliothèque, qui n'a rien à voir avec celui du moteur.
      const parId = new Map(loaded.map((track) => [track.id, track]));
      setTracks(ids.flatMap((id) => {
        const found = parId.get(id);
        return found === undefined ? [] : [found];
      }));
    } catch (cause) {
      setError(String(cause));
    }
  }

  /**
   * Lance la lecture à partir d'un morceau de la liste affichée.
   *
   * Toute la liste devient la file : lancer un titre depuis une recherche doit
   * enchaîner sur les autres résultats, pas s'arrêter net.
   */
  function playFrom(index: number) {
    void playback.play(
      tracks.map((track) => track.id),
      index,
    );
  }

  function startRadio(trackId: number) {
    setError(null);
    void ipc
      .startRadio(trackId)
      .then(showGenerated)
      .catch((cause: unknown) => setError(String(cause)));
  }

  const reload = useCallback(async () => {
    const [loadedTracks, loadedCounts] = await Promise.all([
      ipc.listTracks(),
      ipc.libraryCounts(),
    ]);
    setTracks(loadedTracks);
    setCounts(loadedCounts);
  }, []);

  // Chargement initial, précédé d'un contrôle de présence des fichiers :
  // le SSD a pu être débranché depuis la dernière session.
  useEffect(() => {
    void ipc
      .refreshAvailability()
      .then(reload)
      .catch((cause: unknown) => setError(String(cause)));
  }, [reload]);

  // Abonnement à la progression des imports.
  const unlisten = useRef<(() => void) | null>(null);
  useEffect(() => {
    void ipc.onScanProgress(setProgress).then((stop) => {
      unlisten.current = stop;
    });
    return () => unlisten.current?.();
  }, []);

  // Recherche différée. Une requête vide réaffiche la bibliothèque complète.
  useEffect(() => {
    const trimmed = query.trim();
    if (trimmed === "") {
      // Une recherche vide ne doit pas balayer la playlist qu'on vient de
      // générer : on ne recharge la bibliothèque que si rien n'est affiché.
      return;
    }

    const timer = setTimeout(() => {
      setPlaylist(null);
      void ipc
        .searchTracks(trimmed)
        .then(setTracks)
        .catch((cause: unknown) => setError(String(cause)));
    }, SEARCH_DEBOUNCE_MS);

    return () => clearTimeout(timer);
  }, [query]);

  async function importFolder() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Choisir le dossier à importer",
    });
    if (typeof selected !== "string") return;

    setError(null);
    setSummary(null);
    setProgress({
      processed: 0,
      total: 0,
      imported: 0,
      duplicates: 0,
      failed: 0,
      current: "",
    });

    try {
      const result = await ipc.importFolder(selected);
      setSummary(result);
      await reload();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setProgress(null);
    }
  }

  return (
    <div className="flex h-full flex-col">
      <header className="drag-region shrink-0 border-b border-line px-5 pb-3 pt-9">
        <div className="no-drag flex items-center gap-3">
          <input
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Rechercher un titre, un artiste, un album…"
            className="flex-1 rounded-lg border border-line bg-surface px-3 py-2 text-sm text-ink placeholder:text-ink-faint focus:border-accent focus:outline-none"
          />

          <button
            type="button"
            disabled={importing}
            onClick={() => void importFolder()}
            className="shrink-0 rounded-lg bg-gradient-to-br from-accent to-accent-alt px-4 py-2 text-sm font-medium text-base transition-opacity hover:opacity-90 disabled:opacity-40"
          >
            {importing ? "Import en cours…" : "Importer un dossier"}
          </button>
        </div>

        <Counters counts={counts} libraryRoot={libraryRoot} />

        <DiscoverBar
          disabled={importing || (counts?.tracks ?? 0) === 0}
          onGenerated={(generated) => void showGenerated(generated)}
          onError={setError}
        />
      </header>

      {playlist !== null && (
        <PlaylistBanner
          playlist={playlist}
          onDismiss={() => {
            setPlaylist(null);
            void reload();
          }}
        />
      )}

      {progress !== null && <ProgressBar progress={progress} />}
      {summary !== null && <SummaryBanner summary={summary} onDismiss={() => setSummary(null)} />}
      {error !== null && (
        <p className="border-b border-danger/25 bg-danger/5 px-5 py-2.5 text-xs text-danger">
          {error}
        </p>
      )}

      {playback.error !== null && (
        <p className="border-b border-danger/25 bg-danger/5 px-5 py-2.5 text-xs text-danger">
          {playback.error}
        </p>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto">
        <TrackList
          tracks={tracks}
          currentTrackId={playback.state?.current?.trackId ?? null}
          isPlaying={playback.state?.isPlaying ?? false}
          onPlay={playFrom}
          onRadio={startRadio}
          {...(reasons === undefined ? {} : { reasons })}
        />
      </div>

      {playback.state !== null && (
        <PlayerBar
          state={playback.state}
          onToggle={() => void playback.toggle()}
          onNext={() => void playback.next()}
          onPrevious={() => void playback.previous()}
          onSeek={(position) => void playback.seek(position)}
          onVolume={(volume) => void playback.setVolume(volume)}
          onShuffle={(shuffle) => void playback.toggleShuffle(shuffle)}
          onRepeat={() => void playback.cycleRepeat(playback.state?.repeat ?? "off")}
        />
      )}
    </div>
  );
}

/** Bandeau d'une playlist générée : ce que le moteur a produit, et pourquoi. */
function PlaylistBanner({
  playlist,
  onDismiss,
}: {
  playlist: GeneratedPlaylist;
  onDismiss: () => void;
}) {
  return (
    <div className="flex items-center gap-4 border-b border-line bg-gradient-to-r from-accent/10 to-transparent px-5 py-3">
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm font-medium text-ink">{playlist.title}</p>
        <p className="truncate text-xs text-ink-muted">
          {playlist.subtitle}
          <span className="mx-1.5 text-ink-faint">·</span>
          {playlist.tracks.length} morceaux
        </p>
      </div>

      <button
        type="button"
        onClick={onDismiss}
        className="shrink-0 text-xs text-ink-faint transition-colors hover:text-ink"
      >
        Revenir à la bibliothèque
      </button>
    </div>
  );
}

function Counters({
  counts,
  libraryRoot,
}: {
  counts: LibraryCounts | null;
  libraryRoot: string;
}) {
  return (
    <p className="mt-2.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-ink-muted">
      {counts !== null && (
        <>
          <span>
            {counts.tracks.toLocaleString("fr-FR")} morceau
            {counts.tracks > 1 ? "x" : ""}
          </span>
          <span className="text-ink-faint">·</span>
          <span>{counts.artists.toLocaleString("fr-FR")} artistes</span>
          <span className="text-ink-faint">·</span>
          <span>{counts.albums.toLocaleString("fr-FR")} albums</span>
          {counts.unavailable > 0 && (
            <>
              <span className="text-ink-faint">·</span>
              <span className="text-warn">{counts.unavailable} hors ligne</span>
            </>
          )}
          <span className="text-ink-faint">·</span>
        </>
      )}
      <span className="truncate font-mono text-ink-faint">{libraryRoot}</span>
    </p>
  );
}

function ProgressBar({ progress }: { progress: ScanProgress }) {
  const ratio = progress.total === 0 ? 0 : progress.processed / progress.total;

  return (
    <div className="border-b border-line px-5 py-3">
      <div className="h-1 overflow-hidden rounded-full bg-elevated">
        <div
          className="h-full rounded-full bg-gradient-to-r from-accent to-accent-alt transition-[width] duration-150"
          style={{ width: `${ratio * 100}%` }}
        />
      </div>

      <p className="mt-2 flex items-center justify-between gap-4 text-xs text-ink-muted">
        <span className="truncate">{progress.current || "Analyse du dossier…"}</span>
        <span className="shrink-0 tabular-nums">
          {progress.processed} / {progress.total}
        </span>
      </p>
    </div>
  );
}

function SummaryBanner({
  summary,
  onDismiss,
}: {
  summary: ScanSummary;
  onDismiss: () => void;
}) {
  return (
    <div className="border-b border-line bg-surface px-5 py-3">
      <div className="flex items-start justify-between gap-4">
        <div className="text-xs text-ink-muted">
          <p>
            <span className="text-ok">{summary.imported} importé(s)</span>
            {summary.duplicates > 0 && <> · {summary.duplicates} doublon(s) ignoré(s)</>}
            {summary.failed > 0 && (
              <> · <span className="text-danger">{summary.failed} échec(s)</span></>
            )}
          </p>

          {summary.errors.length > 0 && (
            <ul className="mt-1.5 space-y-0.5 text-ink-faint">
              {summary.errors.map((message) => (
                <li key={message} className="truncate">
                  {message}
                </li>
              ))}
            </ul>
          )}
        </div>

        <button
          type="button"
          onClick={onDismiss}
          className="shrink-0 text-xs text-ink-faint hover:text-ink"
        >
          Fermer
        </button>
      </div>
    </div>
  );
}
