import { useCallback, useEffect, useState } from "react";

import { Icon } from "@/components/Icon";
import { Artwork } from "@/features/library/Artwork";
import {
  ipc,
  type Category,
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
  /** Ouvre une catégorie d'ambiance en pleine page. */
  onOpenCategory: (key: string, title: string) => void;
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
export function HomeView({
  onPlayTrack,
  onGenerated,
  onError,
  onOpenCategory,
}: HomeViewProps) {
  const [home, setHome] = useState<Home | null>(null);
  const [categories, setCategories] = useState<Category[]>([]);
  const [busy, setBusy] = useState<string | null>(null);

  /**
   * Pourquoi l'accueil n'a rien pu montrer.
   *
   * L'erreur était avalée par un `catch` vide : quand la requête échouait,
   * la page restait sur « Préparation… » indéfiniment, sans que rien ne dise
   * pourquoi. Un défaut silencieux est un défaut qu'on ne peut pas corriger
   * — l'utilisateur ne peut que constater que « ça ne marche pas ».
   */
  const [failure, setFailure] = useState<string | null>(null);

  const reload = useCallback(() => {
    setFailure(null);
    void ipc
      .home()
      .then(setHome)
      .catch((cause: unknown) => setFailure(String(cause)));
    void ipc.categories().then(setCategories).catch(() => undefined);
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

  if (failure !== null) {
    return (
      <div className="px-6 py-16">
        <p className="text-sm text-ink">L&apos;accueil n&apos;a pas pu se construire.</p>
        <p className="mt-2 max-w-xl text-[13px] leading-relaxed text-danger">{failure}</p>
        <button
          type="button"
          onClick={reload}
          className="mt-5 rounded-full bg-elevated px-4 py-2 text-[13px] font-semibold text-ink-muted transition-colors hover:text-ink"
        >
          Réessayer
        </button>
      </div>
    );
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

      {categories.length > 0 && (
        <section className="mt-10">
          <h2 className="display text-[clamp(1.15rem,2.4vw,1.5rem)] text-ink">
            Explorer par ambiance
          </h2>
          <p className="mt-1 text-[12px] text-ink-faint">
            Calculé sur le son lui-même, et relatif à ta bibliothèque — aucun
            genre n'est nécessaire.
          </p>

          <div className="mt-4 grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-4">
            {categories.map((category) => (
              <CategoryCard
                key={category.key}
                category={category}
                onClick={() => onOpenCategory(category.key, category.title)}
              />
            ))}
          </div>
        </section>
      )}
    </div>
  );
}

/**
 * Carte de catégorie.
 *
 * Plus basse qu'une carte de mix, et la mosaïque y est un bandeau : une
 * catégorie n'est pas une écoute prête à lancer, c'est une porte vers une
 * liste. La différence de forme évite d'avoir à l'expliquer.
 */
function CategoryCard({
  category,
  onClick,
}: {
  category: Category;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="group overflow-hidden rounded-lg bg-surface text-left transition-colors hover:bg-elevated"
    >
      <div className="flex h-20 w-full">
        {category.coverHashes.length === 0 ? (
          <div className="h-full w-full bg-raised" />
        ) : (
          category.coverHashes.slice(0, 4).map((hash, index) => (
            <Artwork
              key={`${hash}-${index}`}
              hash={hash}
              className="h-full flex-1 rounded-none"
            />
          ))
        )}
      </div>

      <div className="p-3">
        <p className="truncate text-[15px] font-semibold text-ink">{category.title}</p>
        <p className="mt-0.5 line-clamp-2 text-[12px] leading-snug text-ink-faint">
          {category.subtitle}
        </p>
        <p className="numerals mt-1.5 text-[11px] text-ink-faint">
          {category.trackCount} titres
        </p>
      </div>
    </button>
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
      // 10 rem sur un téléphone : deux colonnes plutôt qu'une. Huit écoutes
      // récentes empilées sur toute la largeur remplissaient l'accueil à elles
      // seules, et repoussaient les mix hors de l'écran.
      className="mt-6 grid gap-2"
      style={{ gridTemplateColumns: "repeat(auto-fill, minmax(min(10rem, 100%), 1fr))" }}
    >
      {tracks.map((track, index) => (
        <button
          key={track.id}
          type="button"
          disabled={!track.isAvailable}
          onClick={() => onPlay(tracks, index)}
          className="pression group flex items-center gap-3 overflow-hidden rounded-md bg-elevated pr-3 text-left hover:bg-raised disabled:opacity-40"
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

        <span className="absolute bottom-2 right-2 flex h-11 w-11 items-center justify-center rounded-full bg-ink text-base opacity-0 shadow-lg shadow-black/40 transition-all duration-200 group-hover:translate-y-0 group-hover:opacity-100 translate-y-2">
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
      <div className="flex aspect-square w-full items-center justify-center bg-raised">
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
