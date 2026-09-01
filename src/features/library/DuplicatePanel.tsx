import { useCallback, useEffect, useState } from "react";

import { Icon } from "@/components/Icon";
import { formatDuration, ipc, type NearDuplicate } from "@/lib/ipc";

/**
 * Doublons probables.
 *
 * # Pourquoi Onzer ne tranche pas tout seul
 *
 * Le dédoublonnage automatique s'arrête là où l'audio diffère réellement : deux
 * fichiers de durées différentes **sont** deux enregistrements différents, et
 * souvent deux versions légitimes — le clip et la version album, un live et un
 * studio. Les fusionner d'autorité ferait disparaître une version que tu
 * voulais garder.
 *
 * Onzer se contente donc de les rapprocher, avec ce qu'il faut pour décider :
 * la durée, l'album, et le nombre d'écoutes — c'est presque toujours celui
 * qu'on a écouté qu'on garde.
 */
export function DuplicatePanel({
  onRemoved,
  onPlay,
  currentTrackId,
  isPlaying,
}: {
  onRemoved: () => void;
  /** Écoute un morceau seul, pour trancher à l'oreille. */
  onPlay: (trackId: number) => void;
  currentTrackId: number | null;
  isPlaying: boolean;
}) {
  const [rows, setRows] = useState<NearDuplicate[]>([]);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(() => {
    void ipc.nearDuplicates().then(setRows).catch(() => undefined);
  }, []);

  useEffect(reload, [reload]);

  // Les lignes arrivent triées par clé de regroupement : il suffit de les
  // découper, sans avoir à trier de nouveau côté interface.
  const found: NearDuplicate[][] = [];
  for (const row of rows) {
    const last = found[found.length - 1];
    if (last !== undefined && last[0]?.groupKey === row.groupKey) last.push(row);
    else found.push([row]);
  }

  // Un groupe retombé à un seul morceau n'est plus un doublon : le problème
  // est réglé, et continuer de demander « est-ce bien un doublon ? » pour un
  // morceau tout seul serait absurde. Il disparaît de lui-même, sans qu'on ait
  // à cliquer sur quoi que ce soit.
  //
  // Le filtre est ici plutôt que dans la requête, qui l'applique déjà : entre
  // deux relectures, l'état local a changé sous nos yeux, et c'est cet
  // état-là que l'utilisateur regarde.
  const groups = found.filter((group) => group.length > 1);

  if (groups.length === 0) {
    return null;
  }

  /**
   * Écarte un groupe déclaré légitime.
   *
   * Deux morceaux peuvent porter le même titre et durer presque pareil sans
   * avoir de rapport — une reprise, deux interludes homonymes. Sans ce bouton,
   * le panneau redemanderait éternellement.
   */
  async function dismiss(groupKey: string) {
    try {
      await ipc.ignoreDuplicateGroup(groupKey);
      setRows((previous) => previous.filter((row) => row.groupKey !== groupKey));
    } catch (cause) {
      setError(String(cause));
    }
  }

  async function remove(trackId: number) {
    setBusy(trackId);
    setError(null);

    try {
      await ipc.removeTrack(trackId);
      setRows((previous) => previous.filter((row) => row.id !== trackId));
      onRemoved();
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
        className="flex w-full items-center gap-2.5 text-left"
      >
        <span className="shrink-0 text-ink-faint">
          <Icon name="library" size={16} />
        </span>

        <span className="min-w-0 flex-1">
          <span className="block truncate text-xs text-ink">
            {groups.length} doublon{groups.length > 1 ? "s" : ""} probable
            {groups.length > 1 ? "s" : ""}
          </span>
          <span className="block truncate text-[11px] text-ink-faint">
            Même titre, durée voisine — à toi de dire lequel garder
          </span>
        </span>

        <span className={`shrink-0 text-ink-faint transition-transform ${open ? "rotate-180" : ""}`}>
          <Icon name="chevronDown" size={16} />
        </span>
      </button>

      {open && (
        <div className="mt-2 space-y-3 border-t border-line pt-2">
          {groups.map((group) => (
            <div key={group[0]?.groupKey ?? ""}>
              <div className="flex items-center justify-between gap-3 px-1">
                <p className="min-w-0 truncate text-[11px] font-semibold uppercase tracking-[0.14em] text-ink-faint">
                  {group[0]?.title}
                </p>

                <button
                  type="button"
                  onClick={() => void dismiss(group[0]?.groupKey ?? "")}
                  className="shrink-0 rounded-full px-2.5 py-0.5 text-[11px] text-ink-faint transition-colors hover:bg-raised hover:text-ink"
                >
                  Ce ne sont pas des doublons
                </button>
              </div>

              <ul className="mt-1">
                {group.map((row) => (
                  <li
                    key={row.id}
                    className="group flex items-center gap-3 rounded-lg px-1 py-1.5 transition-colors hover:bg-raised/40"
                  >
                    {/* Écouter est le seul moyen de trancher entre un clip et
                        sa version album : la durée n'y suffit pas. */}
                    <button
                      type="button"
                      title={`Écouter ${row.title}`}
                      aria-label={`Écouter ${row.title}`}
                      onClick={() => onPlay(row.id)}
                      className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-raised text-ink-muted transition-colors hover:bg-ink hover:text-base"
                    >
                      <span
                        className={
                          currentTrackId === row.id && isPlaying ? "" : "translate-x-[1px]"
                        }
                      >
                        <Icon
                          name={currentTrackId === row.id && isPlaying ? "pause" : "play"}
                          size={12}
                        />
                      </span>
                    </button>

                    <div className="min-w-0 flex-1">
                      <p className="truncate text-[12px] text-ink">
                        {row.album ?? "Sans album"}
                        <span className="mx-1.5 text-ink-faint">·</span>
                        <span className="numerals">{formatDuration(row.durationMs)}</span>
                      </p>
                      <p className="truncate text-[11px] text-ink-faint">
                        {row.playCount === 0
                          ? "jamais écouté"
                          : `${row.playCount} écoute${row.playCount > 1 ? "s" : ""}`}
                        <span className="mx-1.5">·</span>
                        <span className="font-mono">{row.relativePath}</span>
                      </p>
                    </div>

                    <button
                      type="button"
                      disabled={busy === row.id}
                      onClick={() => void remove(row.id)}
                      className="shrink-0 rounded-full bg-raised px-3 py-1 text-[11px] font-semibold text-ink-muted transition-colors hover:text-danger disabled:opacity-40"
                    >
                      {busy === row.id ? "…" : "Retirer"}
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          ))}

          {error !== null && <p className="px-1 text-[11px] text-danger">{error}</p>}

          <p className="px-1 text-[11px] leading-relaxed text-ink-faint">
            Retirer ne supprime que la ligne : le fichier reste sur le disque et
            ton historique d'écoute est conservé.
          </p>
        </div>
      )}
    </div>
  );
}
