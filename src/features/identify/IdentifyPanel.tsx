import { useEffect, useState } from "react";

import { ipc, type IdentificationStatus } from "@/lib/ipc";

/**
 * Panneau d'identification par empreinte acoustique.
 *
 * Trois états, trois affichages :
 *
 * | Situation | Ce qui s'affiche |
 * |---|---|
 * | Clé absente | Une invitation discrète à la renseigner |
 * | Identification en cours | Une jauge |
 * | Terminé | Rien — une jauge à 100 % n'apprend plus rien |
 */
const POLL_MS = 5000;

export function IdentifyPanel() {
  const [status, setStatus] = useState<IdentificationStatus | null>(null);
  const [editing, setEditing] = useState(false);
  const [key, setKey] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const read = () => {
      void ipc.identificationStatus().then(setStatus).catch(() => undefined);
    };

    read();
    const timer = setInterval(read, POLL_MS);
    return () => clearInterval(timer);
  }, []);

  async function save() {
    setError(null);
    try {
      await ipc.setAcoustidKey(key);
      setKey("");
      setEditing(false);
      setStatus(await ipc.identificationStatus());
    } catch (cause) {
      setError(String(cause));
    }
  }

  if (status === null) return null;

  // ── Saisie de la clé ──────────────────────────────────────────────────
  if (editing) {
    return (
      <div className="rounded-xl border border-line bg-surface p-3">
        <p className="text-xs text-ink-muted">
          Colle ta clé AcoustID. Elle est gratuite et s'obtient en une minute
          sur{" "}
          <span className="font-mono text-ink-faint">
            acoustid.org/new-application
          </span>
          .
        </p>

        <div className="mt-2.5 flex items-center gap-2">
          <input
            type="password"
            value={key}
            autoFocus
            onChange={(event) => setKey(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void save();
              if (event.key === "Escape") setEditing(false);
            }}
            placeholder="clé AcoustID"
            className="flex-1 rounded-lg border border-line bg-base px-3 py-1.5 font-mono text-xs text-ink placeholder:text-ink-faint focus:border-accent focus:outline-none"
          />
          <button
            type="button"
            onClick={() => void save()}
            className="rounded-lg bg-gradient-to-br from-accent to-accent-alt px-3 py-1.5 text-xs font-medium text-base"
          >
            Enregistrer
          </button>
          <button
            type="button"
            onClick={() => setEditing(false)}
            className="text-xs text-ink-faint hover:text-ink"
          >
            Annuler
          </button>
        </div>

        {error !== null && <p className="mt-2 text-xs text-danger">{error}</p>}
      </div>
    );
  }

  // ── Invitation à configurer ───────────────────────────────────────────
  if (!status.configured) {
    return (
      <button
        type="button"
        onClick={() => setEditing(true)}
        className="group flex w-full items-center gap-2.5 rounded-xl border border-dashed border-line px-3 py-2 text-left transition-colors hover:border-accent/50"
      >
        <Wand />
        <span className="min-w-0 flex-1">
          <span className="block text-xs text-ink">
            Retrouver automatiquement titres, albums et pochettes
          </span>
          <span className="block text-[11px] text-ink-faint">
            Onzer reconnaît tes morceaux au son. Une clé gratuite suffit.
          </span>
        </span>
        <span className="shrink-0 text-xs text-accent opacity-0 transition-opacity group-hover:opacity-100">
          Configurer
        </span>
      </button>
    );
  }

  // ── En cours ──────────────────────────────────────────────────────────
  const { progress } = status;
  if (progress.pending === 0) return null;

  const done = progress.identified + progress.notFound + progress.failed;
  const ratio = progress.total === 0 ? 0 : done / progress.total;

  return (
    <div
      className="flex items-center gap-2.5 rounded-xl border border-line bg-surface px-3 py-2"
      title="Onzer identifie tes morceaux à l'oreille, puis corrige leurs tags et les range."
    >
      <Wand />
      <div className="min-w-0 flex-1">
        <div className="h-1 overflow-hidden rounded-full bg-elevated">
          <div
            className="h-full rounded-full bg-gradient-to-r from-accent to-accent-alt transition-[width] duration-500"
            style={{ width: `${ratio * 100}%` }}
          />
        </div>
        <p className="mt-1.5 text-[11px] text-ink-faint">
          identification {done}/{progress.total}
          {progress.notFound > 0 && ` · ${progress.notFound} introuvable(s)`}
        </p>
      </div>
    </div>
  );
}

/** Baguette : le geste « magique » de l'identification. */
function Wand() {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-4 w-4 shrink-0 text-accent"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="m15 4 5 5L9 20H4v-5z" />
      <path d="M14 5 19 10" />
      <path d="M5 3v4M3 5h4" />
    </svg>
  );
}
