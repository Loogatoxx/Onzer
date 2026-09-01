import { Artwork } from "@/features/library/Artwork";
import { Icon, IconButton } from "@/components/Icon";
import { formatDuration, type QueueItem } from "@/lib/ipc";
import { TrackNote } from "./TrackNote";
import { useLyrics } from "./useLyrics";

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
  /** Ouvre les paroles en pleine largeur. */
  onExpandLyrics: () => void;
  /**
   * Vrai quand les paroles occupent déjà la page.
   *
   * L'onglet disparaît alors : proposer le même texte à deux tailles côte à
   * côte n'apporte rien, et laisse croire à un défaut d'affichage.
   */
  lyricsExpanded: boolean;
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

        <TrackNote trackId={track.trackId} />
      </div>

      <div className="mt-4 flex shrink-0 gap-1 px-4">
        {!props.lyricsExpanded && (
          <Tab active={props.tab === "lyrics"} onClick={() => props.onTab("lyrics")}>
            Paroles
          </Tab>
        )}
        <Tab active={props.tab === "queue"} onClick={() => props.onTab("queue")}>
          À suivre
        </Tab>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-4 pb-4 pt-2">
        {props.tab === "lyrics" && !props.lyricsExpanded ? (
          <LyricsPane
            trackId={track.trackId}
            positionMs={props.positionMs}
            onSeek={props.onSeek}
            onExpand={props.onExpandLyrics}
          />
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
 * Paroles du morceau, en colonne étroite.
 *
 * Synchronisées, elles défilent seules et chaque ligne est cliquable — c'est le
 * moyen le plus précis qui soit de retourner au deuxième couplet.
 *
 * Toute la logique vit dans `useLyrics`, partagée avec la vue en grand : ce qui
 * change ici, c'est la mise en page, jamais le comportement.
 */
function LyricsPane({
  trackId,
  positionMs,
  onSeek,
  onExpand,
}: {
  trackId: number;
  positionMs: number;
  onSeek: (positionMs: number) => void;
  onExpand: () => void;
}) {
  const state = useLyrics(trackId, positionMs);

  if (state.lyrics === null) {
    return <p className="py-8 text-center text-sm text-ink-faint">Lecture des paroles…</p>;
  }

  if (state.editing) {
    return (
      <div>
        <textarea
          autoFocus
          value={state.draft}
          onChange={(event) => state.setDraft(event.target.value)}
          placeholder={"Colle les paroles ici.\n\nFormat LRC accepté :\n[00:12.34]Première ligne"}
          className="h-64 w-full resize-none rounded-lg border border-line bg-base p-3 text-[13px] leading-relaxed text-ink placeholder:text-ink-faint focus:border-accent focus:outline-none"
        />

        {state.error !== null && <p className="mt-2 text-xs text-danger">{state.error}</p>}

        <div className="mt-3 flex gap-2">
          <button
            type="button"
            onClick={() => void state.save()}
            className="rounded-full bg-ink px-4 py-1.5 text-[13px] font-semibold text-base transition-opacity hover:opacity-90"
          >
            Enregistrer
          </button>
          <button
            type="button"
            onClick={() => state.setEditing(false)}
            className="rounded-full px-4 py-1.5 text-[13px] text-ink-muted transition-colors hover:text-ink"
          >
            Annuler
          </button>
        </div>
      </div>
    );
  }

  if (state.isEmpty) {
    return (
      <div className="py-10 text-center">
        <p className="text-sm text-ink-muted">Ce fichier ne contient pas de paroles.</p>

        <button
          type="button"
          disabled={state.searching}
          onClick={() => void state.search()}
          className="mt-4 inline-flex items-center gap-2 rounded-full bg-ink px-4 py-2 text-[13px] font-semibold text-base transition-opacity hover:opacity-90 disabled:opacity-40"
        >
          <span className={state.searching ? "animate-spin" : ""}>
            <Icon name={state.searching ? "repeat" : "search"} size={15} />
          </span>
          {state.searching ? "Recherche…" : "Chercher en ligne"}
        </button>

        <button
          type="button"
          onClick={() => state.setEditing(true)}
          className="mt-2 block w-full text-[13px] text-ink-faint transition-colors hover:text-ink"
        >
          Les coller à la main
        </button>

        {state.error !== null && <p className="mt-4 text-xs text-warn">{state.error}</p>}

        <p className="mt-6 text-[11px] leading-relaxed text-ink-faint">
          La recherche envoie l'artiste, le titre et la durée à LRCLIB. Rien
          d'autre ne quitte ta machine, et rien n'est envoyé sans ce clic.
        </p>
      </div>
    );
  }

  return (
    <div>
      <button
        type="button"
        onClick={onExpand}
        className="mb-3 flex w-full items-center justify-center gap-2 rounded-full bg-elevated py-1.5 text-[12px] font-semibold text-ink-muted transition-colors hover:text-ink"
      >
        <Icon name="lyrics" size={14} />
        Afficher en grand
      </button>

      {state.lyrics.synced.length > 0
        ? state.lyrics.synced.map((line, index) => {
            if (line.text === "") {
              return <div key={`${index}-gap`} className="h-4" />;
            }

            const isCurrent = index === state.current;

            return (
              <p
                key={`${index}-${line.atMs}`}
                ref={isCurrent ? state.activeLine : null}
                onClick={() => onSeek(line.atMs)}
                title={`Aller à ${formatDuration(line.atMs)}`}
                className={`cursor-pointer py-[3px] text-[17px] font-semibold leading-snug transition-colors duration-300 ${
                  isCurrent ? "text-ink" : "text-ink-faint hover:text-ink-muted"
                }`}
              >
                {line.text}
              </p>
            );
          })
        : state.lyrics.plain.map((line, index) => (
            <p
              key={`${index}-${line}`}
              className="py-1 text-[15px] leading-relaxed text-ink-muted"
            >
              {line}
            </p>
          ))}

      <button
        type="button"
        onClick={() => state.setEditing(true)}
        className="mt-6 flex items-center gap-1.5 text-[12px] text-ink-faint transition-colors hover:text-ink"
      >
        <Icon name="pencil" size={13} />
        Modifier les paroles
      </button>
    </div>
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
            <span className="flex min-w-0 flex-1 flex-col">
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
