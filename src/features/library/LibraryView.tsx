import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import {
  ipc,
  type LibraryCounts,
  type ScanProgress,
  type ScanSummary,
  type TrackSummary,
} from "@/lib/ipc";
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

  const importing = progress !== null;

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
    const timer = setTimeout(() => {
      const request = trimmed === "" ? ipc.listTracks() : ipc.searchTracks(trimmed);
      void request.then(setTracks).catch((cause: unknown) => setError(String(cause)));
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
      </header>

      {progress !== null && <ProgressBar progress={progress} />}
      {summary !== null && <SummaryBanner summary={summary} onDismiss={() => setSummary(null)} />}
      {error !== null && (
        <p className="border-b border-danger/25 bg-danger/5 px-5 py-2.5 text-xs text-danger">
          {error}
        </p>
      )}

      <div className="flex-1 overflow-y-auto">
        <TrackList tracks={tracks} />
      </div>
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
