import { useState } from "react";

import { CommandBox } from "@/features/sync/SyncView";
import { Icon } from "@/components/Icon";
import { ipc, type ExportedList } from "@/lib/ipc";

/**
 * Les morceaux dont le fichier a disparu.
 *
 * # Pourquoi une liste, et pas seulement un compteur
 *
 * « 154 hors ligne » informe sans aider. Un morceau hors ligne n'est pas
 * perdu — sa ligne, son historique et ses playlists sont intacts, seul le
 * fichier manque — mais le récupérer suppose de savoir **lesquels**, et les
 * lire à l'écran cent cinquante fois n'est pas une réponse.
 *
 * La forme « Artiste - Titre » n'est pas décorative : c'est celle que lisent
 * `spotdl`, `yt-dlp` et les convertisseurs de playlist. Le fichier produit se
 * colle tel quel là où on en a besoin.
 */
export function OfflineBar({ count }: { count: number }) {
  const [exported, setExported] = useState<ExportedList | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);

  if (count === 0) return null;

  async function exportList() {
    setBusy(true);
    setError(null);

    try {
      const queries = await ipc.offlineTracks();
      setExported(await ipc.exportQueries(queries, "_hors-ligne"));
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  }

  function copy(text: string, key: string) {
    void navigator.clipboard.writeText(text);
    setCopied(key);
    setTimeout(() => setCopied(null), 2000);
  }

  return (
    <div className="rounded-xl bg-elevated px-3 py-2">
      <div className="flex items-center gap-2.5">
        <span className="shrink-0 text-warn">
          <Icon name="folder" size={16} />
        </span>

        <div className="min-w-0 flex-1">
          <p className="truncate text-xs text-ink">Fichiers manquants</p>
          <p className="truncate text-[11px] text-ink-faint">
            {count} morceau{count > 1 ? "x" : ""} hors ligne
          </p>
        </div>

        <button
          type="button"
          disabled={busy}
          onClick={() => void exportList()}
          className="shrink-0 rounded-full bg-raised px-3 py-1 text-[11px] font-semibold text-ink-muted transition-colors hover:text-ink disabled:opacity-40"
        >
          {busy ? "…" : "Exporter la liste"}
        </button>
      </div>

      {error !== null && <p className="mt-2 text-[11px] text-danger">{error}</p>}

      {exported !== null && (
        <div className="mt-2.5 space-y-2 border-t border-line pt-2.5">
          <p className="text-[11px] leading-relaxed text-ink-faint">
            {exported.count} ligne{exported.count > 1 ? "s" : ""} au format
            «&nbsp;Artiste - Titre&nbsp;» dans{" "}
            <span className="font-mono text-ink-muted">{exported.path}</span>. À
            coller dans un convertisseur de playlist, ou à donner directement à
            l&apos;une de ces commandes.
          </p>

          <CommandBox
            title="Tout récupérer, métadonnées comprises"
            command={exported.spotdlCommand}
            copied={copied === "spotdl"}
            onCopy={() => copy(exported.spotdlCommand, "spotdl")}
            note="spotdl tague les fichiers et récupère les pochettes lui-même."
            quiet
          />

          <CommandBox
            title="Repasser sur les manquants"
            command={exported.command}
            copied={copied === "ytdlp"}
            onCopy={() => copy(exported.command, "ytdlp")}
            note="yt-dlp ne dépend d'aucun accès à Spotify — à lancer pour ce que spotdl n'a pas pu prendre."
            quiet
          />
        </div>
      )}
    </div>
  );
}
