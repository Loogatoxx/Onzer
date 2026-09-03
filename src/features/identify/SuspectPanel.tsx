import { useCallback, useEffect, useState } from "react";

import { Icon } from "@/components/Icon";
import { ipc, type SuspectTrack } from "@/lib/ipc";

/**
 * Identifications à vérifier.
 *
 * # Pourquoi Onzer demande au lieu de décider
 *
 * Une identification qui contredit les tags du fichier a le plus souvent
 * **raison** : corriger un fichier mal nommé au téléchargement est même sa
 * raison d'être. Mais elle a parfois tort — un morceau de Damso s'est retrouvé
 * étiqueté « carmen » de Stromae, d'après une fiche MusicBrainz créée depuis un
 * rip YouTube.
 *
 * Onzer ne peut pas trancher à la place de l'utilisateur. Il peut lui montrer
 * les deux versions côte à côte, ce qui suffit presque toujours à décider d'un
 * coup d'œil.
 *
 * Les tags d'origine sont retrouvés dans les exemplaires écartés du dossier
 * `_Doublons` : Onzer ne les a jamais retagués, ils portent donc encore ce que
 * le fichier annonçait au téléchargement.
 */
export function SuspectPanel({
  onRestored,
  onPlay,
}: {
  onRestored: () => void;
  /**
   * Écouter le morceau douteux.
   *
   * # Pourquoi il manquait
   *
   * On demande ici de trancher entre deux versions d'un même titre. Le
   * panneau des doublons, qui pose exactement la même question, offre le
   * bouton depuis toujours : sans lui, il faut quitter la page, retrouver le
   * morceau dans la bibliothèque, l'écouter, revenir. Trancher sans entendre,
   * c'est deviner.
   */
  onPlay: (trackId: number) => void;
}) {
  const [suspects, setSuspects] = useState<SuspectTrack[]>([]);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(() => {
    void ipc.suspectTracks().then(setSuspects).catch(() => undefined);
  }, []);

  useEffect(reload, [reload]);

  if (suspects.length === 0) {
    return null;
  }

  async function restore(trackId: number) {
    setBusy(trackId);
    setError(null);

    try {
      await ipc.restoreOriginalTags(trackId);
      setSuspects((previous) => previous.filter((entry) => entry.id !== trackId));
      onRestored();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="rounded-xl bg-elevated px-3 py-2">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        className="pression flex w-full items-center gap-2.5 rounded-md text-left"
      >
        <span className="shrink-0 text-warn">
          <Icon name="sparkle" size={16} />
        </span>

        <span className="min-w-0 flex-1">
          <span className="block truncate text-xs text-ink">
            {suspects.length} identification{suspects.length > 1 ? "s" : ""} à vérifier
          </span>
          <span className="block truncate text-[11px] text-ink-faint">
            L'empreinte acoustique contredit ce que le fichier annonçait
          </span>
        </span>

        <span className={`shrink-0 text-ink-faint transition-transform ${open ? "rotate-180" : ""}`}>
          <Icon name="chevronDown" size={16} />
        </span>
      </button>

      {open && (
        <ul className="mt-2 space-y-1 border-t border-line pt-2">
          {suspects.map((suspect) => (
            <li
              key={suspect.id}
              className="flex items-center gap-3 rounded-lg px-1 py-1.5"
            >
              <button
                type="button"
                title={`Écouter ${suspect.title}`}
                aria-label={`Écouter ${suspect.title}`}
                onClick={() => onPlay(suspect.id)}
                className="pression flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-raised text-ink-muted hover:bg-ink hover:text-base"
              >
                <span className="translate-x-[1px]">
                  <Icon name="play" size={12} />
                </span>
              </button>

              <div className="min-w-0 flex-1">
                {/* Le fichier d'abord : c'est la version que l'utilisateur
                    reconnaît, et celle qu'il récupérera s'il restaure. */}
                <p className="truncate text-[12px] text-ink">
                  {describe(suspect.originalArtist, suspect.originalTitle)}
                </p>
                <p className="truncate text-[11px] text-ink-faint">
                  devenu {describe(suspect.artist, suspect.title)}
                </p>
              </div>

              <button
                type="button"
                disabled={busy === suspect.id}
                onClick={() => void restore(suspect.id)}
                className="shrink-0 rounded-full bg-raised px-3 py-1 text-[11px] font-semibold text-ink-muted transition-colors hover:text-ink disabled:opacity-40"
              >
                {busy === suspect.id ? "…" : "Rétablir"}
              </button>
            </li>
          ))}

          {error !== null && <p className="px-1 text-[11px] text-danger">{error}</p>}

          <p className="px-1 pt-1 text-[11px] leading-relaxed text-ink-faint">
            Rétablir réécrit les tags d'origine dans le fichier et empêche Onzer
            de reproposer la même correspondance.
          </p>
        </ul>
      )}
    </div>
  );
}

function describe(artist: string | null, title: string): string {
  return artist === null || artist.trim() === "" ? title : `${artist} — ${title}`;
}
