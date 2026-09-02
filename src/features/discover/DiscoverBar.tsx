import { useEffect, useState } from "react";

import { Icon, type IconName } from "@/components/Icon";
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
    <div
      // Centrés sur un écran étroit : deux boutons alignés à gauche sous un
      // titre centré donnent une composition bancale.
      className="flex flex-wrap items-center justify-center gap-2 sm:justify-start"
    >
      <Pill
        icon="sparkle"
        label="Pour toi maintenant"
        hint="Ce que tu écoutes habituellement à cette heure-ci"
        primary
        loading={busy === "now"}
        disabled={disabled}
        onClick={() => void launch("now", () => ipc.startForNow())}
      />

      <Pill
        icon="clock"
        label="Tu les avais oubliés"
        hint="Aimés autrefois, plus écoutés depuis longtemps"
        loading={busy === "forgotten"}
        disabled={disabled}
        onClick={() => void launch("forgotten", () => ipc.startForgotten())}
      />

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
          className="h-full rounded-full bg-ink-faint transition-[width] duration-500"
          style={{ width: `${ratio * 100}%` }}
        />
      </div>
      <span className="numerals text-[11px] text-ink-faint">
        analyse {progress.analyzed}/{progress.total}
      </span>
    </div>
  );
}

/**
 * Pastille d'action.
 *
 * Une seule est pleine — celle qu'on veut voir cliquée. Deux boutons de même
 * poids ne guident plus vers rien.
 */
function Pill({
  icon,
  label,
  hint,
  primary = false,
  loading = false,
  disabled = false,
  onClick,
}: {
  icon: IconName;
  label: string;
  hint: string;
  primary?: boolean;
  loading?: boolean;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={hint}
      disabled={disabled || loading}
      onClick={onClick}
      className={`flex items-center gap-2 rounded-full px-4 py-2 text-[13px] font-semibold transition-all disabled:opacity-40 ${
        primary
          ? "bg-ink text-base hover:opacity-90"
          : "bg-elevated text-ink-muted hover:text-ink"
      }`}
    >
      <span className={loading ? "animate-spin" : ""}>
        <Icon name={loading ? "repeat" : icon} size={15} />
      </span>
      {label}
    </button>
  );
}
