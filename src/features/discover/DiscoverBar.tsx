import { useEffect, useState } from "react";

import { ipc, type AnalysisProgress, type GeneratedPlaylist } from "@/lib/ipc";

/**
 * Barre du moteur de recommandation.
 *
 * L'utilisateur doit **sentir que le moteur est là**, prêt à tourner — sans
 * qu'il occupe toute la place. D'où une rangée de pastilles discrètes, et une
 * jauge d'analyse qui ne s'affiche que tant qu'il reste du travail.
 */
const ANALYSIS_POLL_MS = 4000;

interface DiscoverBarProps {
  disabled: boolean;
  onGenerated: (playlist: GeneratedPlaylist) => void;
  onError: (message: string) => void;
}

export function DiscoverBar({ disabled, onGenerated, onError }: DiscoverBarProps) {
  const [progress, setProgress] = useState<AnalysisProgress | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  // L'analyse tourne en fond : on interroge son avancement plutôt que de
  // recevoir un événement par morceau, ce qui inonderait l'interface.
  useEffect(() => {
    const read = () => {
      void ipc.analysisProgress().then(setProgress).catch(() => undefined);
    };

    read();
    const timer = setInterval(read, ANALYSIS_POLL_MS);
    return () => clearInterval(timer);
  }, []);

  async function launch(kind: string, action: () => Promise<GeneratedPlaylist>) {
    setBusy(kind);
    try {
      onGenerated(await action());
    } catch (cause) {
      onError(String(cause));
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="no-drag mt-3 flex flex-wrap items-center gap-2">
      <Pill
        label="Pour toi maintenant"
        hint="Ce que tu écoutes habituellement à cette heure-ci"
        primary
        loading={busy === "now"}
        disabled={disabled}
        onClick={() => void launch("now", () => ipc.startForNow())}
      >
        <path d="m12 3 2.2 5.6L20 10l-4.4 3.3L16.8 19 12 15.9 7.2 19l1.2-5.7L4 10l5.8-1.4Z" />
      </Pill>

      <Pill
        label="Tu les avais oubliés"
        hint="Aimés autrefois, plus écoutés depuis longtemps"
        loading={busy === "forgotten"}
        disabled={disabled}
        onClick={() => void launch("forgotten", () => ipc.startForgotten())}
      >
        <circle cx="12" cy="12" r="9" />
        <path d="M12 7v5l3 2" />
      </Pill>

      {progress !== null && <AnalysisGauge progress={progress} />}
    </div>
  );
}

/**
 * Jauge d'analyse.
 *
 * Disparaît une fois le travail terminé : une jauge à 100 % en permanence
 * n'apprend plus rien et encombre.
 */
function AnalysisGauge({ progress }: { progress: AnalysisProgress }) {
  if (progress.total === 0 || progress.pending === 0) {
    return null;
  }

  const ratio = progress.analyzed / progress.total;

  return (
    <div
      className="ml-auto flex items-center gap-2.5"
      title="Onzer analyse ta musique en tâche de fond : timbre, tempo, tonalité. La recommandation s'affine à mesure."
    >
      <div className="h-1 w-24 overflow-hidden rounded-full bg-elevated">
        <div
          className="h-full rounded-full bg-accent transition-[width] duration-500"
          style={{ width: `${ratio * 100}%` }}
        />
      </div>
      <span className="text-[11px] tabular-nums text-ink-faint">
        analyse {progress.analyzed}/{progress.total}
      </span>
    </div>
  );
}

function Pill({
  label,
  hint,
  primary = false,
  loading = false,
  disabled = false,
  onClick,
  children,
}: {
  label: string;
  hint: string;
  primary?: boolean;
  loading?: boolean;
  disabled?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={hint}
      disabled={disabled || loading}
      onClick={onClick}
      className={`flex items-center gap-1.5 rounded-full px-3 py-1.5 text-xs font-medium transition-all disabled:opacity-40 ${
        primary
          ? "bg-gradient-to-br from-accent to-accent-alt text-base hover:opacity-90"
          : "border border-line bg-surface text-ink-muted hover:border-accent/40 hover:text-ink"
      }`}
    >
      <svg
        viewBox="0 0 24 24"
        className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`}
        fill="none"
        stroke="currentColor"
        strokeWidth={1.8}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden
      >
        {loading ? <path d="M21 12a9 9 0 1 1-6.2-8.6" /> : children}
      </svg>
      {label}
    </button>
  );
}
