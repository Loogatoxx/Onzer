import { useEffect, useState } from "react";

import { ipc, type AppStatus } from "@/lib/ipc";

/**
 * Écran de démarrage et de diagnostic.
 *
 * Provisoire, mais pas décoratif : il valide de bout en bout la chaîne
 * React → IPC → Rust → SQLite. Il sera remplacé par la véritable interface
 * (bibliothèque + lecteur) à l'étape suivante.
 */
export function App() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    ipc
      .appStatus()
      .then(setStatus)
      .catch((cause: unknown) => setError(String(cause)));
  }, []);

  return (
    <div className="relative flex h-full flex-col overflow-hidden bg-base">
      <AmbientGlow />

      {/* Barre de titre masquée : cette zone reste saisissable à la souris. */}
      <header className="drag-region h-9 shrink-0" />

      <main className="relative flex flex-1 items-center justify-center px-8 pb-12">
        <div className="w-full max-w-lg">
          <Branding />

          <div className="mt-10">
            {error !== null ? (
              <ErrorPanel message={error} />
            ) : status === null ? (
              <LoadingPanel />
            ) : (
              <StatusPanel status={status} />
            )}
          </div>
        </div>
      </main>
    </div>
  );
}

/** Halo d'ambiance : donne de la profondeur au fond sans image. */
function AmbientGlow() {
  return (
    <div aria-hidden className="pointer-events-none absolute inset-0 overflow-hidden">
      <div className="absolute -top-40 left-1/2 h-[32rem] w-[32rem] -translate-x-1/2 rounded-full bg-accent/12 blur-[120px]" />
      <div className="absolute -bottom-52 -right-24 h-[26rem] w-[26rem] rounded-full bg-accent-alt/8 blur-[120px]" />
    </div>
  );
}

function Branding() {
  return (
    <div className="text-center">
      <div className="mx-auto flex h-16 w-16 items-center justify-center rounded-2xl bg-gradient-to-br from-accent to-accent-alt shadow-lg shadow-accent/20">
        <svg viewBox="0 0 24 24" className="h-8 w-8 text-base" aria-hidden>
          <path
            fill="currentColor"
            d="M12 3v10.55A4 4 0 1 0 14 17V7h4V3h-6Z"
          />
        </svg>
      </div>

      <h1 className="mt-5 text-3xl font-semibold tracking-tight text-ink">Onzer</h1>
      <p className="mt-1.5 text-sm text-ink-muted">
        Ta musique. Ton disque. Aucun cloud.
      </p>
    </div>
  );
}

function LoadingPanel() {
  return (
    <div className="flex items-center justify-center gap-2.5 text-sm text-ink-muted">
      <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-accent" />
      Ouverture de la base de données…
    </div>
  );
}

function ErrorPanel({ message }: { message: string }) {
  return (
    <div className="rounded-xl border border-danger/25 bg-danger/5 p-4">
      <p className="text-sm font-medium text-danger">Backend injoignable</p>
      <p className="mt-1.5 text-xs leading-relaxed text-ink-muted">{message}</p>
      <p className="mt-3 text-xs text-ink-faint">
        Attendu si la page est ouverte dans un navigateur : Onzer doit être lancé
        via <code className="text-ink-muted">npm run app</code>.
      </p>
    </div>
  );
}

function StatusPanel({ status }: { status: AppStatus }) {
  const library = describeLibrary(status);

  return (
    <div className="overflow-hidden rounded-xl border border-line bg-surface/70 backdrop-blur">
      <Row label="Schéma de base" value={`v${status.schemaVersion}`} tone="ok" />
      <Row label="Morceaux" value={status.trackCount.toLocaleString("fr-FR")} />
      <Row label="Écoutes enregistrées" value={status.eventCount.toLocaleString("fr-FR")} />
      <Row label="Bibliothèque" value={library.label} tone={library.tone} />

      <p className="border-t border-line px-4 py-3 font-mono text-[11px] leading-relaxed text-ink-faint">
        {status.databasePath}
      </p>
    </div>
  );
}

/**
 * Traduit l'état de la bibliothèque en langage humain.
 *
 * Les trois cas sont volontairement distincts : « pas configurée » n'est pas
 * une anomalie, et « hors ligne » n'est pas une perte de données (ADR-006).
 */
function describeLibrary(status: AppStatus): { label: string; tone: Tone } {
  if (status.libraryRoot === null) {
    return { label: "à configurer", tone: "muted" };
  }
  if (!status.libraryOnline) {
    return { label: "hors ligne — volume débranché", tone: "warn" };
  }
  return { label: status.libraryRoot, tone: "ok" };
}

type Tone = "ok" | "warn" | "muted";

const TONE_CLASS: Record<Tone, string> = {
  ok: "text-ok",
  warn: "text-warn",
  muted: "text-ink-muted",
};

function Row({
  label,
  value,
  tone = "muted",
}: {
  label: string;
  value: string;
  tone?: Tone;
}) {
  return (
    <div className="flex items-center justify-between gap-6 border-b border-line px-4 py-3 last:border-b-0">
      <span className="text-sm text-ink-muted">{label}</span>
      <span className={`truncate text-sm font-medium ${TONE_CLASS[tone]}`}>{value}</span>
    </div>
  );
}
