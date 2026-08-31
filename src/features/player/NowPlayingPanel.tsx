import { useEffect, useRef, useState } from "react";

import { Artwork } from "@/features/library/Artwork";
import { Icon, IconButton } from "@/components/Icon";
import { formatDuration, ipc, type Lyrics, type QueueItem } from "@/lib/ipc";

export type PanelTab = "lyrics" | "queue";

interface NowPlayingPanelProps {
  track: QueueItem;
  positionMs: number;
  isLoved: boolean;
  tab: PanelTab;
  queue: QueueItem[];
  queueIndex: number | null;
  onTab: (tab: PanelTab) => void;
  onClose: () => void;
  onToggleLoved: () => void;
  onSeek: (positionMs: number) => void;
  onJump: (index: number) => void;
  onRadio: (trackId: number) => void;
}

/**
 * Panneau de droite : la pochette en grand, les paroles, la file.
 *
 * # Pourquoi un panneau et non une page
 *
 * Les paroles se lisent **pendant** qu'on parcourt sa bibliothèque. En faire une
 * page plein écran obligerait à quitter ce qu'on regardait pour lire une ligne,
 * puis à y revenir. Le panneau coexiste avec le reste, et c'est tout l'intérêt.
 */
export function NowPlayingPanel(props: NowPlayingPanelProps) {
  const { track } = props;

  return (
    <aside className="hidden w-[22rem] shrink-0 flex-col overflow-hidden rounded-xl bg-surface lg:flex">
      <header className="flex shrink-0 items-center justify-between px-4 pt-4">
        <span className="text-xs font-semibold uppercase tracking-[0.14em] text-ink-faint">
          En cours de lecture
        </span>
        <IconButton name="close" label="Fermer le panneau" size={16} onClick={props.onClose} />
      </header>

      <div className="shrink-0 px-4 pt-3">
        <Artwork
          hash={track.artworkHash}
          className="aspect-square w-full rounded-lg"
        />

        <div className="mt-4 flex items-start gap-3">
          <div className="min-w-0 flex-1">
            <p className="display truncate text-xl text-ink">{track.title}</p>
            <p className="mt-1 truncate text-[13px] text-ink-muted">
              {track.artist ?? "Artiste inconnu"}
              {track.album !== null && (
                <>
                  <span className="mx-1.5 text-ink-faint">·</span>
                  {track.album}
                </>
              )}
            </p>
          </div>

          <button
            type="button"
            title={props.isLoved ? "Retirer des favoris" : "Ajouter aux favoris"}
            aria-label={props.isLoved ? "Retirer des favoris" : "Ajouter aux favoris"}
            onClick={props.onToggleLoved}
            className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-full transition-colors ${
              props.isLoved ? "text-accent" : "text-ink-faint hover:text-ink"
            }`}
          >
            <Icon name={props.isLoved ? "heartFilled" : "heart"} size={18} />
          </button>

          <IconButton
            name="radio"
            label="Radio à partir de ce morceau"
            size={17}
            onClick={() => props.onRadio(track.trackId)}
          />
        </div>
      </div>

      <div className="mt-4 flex shrink-0 gap-1 px-4">
        <Tab active={props.tab === "lyrics"} onClick={() => props.onTab("lyrics")}>
          Paroles
        </Tab>
        <Tab active={props.tab === "queue"} onClick={() => props.onTab("queue")}>
          À suivre
        </Tab>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-4 pb-4 pt-2">
        {props.tab === "lyrics" ? (
          <LyricsPane trackId={track.trackId} positionMs={props.positionMs} onSeek={props.onSeek} />
        ) : (
          <QueuePane
            queue={props.queue}
            queueIndex={props.queueIndex}
            onJump={props.onJump}
          />
        )}
      </div>
    </aside>
  );
}

function Tab({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`rounded-full px-3.5 py-1.5 text-[13px] font-semibold transition-colors ${
        active ? "bg-elevated text-ink" : "text-ink-faint hover:text-ink"
      }`}
    >
      {children}
    </button>
  );
}

// ════════════════════════════════════════════════════════════════════════════
//  Paroles
// ════════════════════════════════════════════════════════════════════════════

/**
 * Index de la ligne chantée à cet instant.
 *
 * C'est le pendant exact de `Lyrics::line_at` côté Rust. La logique est
 * dupliquée en connaissance de cause : l'alternative serait un aller-retour IPC
 * **quatre fois par seconde**, pour une recherche dichotomique de six lignes.
 */
function lineAt(lyrics: Lyrics, positionMs: number): number | null {
  const lines = lyrics.synced;
  if (lines.length === 0) return null;

  const first = lines[0];
  if (first === undefined || positionMs < first.atMs) return null;

  let low = 0;
  let high = lines.length;
  while (low < high) {
    const middle = (low + high) >> 1;
    const line = lines[middle];
    if (line !== undefined && line.atMs <= positionMs) low = middle + 1;
    else high = middle;
  }

  return low - 1;
}

/**
 * Paroles du morceau.
 *
 * Synchronisées, elles défilent seules et chaque ligne est cliquable — c'est le
 * moyen le plus précis qui soit de retourner au deuxième couplet. Simples,
 * elles s'affichent en bloc. Absentes, on propose de les coller : les paroles
 * sont alors écrites **dans le fichier**, et suivront le morceau ailleurs.
 */
function LyricsPane({
  trackId,
  positionMs,
  onSeek,
}: {
  trackId: number;
  positionMs: number;
  onSeek: (positionMs: number) => void;
}) {
  const [lyrics, setLyrics] = useState<Lyrics | null>(null);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);

  const container = useRef<HTMLDivElement>(null);
  const activeLine = useRef<HTMLParagraphElement>(null);

  useEffect(() => {
    setLyrics(null);
    setEditing(false);
    setDraft("");
    setError(null);

    let active = true;
    void ipc
      .trackLyrics(trackId)
      .then((loaded) => {
        if (active) setLyrics(loaded);
      })
      .catch(() => {
        if (active) setLyrics({ synced: [], plain: [] });
      });

    return () => {
      active = false;
    };
  }, [trackId]);

  const current = lyrics === null ? null : lineAt(lyrics, positionMs);

  // La ligne courante se recentre d'elle-même. Sans cela, il faudrait faire
  // défiler à la main pendant qu'on écoute — l'inverse de ce qu'on attend.
  useEffect(() => {
    activeLine.current?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [current]);

  async function save() {
    try {
      setLyrics(await ipc.setTrackLyrics(trackId, draft));
      setEditing(false);
    } catch (cause) {
      setError(String(cause));
    }
  }

  if (lyrics === null) {
    return <p className="py-8 text-center text-sm text-ink-faint">Lecture des paroles…</p>;
  }

  if (editing) {
    return (
      <div>
        <textarea
          autoFocus
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          placeholder={"Colle les paroles ici.\n\nFormat LRC accepté :\n[00:12.34]Première ligne"}
          className="h-64 w-full resize-none rounded-lg border border-line bg-base p-3 text-[13px] leading-relaxed text-ink placeholder:text-ink-faint focus:border-accent focus:outline-none"
        />

        {error !== null && <p className="mt-2 text-xs text-danger">{error}</p>}

        <div className="mt-3 flex gap-2">
          <button
            type="button"
            onClick={() => void save()}
            className="rounded-full bg-ink px-4 py-1.5 text-[13px] font-semibold text-base transition-opacity hover:opacity-90"
          >
            Enregistrer
          </button>
          <button
            type="button"
            onClick={() => setEditing(false)}
            className="rounded-full px-4 py-1.5 text-[13px] text-ink-muted transition-colors hover:text-ink"
          >
            Annuler
          </button>
        </div>

        <p className="mt-3 text-[11px] leading-relaxed text-ink-faint">
          Les paroles sont écrites dans le fichier lui-même : elles suivront le
          morceau si tu l'ouvres ailleurs.
        </p>
      </div>
    );
  }

  if (lyrics.synced.length === 0 && lyrics.plain.length === 0) {
    return (
      <div className="py-10 text-center">
        <p className="text-sm text-ink-muted">Pas de paroles pour ce morceau.</p>
        <button
          type="button"
          onClick={() => setEditing(true)}
          className="mt-4 rounded-full bg-elevated px-4 py-2 text-[13px] font-semibold text-ink transition-colors hover:bg-raised"
        >
          Ajouter des paroles
        </button>
      </div>
    );
  }

  if (lyrics.synced.length === 0) {
    return (
      <div>
        {lyrics.plain.map((line, index) => (
          <p key={`${index}-${line}`} className="py-1 text-[15px] leading-relaxed text-ink-muted">
            {line}
          </p>
        ))}
        <EditLink onClick={() => setEditing(true)} />
      </div>
    );
  }

  return (
    <div ref={container}>
      {lyrics.synced.map((line, index) => {
        const isCurrent = index === current;
        // Une ligne horodatée sans texte marque un silence instrumental : on
        // garde sa place dans le défilement sans afficher de vide béant.
        if (line.text === "") {
          return <div key={`${index}-gap`} className="h-4" />;
        }

        return (
          <p
            key={`${index}-${line.atMs}`}
            ref={isCurrent ? activeLine : null}
            onClick={() => onSeek(line.atMs)}
            title={`Aller à ${formatDuration(line.atMs)}`}
            className={`cursor-pointer py-[3px] text-[17px] font-semibold leading-snug transition-colors duration-300 ${
              isCurrent ? "text-ink" : "text-ink-faint hover:text-ink-muted"
            }`}
          >
            {line.text}
          </p>
        );
      })}
      <EditLink onClick={() => setEditing(true)} />
    </div>
  );
}

function EditLink({ onClick }: { onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="mt-6 flex items-center gap-1.5 text-[12px] text-ink-faint transition-colors hover:text-ink"
    >
      <Icon name="pencil" size={13} />
      Modifier les paroles
    </button>
  );
}

// ════════════════════════════════════════════════════════════════════════════
//  File d'attente
// ════════════════════════════════════════════════════════════════════════════

/**
 * La suite de la file.
 *
 * Ce qui est déjà passé n'est pas affiché : la question qu'on se pose en
 * ouvrant cet onglet est « qu'est-ce qui arrive ensuite ? ».
 */
function QueuePane({
  queue,
  queueIndex,
  onJump,
}: {
  queue: QueueItem[];
  queueIndex: number | null;
  onJump: (index: number) => void;
}) {
  const start = (queueIndex ?? -1) + 1;
  const upcoming = queue.slice(start);

  if (upcoming.length === 0) {
    return (
      <p className="py-10 text-center text-sm text-ink-faint">
        Rien après ce morceau.
      </p>
    );
  }

  return (
    <ul>
      {upcoming.map((item, offset) => (
        <li key={`${item.trackId}-${start + offset}`}>
          <button
            type="button"
            onClick={() => onJump(start + offset)}
            className="flex w-full items-center gap-3 rounded-md p-2 text-left transition-colors hover:bg-elevated"
          >
            <Artwork hash={item.artworkHash} className="h-10 w-10 rounded" />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-[13px] font-medium text-ink">
                {item.title}
              </span>
              <span className="block truncate text-xs text-ink-faint">
                {item.artist ?? "Artiste inconnu"}
              </span>
            </span>
            <span className="numerals shrink-0 text-xs text-ink-faint">
              {formatDuration(item.durationMs)}
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}
