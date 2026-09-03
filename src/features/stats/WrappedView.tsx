import { useEffect, useState } from "react";

import { Artwork } from "@/features/library/Artwork";
import {
  formatDurationLong,
  ipc,
  toMinutes,
  type StatsPeriod,
  type Wrapped,
} from "@/lib/ipc";
import { useReveal } from "./useReveal";

/**
 * Page de statistiques.
 *
 * # Parti pris de direction artistique
 *
 * Un tableau de bord classique empile de petites cartes remplies de petits
 * textes : dense, lisible, et parfaitement ennuyeux. On prend ici le parti
 * inverse, celui des rétrospectives musicales :
 *
 * | Choix | Pourquoi |
 * |---|---|
 * | **Une idée par écran** | On fait défiler pour découvrir, pas pour consulter |
 * | **Typographie énorme** (jusqu'à 12 rem) | Le chiffre EST l'illustration — il n'y a rien d'autre à regarder |
 * | **Crénage très serré** | Sans lui, un texte agrandi paraît lâche ; avec lui, il devient un objet graphique |
 * | **Un seul accent, rare** | Le violet ne souligne que le chiffre pivot de chaque section |
 * | **Beaucoup de vide** | C'est le vide qui rend le plein spectaculaire |
 *
 * # Les fonds
 *
 * Les halos de couleur ont disparu au profit de **dégradés radiaux**. Un cercle
 * flouté par `blur` garde toujours un bord : le flou étale le contour sans le
 * supprimer, et sur un fond aussi sombre l'œil finit par voir le disque. Un
 * dégradé radial, lui, atteint réellement zéro — il n'y a plus rien à voir.
 */
const PERIODS: { label: string; value: StatsPeriod }[] = [
  { label: "30 jours", value: { days: 30 } },
  { label: "12 mois", value: { days: 365 } },
  { label: "Depuis le début", value: "allTime" },
];

/**
 * Ce que la rétrospective sait faire de ses chiffres.
 *
 * # Pourquoi elle n'en faisait rien
 *
 * Chaque rang porte une classe `group` et un `group-hover` : le nom s'éclaire
 * quand la souris passe, la ligne répond — puis rien. C'est le pire des cas,
 * pire qu'une ligne inerte : l'interface **promet** en s'allumant, et ne tient
 * pas. Toutes les données étaient là, aucun geste n'était branché.
 */
interface GestesWrapped {
  onOpenArtist: (id: number, name: string) => void;
  onOpenAlbum: (id: number, name: string, artist: string | null) => void;
  onPlayTrack: (id: number) => void;
}

export function WrappedView(gestes: GestesWrapped) {
  const [period, setPeriod] = useState<StatsPeriod>("allTime");
  const [data, setData] = useState<Wrapped | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setData(null);
    ipc
      .wrapped(period, 10)
      .then(setData)
      .catch((cause: unknown) => setError(String(cause)));
  }, [period]);

  if (error !== null) {
    return <Centered>{error}</Centered>;
  }
  if (data === null) {
    return <Centered>Calcul de tes statistiques…</Centered>;
  }

  return (
    <div className="relative">
      <PeriodPicker period={period} onChange={setPeriod} />

      {data.hasEnoughData ? (
        <>
          <Hero data={data} />
          <TopArtists data={data} gestes={gestes} />
          <TopTracks data={data} gestes={gestes} />
          <Clock data={data} />
          <PersonaSection data={data} />
          <TopAlbums data={data} gestes={gestes} />
          <KeyFigures data={data} />
          <Outro />
        </>
      ) : (
        <NotEnoughYet data={data} />
      )}
    </div>
  );
}

// ════════════════════════════════════════════════════════════════════════════
//  Sections
// ════════════════════════════════════════════════════════════════════════════

/** Le chiffre d'ouverture. Rien d'autre à l'écran, volontairement. */
function Hero({ data }: { data: Wrapped }) {
  const reveal = useReveal<HTMLElement>();
  const minutes = toMinutes(data.totals.listenedMs);

  return (
    <Section ref={reveal.ref} className={reveal.className} wash="top">
      <Eyebrow>Ton écoute · {data.periodLabel}</Eyebrow>

      <p className="display-xl numerals mt-6 text-[clamp(4.5rem,17vw,12rem)] text-ink">
        {minutes.toLocaleString("fr-FR")}
      </p>

      <p className="display mt-3 text-[clamp(1.5rem,4vw,2.75rem)] text-ink-muted">
        minutes de musique
      </p>

      <p className="mt-8 max-w-xl text-[15px] leading-relaxed text-ink-muted">
        Soit {formatDurationLong(data.totals.listenedMs)} passées avec{" "}
        <Highlight>{data.totals.distinctArtists}</Highlight> artistes et{" "}
        <Highlight>{data.totals.distinctTracks}</Highlight> morceaux différents.
      </p>
    </Section>
  );
}

/** L'artiste n°1 occupe l'écran ; les suivants se rangent dessous. */
function TopArtists({ data, gestes }: { data: Wrapped; gestes: GestesWrapped }) {
  const reveal = useReveal<HTMLElement>();
  const [first, ...rest] = data.topArtists;

  if (first === undefined) return null;

  return (
    <Section ref={reveal.ref} className={reveal.className}>
      <Eyebrow>Ton artiste n°1</Eyebrow>

      <button
        type="button"
        onClick={() => gestes.onOpenArtist(first.id, first.name)}
        className="display-xl mt-5 block max-w-full break-words text-left text-[clamp(2.75rem,10vw,7rem)] text-ink transition-colors hover:text-accent"
      >
        {first.name}
      </button>

      <p className="numerals mt-4 text-sm text-ink-muted">
        {first.plays} écoutes · {formatDurationLong(first.listenedMs)}
      </p>

      {rest.length > 0 && (
        <ol className="mt-14">
          {rest.slice(0, 4).map((artist, index) => (
            <li key={artist.id} className="border-t border-line">
              <button
                type="button"
                onClick={() => gestes.onOpenArtist(artist.id, artist.name)}
                className="pression group flex w-full items-baseline gap-5 py-4 text-left"
              >
                <span className="numerals display w-10 shrink-0 text-2xl text-ink-faint transition-colors group-hover:text-ink">
                  {index + 2}
                </span>
                <span className="display min-w-0 flex-1 truncate text-[clamp(1.25rem,3vw,2rem)] text-ink">
                  {artist.name}
                </span>
                <span className="numerals shrink-0 text-sm text-ink-faint">
                  {formatDurationLong(artist.listenedMs)}
                </span>
              </button>
            </li>
          ))}
        </ol>
      )}
    </Section>
  );
}

function TopTracks({ data, gestes }: { data: Wrapped; gestes: GestesWrapped }) {
  const reveal = useReveal<HTMLElement>();
  if (data.topTracks.length === 0) return null;

  return (
    <Section ref={reveal.ref} className={reveal.className}>
      <Eyebrow>Tes morceaux</Eyebrow>

      <ol className="mt-8">
        {data.topTracks.slice(0, 5).map((track, index) => (
          <li key={track.id} className="border-t border-line">
            <button
              type="button"
              onClick={() => gestes.onPlayTrack(track.id)}
              className="pression group flex w-full items-center gap-5 py-4 text-left"
            >
              <span className="numerals display w-10 shrink-0 text-[clamp(1.75rem,4vw,2.5rem)] text-ink-faint transition-colors group-hover:text-ink">
                {index + 1}
              </span>

              <Artwork hash={track.artworkHash} className="h-14 w-14 rounded-md" />

              <div className="min-w-0 flex-1 pl-1">
                <p className="display truncate text-[clamp(1.15rem,2.6vw,1.75rem)] text-ink">
                  {track.title}
                </p>
                <p className="truncate text-sm text-ink-muted">
                  {track.artist ?? "Artiste inconnu"}
                </p>
              </div>

              <span className="numerals shrink-0 text-right text-sm text-ink-faint">
                {track.plays} ×
              </span>
            </button>
          </li>
        ))}
      </ol>
    </Section>
  );
}

/**
 * L'horloge d'écoute.
 *
 * Vingt-quatre barres plutôt qu'une courbe : la musique s'écoute par tranches
 * horaires, pas de façon continue. Une courbe lisserait précisément ce qu'on
 * cherche à montrer.
 */
function Clock({ data }: { data: Wrapped }) {
  const reveal = useReveal<HTMLElement>();
  const peak = Math.max(...data.clock.map((slice) => slice.listenedMs), 1);
  const peakHour = data.behaviour.peakHour;

  return (
    <Section ref={reveal.ref} className={reveal.className} wash="center">
      <Eyebrow>Ton heure de pointe</Eyebrow>

      {peakHour !== null && (
        <p className="display-xl numerals mt-5 text-[clamp(3.5rem,13vw,9rem)] text-ink">
          {peakHour.toString().padStart(2, "0")}
          <span className="text-accent">h</span>
        </p>
      )}

      <div className="mt-12 flex h-40 items-end gap-[3px]">
        {data.clock.map((slice) => {
          const ratio = slice.listenedMs / peak;
          const isPeak = slice.hour === peakHour;

          return (
            <div
              key={slice.hour}
              title={`${slice.hour}h — ${formatDurationLong(slice.listenedMs)}`}
              className="group relative h-full flex-1"
            >
              <div
                className={`absolute bottom-0 w-full rounded-t-sm transition-[height] duration-700 ${
                  isPeak ? "bg-accent" : "bg-elevated group-hover:bg-ink-faint"
                }`}
                // 2 % de hauteur minimale : une heure sans écoute reste
                // visible en creux, ce qui donne sa forme à l'horloge.
                style={{ height: `${Math.max(ratio * 100, 2)}%` }}
              />
            </div>
          );
        })}
      </div>

      <div className="numerals mt-3 flex justify-between text-[11px] text-ink-faint">
        {[0, 6, 12, 18, 23].map((hour) => (
          <span key={hour}>{hour}h</span>
        ))}
      </div>

      <p className="mt-8 max-w-xl text-[15px] leading-relaxed text-ink-muted">
        <Highlight>{Math.round(data.behaviour.weekendShare * 100)} %</Highlight> de
        tes écoutes tombent le week-end.
      </p>
    </Section>
  );
}

/** Le portrait d'auditeur — le moment où les chiffres deviennent une phrase. */
function PersonaSection({ data }: { data: Wrapped }) {
  const reveal = useReveal<HTMLElement>();

  return (
    <Section ref={reveal.ref} className={reveal.className} wash="left">
      <Eyebrow>Ton portrait d'auditeur</Eyebrow>

      <p className="display-xl mt-5 text-[clamp(3rem,11vw,8rem)] text-ink">
        {data.persona.title}
      </p>

      <p className="mt-8 max-w-xl text-lg leading-relaxed text-ink-muted">
        {data.persona.description}
      </p>

      <div className="mt-14 grid gap-x-12 gap-y-8 sm:grid-cols-2">
        <Metric
          value={`${Math.round(data.behaviour.earlySkipRate * 100)} %`}
          label="de morceaux zappés en moins de 15 secondes"
        />
        <Metric
          value={`${Math.round(data.behaviour.completionRate * 100)} %`}
          label="d'écoutes menées jusqu'au bout"
        />
      </div>
    </Section>
  );
}

function TopAlbums({ data, gestes }: { data: Wrapped; gestes: GestesWrapped }) {
  const reveal = useReveal<HTMLElement>();
  if (data.topAlbums.length === 0) return null;

  return (
    <Section ref={reveal.ref} className={reveal.className}>
      <Eyebrow>Tes albums</Eyebrow>

      <div className="mt-8 grid grid-cols-2 gap-6 sm:grid-cols-3">
        {data.topAlbums.slice(0, 6).map((album, index) => (
          <button
            key={album.id}
            type="button"
            onClick={() => gestes.onOpenAlbum(album.id, album.title, album.artist)}
            className="pression group text-left"
          >
            <div className="relative overflow-hidden rounded-xl">
              <Artwork hash={album.artworkHash} className="aspect-square w-full" />
              {/* Le voile part du bas et meurt au tiers : sans lui, un chiffre
                  clair posé sur une pochette claire deviendrait illisible. */}
              <div
                aria-hidden
                className="absolute inset-x-0 bottom-0 h-1/3"
                style={{
                  backgroundImage:
                    "linear-gradient(to top, color-mix(in srgb, var(--color-base) 75%, transparent), transparent)",
                }}
              />
              <span className="numerals display absolute bottom-2 left-3 text-4xl text-ink">
                {index + 1}
              </span>
            </div>
            <p className="display mt-3 truncate text-base text-ink">{album.title}</p>
            <p className="truncate text-xs text-ink-faint">
              {album.artist ?? "Artiste inconnu"}
            </p>
          </button>
        ))}
      </div>
    </Section>
  );
}

function KeyFigures({ data }: { data: Wrapped }) {
  const reveal = useReveal<HTMLElement>();

  return (
    <Section ref={reveal.ref} className={reveal.className}>
      <Eyebrow>En chiffres</Eyebrow>

      <div className="mt-10 grid gap-x-12 gap-y-12 sm:grid-cols-2">
        <Metric
          value={data.totals.discoveries.toLocaleString("fr-FR")}
          label="morceaux découverts sur la période"
        />
        <Metric
          value={data.totals.sessions.toLocaleString("fr-FR")}
          label="sessions d'écoute"
        />
        <Metric
          value={formatDurationLong(data.totals.longestSessionMs)}
          label="ta plus longue session sans t'arrêter"
        />
        <Metric
          value={`${Math.round(data.behaviour.manualShare * 100)} %`}
          label="de morceaux que tu as choisis toi-même"
        />
      </div>
    </Section>
  );
}

function Outro() {
  const reveal = useReveal<HTMLElement>();

  return (
    <Section ref={reveal.ref} className={`${reveal.className} pb-32`}>
      <p className="display max-w-2xl text-[clamp(1.5rem,4vw,2.5rem)] leading-tight text-ink-faint">
        Tout ceci a été calculé sur ta machine.
        <br />
        <span className="text-ink">Aucune donnée n'est sortie d'ici.</span>
      </p>
    </Section>
  );
}

/** Écran d'attente tant que l'historique est trop mince. */
function NotEnoughYet({ data }: { data: Wrapped }) {
  const played = data.behaviour.totalPlays;

  return (
    <Section wash="top">
      <Eyebrow>Presque</Eyebrow>

      <p className="display-xl numerals mt-5 text-[clamp(3.5rem,12vw,8rem)] text-ink">
        {played}
        <span className="text-ink-faint">/20</span>
      </p>

      <p className="display mt-3 text-[clamp(1.25rem,3vw,2rem)] text-ink-muted">
        écoutes enregistrées
      </p>

      <p className="mt-8 max-w-xl text-[15px] leading-relaxed text-ink-muted">
        Tes statistiques s'écrivent à mesure que tu écoutes. Encore{" "}
        <Highlight>{Math.max(20 - played, 0)}</Highlight> morceaux et cette page
        aura enfin quelque chose à raconter.
      </p>
    </Section>
  );
}

// ════════════════════════════════════════════════════════════════════════════
//  Éléments partagés
// ════════════════════════════════════════════════════════════════════════════

/**
 * Positions possibles du dégradé de fond d'une section.
 *
 * # Géométrie et teinte séparées
 *
 * La forme sert deux fois : une fois pour peindre le lavis, une fois pour
 * découper la trame qui l'empêche de faire des anneaux. Les écrire ensemble
 * garantissait qu'elles finissent par diverger.
 *
 * # Cinq pour cent de blanc, c'est quatre niveaux
 *
 * Le lavis « center » traverse quatre niveaux de luminance sur sept cents
 * pixels : c'est le pire rapport de tout le projet, et un écran 8 bits ne
 * peut le rendre qu'en quatre bandes. Le grain les remplit.
 */
const LAVIS = {
  top: {
    forme: "48rem 26rem at 30% -6rem",
    teinte: "color-mix(in srgb, var(--color-accent) 9%, transparent)",
  },
  center: {
    forme: "44rem 24rem at 72% 20%",
    teinte: "color-mix(in srgb, var(--color-ink) 5%, transparent)",
  },
  left: {
    forme: "44rem 24rem at 12% 30%",
    teinte: "color-mix(in srgb, var(--color-accent) 7%, transparent)",
  },
} as const;

/**
 * Une section occupe presque tout l'écran.
 *
 * C'est ce qui force à faire défiler pour découvrir la suite, plutôt que de
 * tout embrasser d'un coup d'œil comme sur un tableau de bord.
 */
function Section({
  children,
  className = "",
  wash,
  ref,
}: {
  children: React.ReactNode;
  className?: string;
  wash?: keyof typeof LAVIS;
  ref?: React.Ref<HTMLElement>;
}) {
  return (
    <section
      ref={ref}
      className={`relative px-8 py-24 sm:px-14 sm:py-28 ${wash === undefined ? "" : "grain"} ${className}`}
      {...(wash === undefined
        ? {}
        : {
            style: {
              backgroundImage: `radial-gradient(${LAVIS[wash].forme}, ${LAVIS[wash].teinte}, transparent 70%)`,
              "--grain-masque": `radial-gradient(${LAVIS[wash].forme}, #000, transparent 70%)`,
            } as React.CSSProperties,
          })}
    >
      <div className="relative mx-auto max-w-3xl">{children}</div>
    </section>
  );
}

function Eyebrow({ children }: { children: React.ReactNode }) {
  return (
    <p className="text-xs font-semibold uppercase tracking-[0.22em] text-ink-faint">
      {children}
    </p>
  );
}

function Highlight({ children }: { children: React.ReactNode }) {
  return <span className="numerals font-semibold text-ink">{children}</span>;
}

function Metric({ value, label }: { value: string; label: string }) {
  return (
    <div>
      <p className="display numerals text-[clamp(2.25rem,6vw,3.75rem)] text-ink">
        {value}
      </p>
      <p className="mt-2 max-w-xs text-sm leading-relaxed text-ink-muted">{label}</p>
    </div>
  );
}

function PeriodPicker({
  period,
  onChange,
}: {
  period: StatsPeriod;
  onChange: (period: StatsPeriod) => void;
}) {
  const same = (a: StatsPeriod, b: StatsPeriod) =>
    JSON.stringify(a) === JSON.stringify(b);

  return (
    // Sous la barre du haut, qui est elle-même collante : d'où le décalage.
    <div className="sticky top-16 z-10 flex justify-center px-6 py-3">
      <div className="flex gap-1 rounded-full bg-elevated/90 p-1 backdrop-blur">
        {PERIODS.map((option) => (
          <button
            key={option.label}
            type="button"
            onClick={() => onChange(option.value)}
            className={`rounded-full px-4 py-1.5 text-xs font-semibold transition-colors ${
              same(option.value, period)
                ? "bg-ink text-base"
                : "text-ink-muted hover:text-ink"
            }`}
          >
            {option.label}
          </button>
        ))}
      </div>
    </div>
  );
}

function Centered({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-96 items-center justify-center p-10 text-center text-sm text-ink-muted">
      {children}
    </div>
  );
}
