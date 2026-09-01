import { Icon } from "@/components/Icon";
import { formatDuration, type QueueItem } from "@/lib/ipc";
import { useLyrics } from "./useLyrics";

interface LyricsViewProps {
  track: QueueItem | null;
  positionMs: number;
  onSeek: (positionMs: number) => void;
}

/**
 * Les paroles, en grand.
 *
 * # Pourquoi une page et non seulement le panneau
 *
 * Le panneau latéral fait 22 rem : de quoi suivre une ligne du coin de l'œil,
 * pas de quoi **lire**. Quand les paroles deviennent ce qu'on regarde, elles
 * doivent occuper l'écran — le texte devient l'illustration, exactement comme
 * le chiffre l'est dans les statistiques.
 *
 * # Le traitement typographique
 *
 * La ligne chantée est pleinement lisible, les autres sont estompées. Ce n'est
 * pas un effet : c'est ce qui permet de retrouver l'endroit d'un seul coup
 * d'œil après avoir regardé ailleurs. Le contraste fait le travail que ferait
 * sinon un curseur.
 */
export function LyricsView({ track, positionMs, onSeek }: LyricsViewProps) {
  const state = useLyrics(track?.trackId ?? null, positionMs);

  if (track === null) {
    return (
      <Centered>
        <p className="text-sm text-ink-muted">Lance un morceau pour voir ses paroles.</p>
      </Centered>
    );
  }

  return (
    <div className="px-6 pb-24 pt-8 sm:px-10">
      <header className="mx-auto max-w-3xl">
        <p className="text-xs font-semibold uppercase tracking-[0.16em] text-ink-faint">
          Paroles
        </p>
        <h1 className="display-xl mt-2 text-[clamp(1.75rem,4.5vw,3rem)] text-ink">
          {track.title}
        </h1>
        <p className="mt-2 text-[15px] text-ink-muted">
          {track.artist ?? "Artiste inconnu"}
        </p>
      </header>

      <div className="mx-auto mt-10 max-w-3xl">
        {state.lyrics === null ? (
          <p className="text-sm text-ink-faint">Lecture des paroles…</p>
        ) : state.editing ? (
          <Editor state={state} />
        ) : state.isEmpty ? (
          <Empty state={state} />
        ) : state.lyrics.synced.length > 0 ? (
          <ol>
            {state.lyrics.synced.map((line, index) => {
              // Une ligne horodatée sans texte marque un silence instrumental :
              // elle garde sa place dans le défilement sans afficher de vide.
              if (line.text === "") {
                return <li key={`${index}-gap`} className="h-6" />;
              }

              const isCurrent = index === state.current;

              return (
                <li key={`${index}-${line.atMs}`}>
                  <p
                    ref={isCurrent ? state.activeLine : null}
                    onClick={() => onSeek(line.atMs)}
                    title={`Aller à ${formatDuration(line.atMs)}`}
                    className={`display cursor-pointer py-1.5 text-[clamp(1.5rem,3.2vw,2.5rem)] leading-tight transition-colors duration-300 ${
                      isCurrent
                        ? "text-ink"
                        : "text-ink-faint/60 hover:text-ink-muted"
                    }`}
                  >
                    {line.text}
                  </p>
                </li>
              );
            })}
          </ol>
        ) : (
          <div>
            {state.lyrics.plain.map((line, index) => (
              <p
                key={`${index}-${line}`}
                className="display py-1.5 text-[clamp(1.5rem,3.2vw,2.5rem)] leading-tight text-ink-muted"
              >
                {line}
              </p>
            ))}
          </div>
        )}

        {!state.editing && !state.isEmpty && state.lyrics !== null && (
          <button
            type="button"
            onClick={() => state.setEditing(true)}
            className="mt-10 flex items-center gap-2 text-[13px] text-ink-faint transition-colors hover:text-ink"
          >
            <Icon name="pencil" size={14} />
            Modifier les paroles
          </button>
        )}
      </div>
    </div>
  );
}

function Empty({ state }: { state: ReturnType<typeof useLyrics> }) {
  return (
    <div className="py-8">
      <p className="display text-[clamp(1.25rem,2.6vw,1.75rem)] text-ink-muted">
        Ce fichier ne contient pas de paroles.
      </p>

      <div className="mt-6 flex flex-wrap items-center gap-3">
        <button
          type="button"
          disabled={state.searching}
          onClick={() => void state.search()}
          className="flex items-center gap-2 rounded-full bg-ink px-5 py-2.5 text-[13px] font-semibold text-base transition-opacity hover:opacity-90 disabled:opacity-40"
        >
          <span className={state.searching ? "animate-spin" : ""}>
            <Icon name={state.searching ? "repeat" : "search"} size={15} />
          </span>
          {state.searching ? "Recherche…" : "Chercher en ligne"}
        </button>

        <button
          type="button"
          onClick={() => state.setEditing(true)}
          className="rounded-full bg-elevated px-5 py-2.5 text-[13px] font-semibold text-ink-muted transition-colors hover:text-ink"
        >
          Les coller à la main
        </button>
      </div>

      {state.error !== null && (
        <p className="mt-4 text-[13px] text-warn">{state.error}</p>
      )}

      <p className="mt-6 max-w-xl text-[12px] leading-relaxed text-ink-faint">
        La recherche envoie l'artiste, le titre et la durée à LRCLIB. Rien
        d'autre ne quitte ta machine, et rien n'est envoyé sans ce clic.
      </p>
    </div>
  );
}

function Editor({ state }: { state: ReturnType<typeof useLyrics> }) {
  return (
    <div>
      <textarea
        autoFocus
        value={state.draft}
        onChange={(event) => state.setDraft(event.target.value)}
        placeholder={"Colle les paroles ici.\n\nFormat LRC accepté :\n[00:12.34]Première ligne"}
        className="h-[60vh] w-full resize-none rounded-xl border border-line bg-base p-5 text-[15px] leading-relaxed text-ink placeholder:text-ink-faint focus:border-accent focus:outline-none"
      />

      {state.error !== null && <p className="mt-3 text-[13px] text-danger">{state.error}</p>}

      <div className="mt-4 flex gap-3">
        <button
          type="button"
          onClick={() => void state.save()}
          className="rounded-full bg-ink px-5 py-2.5 text-[13px] font-semibold text-base transition-opacity hover:opacity-90"
        >
          Enregistrer
        </button>
        <button
          type="button"
          onClick={() => state.setEditing(false)}
          className="rounded-full px-5 py-2.5 text-[13px] text-ink-muted transition-colors hover:text-ink"
        >
          Annuler
        </button>
      </div>

      <p className="mt-4 text-[12px] leading-relaxed text-ink-faint">
        Les paroles sont écrites dans le fichier lui-même : elles suivront le
        morceau si tu l'ouvres ailleurs.
      </p>
    </div>
  );
}

function Centered({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-96 items-center justify-center px-6 text-center">
      {children}
    </div>
  );
}
