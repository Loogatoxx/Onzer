import { useCallback, useEffect, useState } from "react";

import { AppShell } from "@/app/AppShell";
import { SetupView } from "@/features/library/SetupView";
import { ipc, type AppStatus } from "@/lib/ipc";

/**
 * Racine de l'application.
 *
 * Aiguille sur trois états, volontairement distincts (ADR-006) :
 *
 * * pas de racine configurée  → écran de configuration ;
 * * racine configurée mais volume absent → bandeau d'avertissement, la
 *   bibliothèque reste consultable ;
 * * tout va bien → la bibliothèque.
 */
export function App() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    ipc
      .appStatus()
      .then(setStatus)
      .catch((cause: unknown) => setError(String(cause)));
  }, []);

  useEffect(refresh, [refresh]);

  if (error !== null) {
    return (
      <Centered>
        <BackendError message={error} />
      </Centered>
    );
  }

  if (status === null) {
    return (
      <Centered>
        <p className="text-sm text-ink-muted">Ouverture de la base…</p>
      </Centered>
    );
  }

  if (status.libraryRoot === null) {
    return (
      <Centered>
        <Branding />
        <div className="mt-10">
          <SetupView onReady={refresh} />
        </div>
      </Centered>
    );
  }

  return (
    <div className="flex h-full flex-col bg-base">
      {!status.libraryOnline && <OfflineBanner root={status.libraryRoot} />}
      <div className="min-h-0 flex-1">
        <AppShell libraryRoot={status.libraryRoot} />
      </div>
    </div>
  );
}

function Centered({ children }: { children: React.ReactNode }) {
  return (
    <div className="relative flex h-full flex-col bg-base">
      <AmbientGlow />
      <div className="drag-region h-9 shrink-0" />
      <main className="relative flex flex-1 items-center justify-center px-8 pb-12">
        <div className="w-full max-w-lg">{children}</div>
      </main>
    </div>
  );
}

/**
 * Halo d'ambiance de l'écran d'accueil.
 *
 * Un dégradé radial, et non un cercle flouté : un disque en `blur` garde un
 * bord — très doux, mais présent — que l'œil finit toujours par repérer sur un
 * fond aussi sombre. Le dégradé, lui, s'éteint mathématiquement à zéro.
 */
function AmbientGlow() {
  return (
    <div
      aria-hidden
      className="pointer-events-none absolute inset-0"
      style={{
        backgroundImage:
          "radial-gradient(60rem 32rem at 50% -12rem, rgba(139,92,246,0.10), transparent 70%)",
      }}
    />
  );
}

function Branding() {
  return (
    <div className="text-center">
      <div className="mx-auto flex h-16 w-16 items-center justify-center rounded-2xl bg-gradient-to-br from-accent to-accent-alt shadow-lg shadow-accent/20">
        <svg viewBox="0 0 24 24" className="h-8 w-8 text-base" aria-hidden>
          <path fill="currentColor" d="M12 3v10.55A4 4 0 1 0 14 17V7h4V3h-6Z" />
        </svg>
      </div>
      <h1 className="mt-5 text-3xl font-semibold tracking-tight text-ink">Onzer</h1>
      <p className="mt-1.5 text-sm text-ink-muted">Ta musique. Ton disque. Aucun cloud.</p>
    </div>
  );
}

/**
 * Le volume est débranché. Rien n'est perdu : la bibliothèque, les playlists
 * et les statistiques restent consultables, seuls les fichiers sont absents.
 */
function OfflineBanner({ root }: { root: string }) {
  return (
    <div className="shrink-0 border-b border-warn/25 bg-warn/5 px-5 py-2.5">
      <p className="text-xs text-warn">
        Bibliothèque hors ligne — le volume contenant{" "}
        <span className="font-mono">{root}</span> n'est pas monté. Tes morceaux
        sont toujours là, seuls les fichiers sont inaccessibles.
      </p>
    </div>
  );
}

function BackendError({ message }: { message: string }) {
  return (
    <div className="rounded-xl border border-danger/25 bg-danger/5 p-4">
      <p className="text-sm font-medium text-danger">Backend injoignable</p>
      <p className="mt-1.5 text-xs leading-relaxed text-ink-muted">{message}</p>
      <p className="mt-3 text-xs text-ink-faint">
        Attendu si la page est ouverte dans un navigateur : Onzer doit être
        lancé via <code className="text-ink-muted">npm run app</code>.
      </p>
    </div>
  );
}
