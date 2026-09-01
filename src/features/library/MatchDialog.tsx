import { useEffect, useState } from "react";

import { Icon } from "@/components/Icon";
import { ipc, type MetadataCandidate, type TrackSummary } from "@/lib/ipc";

/**
 * Chercher ailleurs quand l'empreinte a échoué.
 *
 * # Pourquoi une liste et non une réponse
 *
 * L'identification automatique n'a qu'une règle : accepter ou refuser. Elle
 * refuse beaucoup — sur cette bibliothèque, 256 morceaux sont restés
 * introuvables et 93 sans album. Refuser est le bon réflexe pour une machine
 * qui décide seule : une fiche fausse écrit un mauvais titre, télécharge les
 * mauvaises paroles et attache la mauvaise pochette.
 *
 * Mais l'utilisateur, lui, **reconnaît son morceau d'un coup d'œil**. Devant
 * lui, une proposition douteuse ne coûte rien : il la laisse. C'est pourquoi
 * cette liste ne filtre pas — elle trie, et montre tout, y compris ce que
 * l'automatique aurait rejeté.
 *
 * # Pourquoi la durée est affichée
 *
 * C'est presque toujours elle qui explique un refus. Un fichier de quatorze
 * minutes en face d'un morceau de trois : ce n'est pas le catalogue qui se
 * trompe, c'est le fichier qui contient tout un projet. L'écart est donc écrit
 * en toutes lettres, pour que le choix se fasse en connaissance de cause.
 */
export function MatchDialog({
  track,
  onClose,
  onApplied,
}: {
  track: TrackSummary;
  onClose: () => void;
  onApplied: () => void;
}) {
  const [candidates, setCandidates] = useState<MetadataCandidate[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [applying, setApplying] = useState<number | null>(null);

  useEffect(() => {
    let alive = true;

    ipc
      .metadataCandidates(track.id)
      .then((found) => {
        if (alive) setCandidates(found);
      })
      .catch((cause: unknown) => {
        if (alive) {
          setError(String(cause));
          setCandidates([]);
        }
      });

    return () => {
      alive = false;
    };
  }, [track.id]);

  async function apply(candidate: MetadataCandidate, index: number) {
    setApplying(index);
    setError(null);

    try {
      await ipc.applyCandidate(track.id, candidate);
      onApplied();
      onClose();
    } catch (cause) {
      setError(String(cause));
      setApplying(null);
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-base/70 p-6 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="flex max-h-[80vh] w-full max-w-2xl flex-col rounded-2xl bg-surface shadow-2xl shadow-black/60"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-4 p-6 pb-4">
          <div className="min-w-0">
            <p className="text-xs font-semibold uppercase tracking-[0.16em] text-ink-faint">
              Chercher ailleurs
            </p>
            <h2 className="display mt-1 truncate text-xl text-ink">{track.title}</h2>
            <p className="mt-1 truncate text-[12px] text-ink-muted">
              {track.artist ?? "Artiste inconnu"} · {formatDuration(track.durationMs)}
            </p>
          </div>

          <button
            type="button"
            aria-label="Fermer"
            onClick={onClose}
            className="shrink-0 text-ink-faint transition-colors hover:text-ink"
          >
            <Icon name="close" size={18} />
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-6">
          {candidates === null && (
            <p className="py-8 text-center text-[13px] text-ink-muted">
              Deezer, iTunes et MusicBrainz sont interrogés…
            </p>
          )}

          {candidates !== null && candidates.length === 0 && (
            <p className="py-8 text-center text-[13px] text-ink-muted">
              Aucun catalogue ne connaît ce morceau sous ce nom. Corriger le
              titre à la main, puis recommencer, donne souvent un résultat.
            </p>
          )}

          <ul className="space-y-1.5 pb-2">
            {(candidates ?? []).map((candidate, index) => (
              <CandidateRow
                key={`${candidate.source}-${candidate.title}-${index}`}
                candidate={candidate}
                fileDurationMs={track.durationMs}
                busy={applying !== null}
                applying={applying === index}
                onApply={() => void apply(candidate, index)}
              />
            ))}
          </ul>
        </div>

        <div className="border-t border-raised p-6 pt-4">
          {error !== null && <p className="mb-3 text-[12px] text-danger">{error}</p>}

          <p className="text-[11px] leading-relaxed text-ink-faint">
            La fiche retenue est écrite dans les tags du fichier, avec sa
            pochette. Les paroles sont effacées si le titre change — elles
            appartenaient à l&apos;ancien.
          </p>
        </div>
      </div>
    </div>
  );
}

function CandidateRow({
  candidate,
  fileDurationMs,
  busy,
  applying,
  onApply,
}: {
  candidate: MetadataCandidate;
  fileDurationMs: number;
  busy: boolean;
  applying: boolean;
  onApply: () => void;
}) {
  const [preview, setPreview] = useState<string | null>(null);
  const url = candidate.previewUrl;

  useEffect(() => {
    if (url === null) return;

    let alive = true;
    ipc
      .candidatePreview(url)
      .then((data) => {
        if (alive) setPreview(data);
      })
      .catch(() => undefined);

    return () => {
      alive = false;
    };
  }, [url]);

  const gap =
    candidate.durationMs === 0 || fileDurationMs === 0
      ? null
      : Math.round((candidate.durationMs - fileDurationMs) / 1000);

  return (
    <li>
      <button
        type="button"
        disabled={busy}
        onClick={onApply}
        className="flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-colors hover:bg-elevated disabled:opacity-50"
      >
        <span className="h-12 w-12 shrink-0 overflow-hidden rounded-md bg-raised">
          {preview !== null ? (
            <img src={preview} alt="" className="h-full w-full object-cover" />
          ) : (
            <span className="flex h-full w-full items-center justify-center text-ink-faint">
              <Icon name="library" size={18} />
            </span>
          )}
        </span>

        <span className="min-w-0 flex-1">
          <span className="block truncate text-[13px] font-medium text-ink">
            {candidate.title}
          </span>
          <span className="block truncate text-[12px] text-ink-muted">
            {candidate.artist ?? "Artiste inconnu"}
            {candidate.album !== null && ` · ${candidate.album}`}
            {candidate.year !== null && ` (${candidate.year})`}
          </span>
        </span>

        <span className="shrink-0 text-right">
          <span className="block text-[10px] font-semibold uppercase tracking-[0.12em] text-ink-faint">
            {candidate.source}
          </span>
          {gap !== null && (
            <span
              className={`block text-[11px] ${
                Math.abs(gap) <= 30 ? "text-ink-muted" : "text-danger"
              }`}
              title="Écart avec la durée du fichier"
            >
              {gap > 0 ? "+" : ""}
              {gap} s
            </span>
          )}
        </span>

        {applying && (
          <span className="shrink-0 text-[11px] text-ink-muted">Application…</span>
        )}
      </button>
    </li>
  );
}

function formatDuration(ms: number): string {
  const total = Math.round(ms / 1000);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}
