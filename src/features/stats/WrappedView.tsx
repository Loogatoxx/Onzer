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
 * | **Une couleur par section** | Donne un rythme au défilement sans transformer la page en arc-en-ciel |
 * | **Beaucoup de vide** | C'est le vide qui rend le plein spectaculaire |
 *
 * Les néons ne servent **jamais** de fond : uniquement des chiffres, des
 * dégradés de texte et des halos. Un aplat fluo sur toute une section fatiguerait
 * l'œil au bout de deux sections.
 */
const PERIODS: { label: string; value: StatsPeriod }[] = [
  { label: "30 jours", value: { days: 30 } },
  { label: "12 mois", value: { days: 365 } },
  { label: "Depuis le début", value: "allTime" },
];

export function WrappedView() {
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
          <TopArtists data={data} />
          <TopTracks data={data} />
          <Clock data={data} />
          <PersonaSection data={data} />
          <TopAlbums data={data} />
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
    <Section ref={reveal.ref} className={reveal.className}>
      <Glow className="-top-32 left-1/4 bg-accent/20" />

      <Eyebrow>Ton écoute · {data.periodLabel}</Eyebrow>

      <p className="display-tight numerals mt-6 bg-gradient-to-br from-accent via-accent-soft to-accent-alt text-gradient text-[clamp(4.5rem,17vw,12rem)]">
        {minutes.toLocaleString("fr-FR")}
      </p>

      <p className="display mt-2 text-[clamp(1.5rem,4vw,2.75rem)] text-ink">
        minutes de musique
      </p>

      <p className="mt-8 max-w-xl text-base leading-relaxed text-ink-muted">
        Soit {formatDurationLong(data.totals.listenedMs)} passées avec{" "}
        <Highlight>{data.totals.distinctArtists}</Highlight> artistes et{" "}
        <Highlight>{data.totals.distinctTracks}</Highlight> morceaux différents.
      </p>
    </Section>
  );
}

/** L'artiste n°1 occupe l'écran ; les suivants se rangent dessous. */
function TopArtists({ data }: { data: Wrapped }) {
  const reveal = useReveal<HTMLElement>();
  const [first, ...rest] = data.topArtists;

  if (first === undefined) return null;

  return (
    <Section ref={reveal.ref} className={reveal.className}>
      <Glow className="top-10 right-0 bg-neon-lime/12" />

      <Eyebrow accent="text-neon-lime">Ton artiste n°1</Eyebrow>

      <p className="display-tight mt-5 text-[clamp(2.75rem,10vw,7rem)] text-ink">
        {first.name}
      </p>

      <p className="numerals mt-4 text-sm text-ink-muted">
        {first.plays} écoutes · {formatDurationLong(first.listenedMs)}
      </p>

      {rest.length > 0 && (
        <ol className="mt-14 space-y-1">
          {rest.slice(0, 4).map((artist, index) => (
            <li
              key={artist.id}
              className="group flex items-baseline gap-5 border-t border-line py-4"
            >
              <span className="numerals display w-10 shrink-0 text-2xl text-ink-faint transition-colors group-hover:text-neon-lime">
                {index + 2}
              </span>
              <span className="display min-w-0 flex-1 truncate text-[clamp(1.25rem,3vw,2rem)] text-ink">
                {artist.name}
              </span>
              <span className="numerals shrink-0 text-sm text-ink-faint">
                {formatDurationLong(artist.listenedMs)}
              </span>
            </li>
          ))}
        </ol>
      )}
    </Section>
  );
}

function TopTracks({ data }: { data: Wrapped }) {
  const reveal = useReveal<HTMLElement>();
  if (data.topTracks.length === 0) return null;

  return (
    <Section ref={reveal.ref} className={reveal.className}>
      <Glow className="top-0 left-0 bg-neon-pink/12" />

      <Eyebrow accent="text-neon-pink">Tes morceaux</Eyebrow>

      <ol className="mt-8 space-y-1">
        {data.topTracks.slice(0, 5).map((track, index) => (
          <li
            key={track.id}
            className="group flex items-center gap-5 border-t border-line py-4"
          >
            <span className="numerals display w-10 shrink-0 text-[clamp(1.75rem,4vw,2.5rem)] text-ink-faint transition-colors group-hover:text-neon-pink">
              {index + 1}
            </span>

            <div className="shrink-0 scale-125">
              <Artwork hash={track.artworkHash} />
            </div>

            <div className="min-w-0 flex-1 pl-2">
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
    <Section ref={reveal.ref} className={reveal.className}>
      <Glow className="top-16 right-1/4 bg-accent-alt/15" />

      <Eyebrow accent="text-accent-alt">Ton heure de pointe</Eyebrow>

      {peakHour !== null && (
        <p className="display-tight numerals mt-5 text-[clamp(3.5rem,13vw,9rem)] text-ink">
          {peakHour.toString().padStart(2, "0")}
          <span className="text-accent-alt">h</span>
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
              className="group relative flex-1"
              style={{ height: "100%" }}
            >
              <div
                className={`absolute bottom-0 w-full rounded-t-sm transition-all duration-700 ${
                  isPeak
                    ? "bg-gradient-to-t from-accent to-accent-alt"
                    : "bg-elevated group-hover:bg-ink-faint"
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

      <p className="mt-8 max-w-xl text-base leading-relaxed text-ink-muted">
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
    <Section ref={reveal.ref} className={reveal.className}>
      <Glow className="-top-20 left-1/3 bg-neon-amber/12" />

      <Eyebrow accent="text-neon-amber">Ton portrait d'auditeur</Eyebrow>

      <p className="display-tight mt-5 bg-gradient-to-br from-neon-amber to-neon-pink text-gradient text-[clamp(3rem,11vw,8rem)]">
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

function TopAlbums({ data }: { data: Wrapped }) {
  const reveal = useReveal<HTMLElement>();
  if (data.topAlbums.length === 0) return null;

  return (
    <Section ref={reveal.ref} className={reveal.className}>
      <Glow className="top-0 right-10 bg-accent/15" />

      <Eyebrow>Tes albums</Eyebrow>

      <div className="mt-8 grid grid-cols-2 gap-6 sm:grid-cols-3">
        {data.topAlbums.slice(0, 6).map((album, index) => (
          <div key={album.id} className="group">
            <div className="relative aspect-square overflow-hidden rounded-xl bg-elevated">
              <CoverFill hash={album.artworkHash} />
              <span className="numerals display absolute bottom-2 left-3 text-4xl text-ink/90 mix-blend-difference">
                {index + 1}
              </span>
            </div>
            <p className="display mt-3 truncate text-base text-ink">{album.title}</p>
            <p className="truncate text-xs text-ink-faint">
              {album.artist ?? "Artiste inconnu"}
            </p>
          </div>
        ))}
      </div>
    </Section>
  );
}

function KeyFigures({ data }: { data: Wrapped }) {
  const reveal = useReveal<HTMLElement>();

  return (
    <Section ref={reveal.ref} className={reveal.className}>
      <Eyebrow accent="text-neon-lime">En chiffres</Eyebrow>

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
    <Section ref={reveal.ref} className={`${reveal.className} pb-40`}>
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
    <Section>
      <Glow className="-top-24 left-1/4 bg-accent/15" />

      <Eyebrow>Presque</Eyebrow>

      <p className="display-tight numerals mt-5 text-[clamp(3.5rem,12vw,8rem)] text-ink">
        {played}
        <span className="text-ink-faint">/20</span>
      </p>

      <p className="display mt-3 text-[clamp(1.25rem,3vw,2rem)] text-ink">
        écoutes enregistrées
      </p>

      <p className="mt-8 max-w-xl text-base leading-relaxed text-ink-muted">
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
 * Une section occupe presque tout l'écran.
 *
 * C'est ce qui force à faire défiler pour découvrir la suite, plutôt que de
 * tout embrasser d'un coup d'œil comme sur un tableau de bord.
 */
function Section({
  children,
  className = "",
  ref,
}: {
  children: React.ReactNode;
  className?: string;
  ref?: React.Ref<HTMLElement>;
}) {
  return (
    <section
      ref={ref}
      className={`relative overflow-hidden px-8 py-24 sm:px-14 sm:py-28 ${className}`}
    >
      <div className="relative mx-auto max-w-3xl">{children}</div>
    </section>
  );
}

/** Halo de couleur, très diffus : la couleur sans l'aplat. */
function Glow({ className }: { className: string }) {
  return (
    <div
      aria-hidden
      className={`pointer-events-none absolute h-[26rem] w-[26rem] rounded-full blur-[140px] ${className}`}
    />
  );
}

function Eyebrow({
  children,
  accent = "text-accent",
}: {
  children: React.ReactNode;
  accent?: string;
}) {
  return (
    <p className={`text-xs font-semibold uppercase tracking-[0.22em] ${accent}`}>
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

/** Pochette en pleine largeur de sa carte. */
function CoverFill({ hash }: { hash: string | null }) {
  return (
    <div className="absolute inset-0 [&_img]:h-full [&_img]:w-full [&_img]:rounded-none [&>div]:h-full [&>div]:w-full [&>div]:rounded-none">
      <Artwork hash={hash} />
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
    <div className="sticky top-0 z-10 flex justify-center border-b border-line bg-base/80 px-6 py-3 backdrop-blur">
      <div className="flex gap-1 rounded-full border border-line bg-surface p-1">
        {PERIODS.map((option) => (
          <button
            key={option.label}
            type="button"
            onClick={() => onChange(option.value)}
            className={`rounded-full px-4 py-1.5 text-xs font-medium transition-colors ${
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
    <div className="flex h-full items-center justify-center p-10 text-center text-sm text-ink-muted">
      {children}
    </div>
  );
}
