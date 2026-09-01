import { useCallback, useEffect, useState } from "react";

import { Icon } from "@/components/Icon";
import { Artwork } from "@/features/library/Artwork";
import {
  ipc,
  type GeneratedPlaylist,
  type Home,
  type HomeMix,
  type TrackSummary,
} from "@/lib/ipc";

interface HomeViewProps {
  /** Lance la lecture d'un morceau de la rangée « Reprendre ». */
  onPlayTrack: (tracks: TrackSummary[], index: number) => void;
  onGenerated: (playlist: GeneratedPlaylist) => void;
  onError: (message: string) => void;
}

/**
 * Page d'accueil.
 *
 * # Ce qu'elle résout
 *
 * Une bibliothèque personnelle a un défaut que les catalogues en ligne n'ont
 * pas : **on l'a déjà toute entendue**. Une liste de 253 morceaux connus ne
 * donne envie de rien. Cette page ne montre donc pas la bibliothèque, elle
 * propose des **entrées** dedans.
 *
 * # Pourquoi des mosaïques et non des pochettes uniques
 *
 * Un mix n'est pas un album : lui donner une pochette unique le ferait passer
 * pour un disque, et l'utilisateur s'attendrait à retrouver toujours les mêmes
 * morceaux. Quatre pochettes assemblées disent tout de suite qu'il s'agit d'un
 * assemblage, et donnent un aperçu de ce qu'il contient.
 */
export function HomeView({ onPlayTrack, onGenerated, onError }: HomeViewProps) {
  const [home, setHome] = useState<Home | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const reload = useCallback(() => {
    void ipc.home().then(setHome).catch(() => undefined);
  }, []);

  useEffect(reload, [reload]);

  async function launch(mix: HomeMix) {
    const key = mixKey(mix);
    setBusy(key);

    try {
      onGenerated(await start(mix));
    } catch (cause) {
      onError(String(cause));
    } finally {
      setBusy(null);
    }
  }

  if (home === null) {
    return <div className="px-6 py-16 text-sm text-ink-faint">Préparation…</div>;
  }

  return (
    <div className="px-6 pb-10 pt-6">
      <h1 className="display-xl text-[clamp(1.75rem,4vw,2.75rem)] text-ink">
        {home.greeting}
      </h1>

      {home.resume.length > 0 && (
        <ResumeGrid tracks={home.resume} onPlay={onPlayTrack} />
      )}

      {home.shelves.map((shelf) => (
        shelf.mixes.length > 0 && (
          <section key={shelf.title} className="mt-10">
            <h2 className="display text-[clamp(1.15rem,2.4vw,1.5rem)] text-ink">
              {shelf.title}
            </h2>

            <div className="mt-4 grid grid-cols-2 gap-4 md:grid-cols-3 xl:grid-cols-4">
              {shelf.mixes.map((mix) => (
                <MixCard
                  key={mixKey(mix)}
                  mix={mix}
                  loading={busy === mixKey(mix)}
                  onClick={() => void launch(mix)}
                />
              ))}
            </div>
          </section>
        )
      ))}
    </div>
  );
}

/** Deux mix de goût partagent leur `kind` ; l'artiste les distingue. */
function mixKey(mix: HomeMix): string {
  return mix.artistId === null ? mix.kind : `${mix.kind}:${mix.artistId}`;
}

function start(mix: HomeMix): Promise<GeneratedPlaylist> {
  switch (mix.kind) {
    case "loved":
      return ipc.startLoved();
    case "forgotten":
      return ipc.startForgotten();
    case "never_played":
      return ipc.startNeverPlayed();
    case "artist_mix":
      return ipc.startArtistMix(mix.artistId ?? 0);
    default:
      return ipc.startForNow();
  }
}

/**
 * Rangée de reprise.
 *
 * Format large et court — deux lignes de quatre — plutôt qu'une liste : ce sont
 * des raccourcis, pas un catalogue. Ils doivent tenir sous le titre sans
 * repousser le reste de la page.
 */
function ResumeGrid({
  tracks,
  onPlay,
}: {
  tracks: TrackSummary[];
  onPlay: (tracks: TrackSummary[], index: number) => void;
}) {
  return (
    // `auto-fill` et non un nombre fixe de colonnes : avec une seule écoute
    // en mémoire, une grille de quatre colonnes réduisait la carte au quart de
    // la largeur et tronquait le titre à « M.. ». Une largeur minimale garantit
    // qu'une carte reste lisible quel qu'en soit le nombre.
    <div
      className="mt-6 grid gap-2"
      style={{ gridTemplateColumns: "repeat(auto-fill, minmax(15rem, 1fr))" }}
    >
      {tracks.map((track, index) => (
        <button
          key={track.id}
          type="button"
          disabled={!track.isAvailable}
          onClick={() => onPlay(tracks, index)}
          className="group flex items-center gap-3 overflow-hidden rounded-md bg-elevated pr-3 text-left transition-colors hover:bg-raised disabled:opacity-40"
        >
          <Artwork hash={track.artworkHash} className="h-14 w-14 rounded-none" />

          <span className="flex min-w-0 flex-1 flex-col">
            <span className="block truncate text-[13px] font-semibold text-ink">
              {track.title}
            </span>
            <span className="block truncate text-[11px] text-ink-faint">
              {track.artist ?? "Artiste inconnu"}
            </span>
          </span>

          {/* Le bouton n'apparaît qu'au survol : la carte entière est déjà
              cliquable, il ne sert qu'à le rendre évident. */}
          <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-ink text-base opacity-0 transition-opacity group-hover:opacity-100">
            <span className="translate-x-[1px]">
              <Icon name="play" size={14} />
            </span>
          </span>
        </button>
      ))}
    </div>
  );
}

function MixCard({
  mix,
  loading,
  onClick,
}: {
  mix: HomeMix;
  loading: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      disabled={loading}
      onClick={onClick}
      className="group rounded-lg bg-surface p-3 text-left transition-colors hover:bg-elevated disabled:opacity-60"
    >
      <div className="relative overflow-hidden rounded-md">
        <CoverMosaic hashes={mix.coverHashes} />

        <span className="absolute bottom-2 right-2 flex h-11 w-11 items-center justify-center rounded-full bg-ink text-base opacity-0 shadow-xl shadow-black/40 transition-all duration-200 group-hover:translate-y-0 group-hover:opacity-100 translate-y-2">
          <span className={loading ? "animate-spin" : "translate-x-[1px]"}>
            <Icon name={loading ? "repeat" : "play"} size={18} />
          </span>
        </span>
      </div>

      <p className="mt-3 truncate text-[15px] font-semibold text-ink">{mix.title}</p>
      <p className="mt-0.5 line-clamp-2 text-[12px] leading-snug text-ink-faint">
        {mix.subtitle}
      </p>
    </button>
  );
}

/**
 * Mosaïque de pochettes.
 *
 * Une seule pochette disponible occupe tout le carré : un quadrant rempli et
 * trois vides ferait croire à un défaut d'affichage.
 */
function CoverMosaic({ hashes }: { hashes: string[] }) {
  if (hashes.length === 0) {
    return (
      <div className="flex aspect-square w-full items-center justify-center bg-gradient-to-br from-raised to-elevated">
        <Icon name="sparkle" size={32} className="text-ink-faint" />
      </div>
    );
  }

  if (hashes.length < 4) {
    return <Artwork hash={hashes[0] ?? null} className="aspect-square w-full rounded-none" />;
  }

  return (
    <div className="grid aspect-square w-full grid-cols-2 grid-rows-2">
      {hashes.slice(0, 4).map((hash, index) => (
        <Artwork key={`${hash}-${index}`} hash={hash} className="h-full w-full rounded-none" />
      ))}
    </div>
  );
}
