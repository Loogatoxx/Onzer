import { useEffect, useState } from "react";

import { Icon } from "@/components/Icon";
import { ipc, type WhisperStatus } from "@/lib/ipc";

/**
 * Synchronisation à l'oreille.
 *
 * # Ce que ce bandeau propose, et ce qu'il ne promet pas
 *
 * Deux opérations de natures très différentes, et il serait malhonnête de les
 * présenter comme une seule :
 *
 * - **Caler** des paroles qu'on possède déjà : les mots restent ceux de
 *   l'auteur, seule la minuterie vient du modèle. La pire erreur possible est
 *   un décalage de quelques secondes.
 * - **Transcrire** un morceau sans paroles : là, c'est le modèle qui écrit le
 *   texte, et il se trompe. C'est pourquoi c'est une case à cocher séparée,
 *   décochée par défaut, et que le résultat porte une signature.
 *
 * # Pourquoi le temps est annoncé d'avance
 *
 * Trente à quarante secondes par morceau, sur une machine qui chauffe. Ne pas
 * le dire, c'est laisser croire à un bouton instantané et faire découvrir la
 * vérité une heure plus tard.
 */
const POLL_MS = 1500;

export function ListenBar() {
  const [status, setStatus] = useState<WhisperStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [transcribeEmpty, setTranscribeEmpty] = useState(false);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const read = () => {
      void ipc.whisperStatus().then(setStatus).catch(() => undefined);
    };

    read();
    const timer = setInterval(read, POLL_MS);
    return () => clearInterval(timer);
  }, []);

  if (status === null) return null;

  const pending = status.toAlign + (transcribeEmpty ? status.toTranscribe : 0);

  // Rien à caler et rien en cours : le bandeau s'efface.
  if (status.toAlign === 0 && status.toTranscribe === 0 && !status.running) {
    return null;
  }

  const minutes = Math.round((pending * 35) / 60);

  return (
    <div className="rounded-xl bg-elevated px-3 py-2">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        className="flex w-full items-center gap-2.5 text-left"
      >
        <span
          className={`shrink-0 ${status.running ? "animate-pulse text-accent" : "text-ink-faint"}`}
        >
          <Icon name="sparkle" size={16} />
        </span>

        <span className="min-w-0 flex-1">
          <span className="block truncate text-xs text-ink">Synchroniser à l&apos;oreille</span>
          <span className="block truncate text-[11px] text-ink-faint">
            {status.running
              ? `${status.done}/${status.total} morceaux écoutés…`
              : `${status.toAlign} à caler · ${status.toTranscribe} sans paroles`}
          </span>
        </span>

        <span className={`shrink-0 text-ink-faint transition-transform ${open ? "rotate-180" : ""}`}>
          <Icon name="chevronDown" size={16} />
        </span>
      </button>

      {status.running && (
        <div className="mt-2 h-1 overflow-hidden rounded-full bg-raised">
          <div
            className="h-full rounded-full bg-accent transition-[width] duration-500"
            style={{ width: `${status.total === 0 ? 0 : (status.done / status.total) * 100}%` }}
          />
        </div>
      )}

      {open && (
        <div className="mt-2 space-y-2 border-t border-line pt-2">
          {!status.ready ? (
            <p className="text-[11px] leading-relaxed text-ink-faint">
              Il manque de quoi écouter :{" "}
              {status.missing?.whisper === true && (
                <span className="font-mono">brew install whisper-cpp</span>
              )}
              {status.missing?.ffmpeg === true && (
                <span className="font-mono"> brew install ffmpeg</span>
              )}
              {status.missing?.model === true && " un modèle .bin de whisper.cpp"}
            </p>
          ) : (
            <>
              <p className="text-[11px] leading-relaxed text-ink-faint">
                Un modèle écoute le morceau sur ta machine — rien ne part sur le
                réseau — et Onzer cale <strong className="font-semibold text-ink-muted">tes</strong>{" "}
                paroles sur ce qu&apos;il entend. Les mots affichés restent ceux
                de l&apos;auteur ; seule la minuterie vient du modèle.
              </p>

              <label className="flex cursor-pointer items-start gap-2 text-[11px] leading-relaxed text-ink-faint">
                <input
                  type="checkbox"
                  checked={transcribeEmpty}
                  onChange={(event) => setTranscribeEmpty(event.target.checked)}
                  className="mt-0.5 accent-[var(--color-accent)]"
                />
                <span>
                  Transcrire aussi les {status.toTranscribe} morceaux sans
                  aucune parole. Là, c&apos;est le modèle qui écrit le texte :
                  il se trompe, et le résultat est signé « transcription
                  automatique ».
                </span>
              </label>

              <p className="text-[11px] text-ink-faint">
                {pending} morceau{pending > 1 ? "x" : ""} à écouter, environ{" "}
                {minutes} minute{minutes > 1 ? "s" : ""} de calcul.
              </p>
            </>
          )}

          {error !== null && <p className="text-[11px] text-danger">{error}</p>}

          <button
            type="button"
            disabled={!status.ready || (pending === 0 && !status.running)}
            onClick={() => {
              setError(null);
              if (status.running) {
                void ipc.stopListening().catch((cause: unknown) => setError(String(cause)));
              } else {
                void ipc
                  .listenAndSync(transcribeEmpty)
                  .catch((cause: unknown) => setError(String(cause)));
              }
            }}
            className="w-full rounded-full bg-raised px-3 py-1.5 text-[11px] font-semibold text-ink-muted transition-colors hover:text-ink disabled:opacity-40"
          >
            {status.running ? "Arrêter après ce morceau" : "Lancer l'écoute"}
          </button>
        </div>
      )}
    </div>
  );
}
