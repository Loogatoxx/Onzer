import { useEffect, useRef, useState } from "react";

import { Icon, IconButton, type IconName } from "@/components/Icon";
import {
  formatDate,
  formatDuration,
  type PlaylistSummary,
  type TrackSummary,
} from "@/lib/ipc";
import { Artwork } from "./Artwork";

/**
 * Gabarit de colonnes, partagé par l'en-tête et les lignes.
 *
 * Il est défini **une seule fois** : deux grilles indépendantes finissent
 * toujours par se désaligner d'un pixel ou deux, et c'est exactement le genre
 * de détail qui fait qu'une interface paraît bricolée.
 *
 * Les colonnes disparaissent par ordre d'importance à mesure que la place
 * manque — la date d'ajout d'abord, l'album ensuite. Le titre, lui, ne
 * disparaît jamais.
 */
const COLONNES =
  // # Deux dispositions, une seule grille
//
// En dessous de `lg`, il ne reste que le numéro, le titre et le menu : les
// paroles et la durée descendent **sous le titre**, comme sur n'importe quel
// lecteur de téléphone. Étaler quatre colonnes sur 375 px laissait au titre
// une centaine de pixels — on lisait « Somewhere Only We K… » et rien d'autre.
//
// Au-dessus, la place existe : l'album et la date d'ajout reprennent leur
// colonne, parce que les masquer sur un écran large serait gâcher l'espace
// qu'on vient de gagner.
"grid-cols-[1.25rem_minmax(0,1fr)_auto] lg:grid-cols-[1.75rem_minmax(0,2fr)_1.25rem_minmax(0,1.4fr)_3.25rem_auto] xl:grid-cols-[1.75rem_minmax(0,2fr)_1.25rem_minmax(0,1.4fr)_7rem_3.25rem_auto] items-center gap-2.5 lg:gap-4";

const GRID = `grid ${COLONNES}`;

interface TrackTableProps {
  tracks: TrackSummary[];
  currentTrackId: number | null;
  isPlaying: boolean;
  /** Lance la lecture à partir de cette position de la liste. */
  onPlay: (index: number) => void;
  onRadio: (trackId: number) => void;
  onToggleLoved: (trackId: number) => void;
  /** Ajoute à la fin de la file, sans interrompre l'écoute. */
  onEnqueue: (trackId: number) => void;
  /** Ouvre la page de l'artiste principal du morceau. */
  onOpenArtist: (trackId: number) => void;
  /** Retire de la bibliothèque. Le fichier n'est pas touché. */
  onRemove: (trackId: number) => void;
  /** Ouvre la correction manuelle du titre, de l'artiste et de l'album. */
  onCorrect: (track: TrackSummary) => void;
  /** Ouvre la recherche dans les catalogues, quand l'empreinte a échoué. */
  onMatch: (track: TrackSummary) => void;
  /** Cale à l'oreille les paroles d'un morceau qui les a sans horodatage. */
  onSyncLyrics: (track: TrackSummary) => void;
  /** Ouvre l'album du morceau. */
  onOpenAlbum: (track: TrackSummary) => void;
  /** Ouvre l'écran de lecture, quand on touche le morceau déjà en cours. */
  onOpenPlaying: () => void;
  /**
   * Tri courant, quand la liste en accepte un.
   *
   * Absent — les recommandations, une playlist — l'en-tête ne se clique pas :
   * l'ordre y **porte le sens** (c'est le moteur ou l'utilisateur qui l'a
   * choisi), et le trier autrement le détruirait.
   */
  sort?: TrackSort;
  onSort?: (column: SortColumn) => void;
  /**
   * Favoris, tenus par la coquille.
   *
   * `TrackSummary.isLoved` sert à les amorcer, mais ne peut pas rester la
   * source : le même morceau peut être aimé depuis la barre de lecture, et
   * deux copies de la vérité finiraient par diverger sous les yeux de
   * l'utilisateur.
   */
  loved: ReadonlySet<number>;
  /** Playlists proposées dans le menu « Ajouter à ». */
  playlists: PlaylistSummary[];
  onAddToPlaylist: (playlistId: number, trackId: number) => void;
  /** Fourni uniquement dans une playlist : retirer la ligne à cette position. */
  onRemoveAt?: (position: number) => void;
  /**
   * Raison de présence, quand la liste vient du moteur de recommandation.
   * Une recommandation inexplicable inspire la méfiance.
   */
  reasons?: Map<number, string>;
  emptyMessage?: string;
}

/** Les colonnes sur lesquelles une liste peut être triée. */
/**
 * Délai au-delà duquel un appui devient un appui long.
 *
 * 450 ms : au-dessous, un clic un peu appuyé ouvrirait le menu par surprise ;
 * au-dessus, on croit que rien ne se passe et on relâche.
 */
const APPUI_LONG_MS = 450;

export type SortColumn = "title" | "album" | "duration" | "added";

export interface TrackSort {
  column: SortColumn;
  descending: boolean;
}

/**
 * En-tête de colonne cliquable.
 *
 * # Pourquoi la flèche et pas seulement la couleur
 *
 * Savoir *qu'une* colonne trie ne dit pas dans quel sens. La flèche le montre,
 * et le second clic l'inverse — c'est le geste que tout tableau a appris à
 * faire depuis trente ans.
 */
function SortHeader({
  column,
  label,
  sort,
  onSort,
  className = "",
  children,
}: {
  column: SortColumn;
  label: string;
  sort: TrackSort | undefined;
  onSort: ((column: SortColumn) => void) | undefined;
  className?: string;
  children?: React.ReactNode;
}) {
  if (onSort === undefined) {
    return <span className={className}>{children ?? label}</span>;
  }

  const actif = sort?.column === column;

  return (
    <button
      type="button"
      onClick={() => onSort(column)}
      className={`flex items-center gap-1 text-left uppercase tracking-[0.14em] transition-colors hover:text-ink ${
        actif ? "text-ink" : ""
      } ${className}`}
    >
      {children ?? label}
      {actif && (
        <span className={sort?.descending === true ? "" : "rotate-180"}>
          <Icon name="chevronDown" size={13} />
        </span>
      )}
    </button>
  );
}

export function TrackTable({
  tracks,
  currentTrackId,
  isPlaying,
  onPlay,
  onRadio,
  onToggleLoved,
  onEnqueue,
  onOpenArtist,
  onRemove,
  onCorrect,
  onMatch,
  onSyncLyrics,
  onOpenAlbum,
  onOpenPlaying,
  loved,
  playlists,
  onAddToPlaylist,
  onRemoveAt,
  reasons,
  sort,
  onSort,
  emptyMessage = "Rien à afficher ici.",
}: TrackTableProps) {
  if (tracks.length === 0) {
    return (
      <p className="px-6 py-20 text-center text-sm text-ink-muted">{emptyMessage}</p>
    );
  }

  return (
    <div className="px-1 pb-8 lg:px-3">
      {onSort !== undefined && (
        <TriCompact sort={sort} onSort={onSort} />
      )}

      <div
        // # Pourquoi le décalage suit la coquille et non la ligne
        //
        // Ces soixante-quatre pixels sont la hauteur de la barre du haut, qui
        // n'existe qu'à partir de 768 px. En dessous, l'en-tête se collait donc
        // **à l'intérieur** de la liste : une bande « # TITRE » flottait au
        // milieu des morceaux, avec des lignes visibles au-dessus d'elle et une
        // autre à moitié cachée dessous.
        //
        // La densité des lignes bascule à 1024 px, la barre du haut à 768 :
        // deux seuils différents parce qu'ils répondent à deux questions
        // différentes — la place pour des colonnes, la place pour une barre
        // latérale. Celui-ci doit suivre la barre.
        className={`${COLONNES} hidden lg:grid sticky top-0 z-10 mb-1 border-b border-line bg-surface/85 px-1.5 py-2 md:top-[64px] lg:px-3 text-[11px] font-semibold uppercase tracking-[0.14em] text-ink-faint backdrop-blur`}
      >
        <span className="text-center">#</span>
        <SortHeader column="title" label="Titre" sort={sort} onSort={onSort} />
        {/* Colonne sans en-tête : « Paroles » écrit au-dessus d'une pastille
            large d'un caractère déborderait sur le titre. L'icône se comprend
            au survol, où elle porte son infobulle. */}
        <span aria-label="Paroles" className="hidden lg:block" />
        <SortHeader
          column="album"
          label="Album"
          sort={sort}
          onSort={onSort}
          className="hidden lg:block"
        />
        <SortHeader
          column="added"
          label="Ajouté"
          sort={sort}
          onSort={onSort}
          className="hidden xl:block"
        />
        {/* L'horloge est **dans** la colonne des durées, pas au-dessus du
            bloc d'actions : elle annonce ces chiffres-là, elle doit tomber
            dessus. */}
        <span className="hidden justify-center lg:flex">
          <SortHeader column="duration" label="" sort={sort} onSort={onSort}>
            <Icon name="clock" size={15} />
          </SortHeader>
        </span>
        <span aria-hidden />
      </div>

      <ul>
        {tracks.map((track, index) => (
          <TrackRow
            key={`${track.id}-${index}`}
            track={track}
            index={index}
            isCurrent={track.id === currentTrackId}
            isPlaying={isPlaying}
            onPlay={() => onPlay(index)}
            onRadio={() => onRadio(track.id)}
            onToggleLoved={() => onToggleLoved(track.id)}
            isLoved={loved.has(track.id)}
            onEnqueue={() => onEnqueue(track.id)}
            onOpenArtist={() => onOpenArtist(track.id)}
            onRemove={() => onRemove(track.id)}
            onCorrect={() => onCorrect(track)}
            onMatch={() => onMatch(track)}
            onSyncLyrics={() => onSyncLyrics(track)}
            onOpenAlbum={() => onOpenAlbum(track)}
            onOpenPlaying={onOpenPlaying}
            playlists={playlists}
            onAddToPlaylist={(playlistId) => onAddToPlaylist(playlistId, track.id)}
            {...(onRemoveAt === undefined
              ? {}
              : { onRemoveFromPlaylist: () => onRemoveAt(index) })}
            {...(reasons?.get(track.id) === undefined
              ? {}
              : { reason: reasons.get(track.id) as string })}
          />
        ))}
      </ul>
    </div>
  );
}

interface TrackRowProps {
  track: TrackSummary;
  index: number;
  isCurrent: boolean;
  isPlaying: boolean;
  onPlay: () => void;
  onRadio: () => void;
  onToggleLoved: () => void;
  isLoved: boolean;
  onEnqueue: () => void;
  onOpenArtist: () => void;
  onRemove: () => void;
  onCorrect: () => void;
  onMatch: () => void;
  onSyncLyrics: () => void;
  onOpenAlbum: () => void;
  onOpenPlaying: () => void;
  playlists: PlaylistSummary[];
  onAddToPlaylist: (playlistId: number) => void;
  /** Fourni uniquement dans une playlist : retirer la ligne à cette position. */
  onRemoveFromPlaylist?: () => void;
  reason?: string;
}

/**
 * Une ligne de morceau.
 *
 * Le numéro se change en bouton de lecture au survol. C'est la convention des
 * lecteurs modernes, et elle a une vraie raison d'être : elle évite un bouton
 * permanent par ligne, qui transformerait la liste en tableau de commandes.
 */
function TrackRow({
  track,
  index,
  isCurrent,
  isPlaying,
  onPlay,
  onRadio,
  onToggleLoved,
  isLoved,
  onEnqueue,
  onOpenArtist,
  onRemove,
  onCorrect,
  onMatch,
  onSyncLyrics,
  onOpenAlbum,
  onOpenPlaying,
  playlists,
  onAddToPlaylist,
  onRemoveFromPlaylist,
  reason,
}: TrackRowProps) {
  const unavailable = !track.isAvailable;

  /** Le menu, qu'on ouvre aux trois points ou d'un appui maintenu. */
  const [menuOpen, setMenuOpen] = useState(false);
  const minuteur = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const longPress = useRef(false);

  return (
    <li
      // # Un clic, pas deux
      //
      // Le double-clic vient du bureau, où une liste sert autant à
      // sélectionner qu'à ouvrir. Ici on ne sélectionne rien : on écoute. Et
      // sur un écran tactile, le double-clic n'existe simplement pas.
      //
      // Les zones qui font autre chose — nom d'artiste, album, menu — arrêtent
      // la propagation elles-mêmes.
      onClick={() => {
        if (unavailable) return;
        // Un appui long vient de servir : ce clic est sa retombée, pas une
        // intention. L'ignorer évite de lancer la lecture sous le menu qu'on
        // vient d'ouvrir.
        if (longPress.current) {
          longPress.current = false;
          return;
        }

        // Relancer depuis le début un morceau déjà en cours n'est pas ce qu'on
        // demande en le touchant : on veut le **voir**.
        if (isCurrent) onOpenPlaying();
        else onPlay();
      }}
      // # L'appui long ouvre le menu
      //
      // Viser trois points de seize pixels au bout d'une ligne est le geste le
      // plus difficile de l'interface. Maintenir le doigt là où il est déjà
      // n'en demande aucun — c'est ce que font tous les lecteurs de téléphone,
      // et le menu contient exactement les mêmes actions.
      onContextMenu={(event) => {
        event.preventDefault();
        setMenuOpen(true);
      }}
      onPointerDown={() => {
        longPress.current = false;
        minuteur.current = setTimeout(() => {
          longPress.current = true;
          setMenuOpen(true);
        }, APPUI_LONG_MS);
      }}
      onPointerUp={() => clearTimeout(minuteur.current)}
      onPointerLeave={() => clearTimeout(minuteur.current)}
      onPointerCancel={() => clearTimeout(minuteur.current)}
      className={`${GRID} pression group rounded-md px-1.5 py-2 transition-colors hover:bg-elevated lg:px-3 ${
        isCurrent ? "bg-elevated/60" : ""
      } ${unavailable ? "opacity-40" : ""}`}
    >
      {/* ── Numéro / lecture ─────────────────────────────────────────── */}
      <div className="flex h-9 w-5 items-center justify-center lg:w-7">
        {isCurrent ? (
          <span className="group-hover:hidden">
            <PlayingIndicator animated={isPlaying} />
          </span>
        ) : (
          <span className="numerals text-sm text-ink-faint group-hover:hidden">
            {index + 1}
          </span>
        )}

        <button
          type="button"
          disabled={unavailable}
          aria-label={`Lire ${track.title}`}
          onClick={onPlay}
          className="hidden text-ink group-hover:flex disabled:cursor-not-allowed"
        >
          <Icon name={isCurrent && isPlaying ? "pause" : "play"} size={15} />
        </button>
      </div>

      {/* ── Titre ────────────────────────────────────────────────────── */}
      <div className="flex min-w-0 items-center gap-2.5 lg:gap-3">
        <Artwork hash={track.artworkHash} className="h-10 w-10 rounded" />

        <div className="min-w-0">
          <p
            className={`truncate text-[15px] font-medium leading-tight ${
              isCurrent ? "text-accent" : "text-ink"
            }`}
          >
            {track.title}
          </p>
          {/* # Pourquoi tout tient sur une seule seconde ligne
              La durée occupait une troisième ligne, et le bloc de texte
              dépassait alors la pochette en haut comme en bas : la ligne
              n'avait plus de cadre, l'image flottait au milieu d'un texte plus
              grand qu'elle. Deux lignes font exactement ses quarante pixels. */}
          <div className="flex min-w-0 items-center gap-1.5 text-[13px] leading-tight text-ink-muted">
            <span className="min-w-0 truncate">
              {track.artist === null ? (
                "Artiste inconnu"
              ) : (
                // Le nom d'un artiste est une porte : c'est ce qu'on attend de
                // lui partout ailleurs, et le lire sans pouvoir y aller donne
                // l'impression d'une impasse.
                <button
                  type="button"
                  onClick={(event) => {
                    event.stopPropagation();
                    onOpenArtist();
                  }}
                  className="block max-w-full truncate transition-colors hover:text-ink hover:underline"
                >
                  {track.artist}
                </button>
              )}
            </span>
            {unavailable && <span className="shrink-0 text-warn">hors ligne</span>}

            {/* La durée n'a sa colonne qu'au-dessus de `lg` : en compact elle
                suit l'artiste, précédée du point qui la détache du nom. */}
            <span className="flex shrink-0 items-center gap-1 text-[12px] text-ink-faint lg:hidden">
              <span aria-hidden>·</span>
              {track.hasLyrics && (
                <span title="Paroles disponibles">
                  <Icon name="lyrics" size={12} />
                </span>
              )}
              <span className="numerals">{formatDuration(track.durationMs)}</span>
            </span>
          </div>

          {reason !== undefined && (
            <p className="mt-0.5 truncate text-[11px] text-ink-faint">{reason}</p>
          )}
        </div>
      </div>

      {/* ── Paroles ──────────────────────────────────────────────────── */}
      <span
        className="hidden lg:block"
        title={
          track.hasLyrics
            ? "Paroles disponibles"
            : "Pas de paroles — à récupérer depuis le panneau de lecture"
        }
      >
        <span className={track.hasLyrics ? "text-ink-muted" : "text-ink-faint/25"}>
          <Icon name="lyrics" size={14} />
        </span>
      </span>

      {/* ── Album ────────────────────────────────────────────────────── */}
      <p className="hidden min-w-0 truncate text-[13px] text-ink-muted lg:block">
        {track.album === null ? (
          "—"
        ) : (
          <button
            type="button"
            onClick={(event) => {
              event.stopPropagation();
              onOpenAlbum();
            }}
            className="max-w-full truncate transition-colors hover:text-ink hover:underline"
          >
            {track.album}
          </button>
        )}
      </p>

      {/* ── Date d'ajout ─────────────────────────────────────────────── */}
      <p className="hidden truncate text-[13px] text-ink-faint xl:block">
        {formatDate(track.addedAt)}
      </p>

      {/* ── Durée ────────────────────────────────────────────────────── */}
      <span className="numerals hidden text-center text-[13px] text-ink-muted lg:block">
        {formatDuration(track.durationMs)}
      </span>

      {/* ── Actions ──────────────────────────────────────────────────── */}
      <div className="-mr-1 flex items-center justify-end gap-1 lg:mr-0">
        <button
          type="button"
          title={isLoved ? "Retirer des favoris" : "Ajouter aux favoris"}
          aria-label={isLoved ? "Retirer des favoris" : "Ajouter aux favoris"}
          onClick={onToggleLoved}
          // # Pourquoi le cœur ne se cache plus
          //
          // Il n'apparaissait qu'au survol. Sur un écran tactile, le survol
          // n'existe pas : le cœur était donc **invisible**, et l'on découvrait
          // les favoris en appuyant par hasard. Même sur un bureau, un contour
          // gris se voit sans encombrer.
          className={`hidden h-8 w-8 items-center justify-center rounded-full transition-colors lg:flex ${
            isLoved ? "text-accent" : "text-ink-faint/70 hover:text-ink"
          }`}
        >
          <Icon name={isLoved ? "heartFilled" : "heart"} size={16} />
        </button>

        <RowMenu
          track={track}
          isLoved={isLoved}
          playlists={playlists}
          onRadio={onRadio}
          onAddToPlaylist={onAddToPlaylist}
          onToggleLoved={onToggleLoved}
          onEnqueue={onEnqueue}
          onOpenArtist={onOpenArtist}
          onRemove={onRemove}
          onCorrect={onCorrect}
          onMatch={onMatch}
          onSyncLyrics={onSyncLyrics}
          open={menuOpen}
          onOpenChange={setMenuOpen}
          {...(onRemoveFromPlaylist === undefined ? {} : { onRemoveFromPlaylist })}
        />
      </div>
    </li>
  );
}

/** Ce que chaque colonne triable s'appelle, pour l'écrit et pour le menu. */
const NOMS_DE_TRI: Record<SortColumn, string> = {
  title: "Titre",
  album: "Album",
  duration: "Durée",
  added: "Ajouté",
};

/**
 * Le tri, sur un écran sans colonnes.
 *
 * # Pourquoi l'en-tête ne convenait pas
 *
 * Trier se faisait en visant « TITRE » ou « ALBUM » écrits en petites
 * majuscules espacées, avec une flèche de treize pixels pour tout retour. Sur
 * un téléphone, l'en-tête coûtait une ligne entière pour deux mots qu'on ne
 * pense pas à toucher — rien ne dit qu'un titre de colonne est un bouton.
 *
 * Un bouton qui **dit son état** (« Titre ▾ ») coûte la même place et se
 * comprend sans qu'on ait à l'essayer. Le second appui sur le critère déjà
 * actif inverse le sens, comme un en-tête de tableau.
 */
function TriCompact({
  sort,
  onSort,
}: {
  sort: TrackSort | undefined;
  onSort: (column: SortColumn) => void;
}) {
  const [ouvert, setOuvert] = useState(false);
  const ancre = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!ouvert) return;

    const fermer = (event: MouseEvent) => {
      if (!ancre.current?.contains(event.target as Node)) setOuvert(false);
    };
    const echapper = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOuvert(false);
    };

    document.addEventListener("mousedown", fermer);
    document.addEventListener("keydown", echapper);
    return () => {
      document.removeEventListener("mousedown", fermer);
      document.removeEventListener("keydown", echapper);
    };
  }, [ouvert]);

  const courant = sort === undefined ? "Titre" : NOMS_DE_TRI[sort.column];

  return (
    <div ref={ancre} className="relative flex justify-end px-1.5 pb-1 lg:hidden">
      <button
        type="button"
        onClick={() => setOuvert(!ouvert)}
        aria-expanded={ouvert}
        className="flex items-center gap-1.5 rounded-full px-2 py-1 text-[12px] text-ink-muted transition-colors active:bg-elevated"
      >
        <Icon name="list" size={14} />
        {courant}
        <span
          className={`transition-transform duration-200 ${
            sort?.descending === true ? "" : "rotate-180"
          }`}
        >
          <Icon name="chevronDown" size={12} />
        </span>
      </button>

      {ouvert && (
        <div className="animate-surgir absolute right-1.5 top-8 z-30 w-44 overflow-hidden rounded-lg border border-line bg-raised py-1 shadow-2xl shadow-black/60">
          {(Object.keys(NOMS_DE_TRI) as SortColumn[]).map((colonne) => {
            const actif = sort?.column === colonne;
            return (
              <button
                key={colonne}
                type="button"
                onClick={() => {
                  onSort(colonne);
                  setOuvert(false);
                }}
                className={`flex w-full items-center justify-between px-3 py-2 text-left text-[13px] transition-colors active:bg-elevated ${
                  actif ? "text-ink" : "text-ink-muted"
                }`}
              >
                {NOMS_DE_TRI[colonne]}
                {actif && (
                  <span className={sort?.descending === true ? "" : "rotate-180"}>
                    <Icon name="chevronDown" size={12} />
                  </span>
                )}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

/**
 * Menu contextuel d'une ligne.
 *
 * Il se ferme au clic extérieur **et** à la touche Échap : un menu qu'on ne
 * peut fermer qu'en cliquant précisément à côté est une petite tyrannie.
 */
function RowMenu({
  track,
  isLoved,
  playlists,
  onRadio,
  onAddToPlaylist,
  onToggleLoved,
  onEnqueue,
  onOpenArtist,
  onRemove,
  onCorrect,
  onMatch,
  onSyncLyrics,
  onRemoveFromPlaylist,
  open,
  onOpenChange,
}: {
  track: TrackSummary;
  isLoved: boolean;
  playlists: PlaylistSummary[];
  onRadio: () => void;
  onAddToPlaylist: (playlistId: number) => void;
  onToggleLoved: () => void;
  onEnqueue: () => void;
  onOpenArtist: () => void;
  onRemove: () => void;
  onCorrect: () => void;
  onMatch: () => void;
  onSyncLyrics: () => void;
  onRemoveFromPlaylist?: () => void;
  /** L'ouverture appartient à la ligne : l'appui long la déclenche aussi. */
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const setOpen = onOpenChange;
  /** Deuxième clic exigé avant de retirer de la bibliothèque. */
  const [armed, setArmed] = useState(false);
  const anchor = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;

    const close = (event: MouseEvent) => {
      if (!anchor.current?.contains(event.target as Node)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };

    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", escape);
    };
  }, [open]);

  // Le menu refermé désarme la suppression : rouvrir ne doit jamais retomber
  // sur un bouton déjà à moitié pressé.
  useEffect(() => {
    if (!open) setArmed(false);
  }, [open]);

  function choose(action: () => void) {
    action();
    setOpen(false);
  }

  return (
    <div
      ref={anchor}
      className="relative"
      // # Pourquoi le menu retient les clics
      //
      // Il vit **dans** la ligne, et la ligne lance la lecture quand on la
      // touche. Sans cette barrière, choisir « Sauvegarder dans Titres likés »
      // faisait deux choses : le favori, et le morceau qui part. Deux actions
      // pour un geste, dont une qu'on n'a pas demandée — le défaut qu'on venait
      // de corriger sur le cœur, à un endroit qu'on n'avait pas regardé.
      onClick={(event) => event.stopPropagation()}
    >
      <IconButton
        name="more"
        label={`Autres actions pour ${track.title}`}
        active={open}
        size={16}
        onClick={(event) => {
          event.stopPropagation();
          setOpen(!open);
        }}
        // Sur un écran tactile, ce qui ne se montre qu'au survol ne se montre
        // jamais. Le menu reste donc visible en compact.
        className={open ? "" : "lg:opacity-0 lg:focus:opacity-100 lg:group-hover:opacity-100"}
      />

      {open && (
        <div className="animate-surgir absolute right-0 top-9 z-30 w-64 overflow-hidden rounded-lg border border-line bg-raised py-1 shadow-2xl shadow-black/60">
          <MenuItem icon="queue" onClick={() => choose(onEnqueue)}>
            Ajouter à la file d'attente
          </MenuItem>

          <MenuItem
            icon={isLoved ? "heartFilled" : "heart"}
            onClick={() => choose(onToggleLoved)}
          >
            {isLoved ? "Retirer des titres likés" : "Sauvegarder dans Titres likés"}
          </MenuItem>

          <MenuItem icon="radio" onClick={() => choose(onRadio)}>
            Lancer une radio
          </MenuItem>

          {track.artist !== null && (
            <MenuItem icon="artist" onClick={() => choose(onOpenArtist)}>
              Accéder à l'artiste
            </MenuItem>
          )}

          <MenuItem icon="pencil" onClick={() => choose(onCorrect)}>
            Corriger le titre
          </MenuItem>

          <MenuItem icon="search" onClick={() => choose(onMatch)}>
            Chercher ailleurs
          </MenuItem>

          {/* Le morceau a son texte mais pas ses temps : c'est exactement le
              cas que le modèle sait réparer, et le seul où la proposition a
              un sens. */}
          {track.hasLyrics && !track.hasSynced && (
            <MenuItem icon="sparkle" onClick={() => choose(onSyncLyrics)}>
              Caler les paroles
            </MenuItem>
          )}

          {onRemoveFromPlaylist !== undefined && (
            <MenuItem icon="close" onClick={() => choose(onRemoveFromPlaylist)}>
              Retirer de cette playlist
            </MenuItem>
          )}

          {playlists.length > 0 && (
            <>
              <MenuHeading>Ajouter à</MenuHeading>
              <div className="max-h-52 overflow-y-auto">
                {playlists.map((playlist) => (
                  <MenuItem
                    key={playlist.id}
                    icon="plus"
                    onClick={() => choose(() => onAddToPlaylist(playlist.id))}
                  >
                    {playlist.name}
                  </MenuItem>
                ))}
              </div>
            </>
          )}

          {/* Séparé du reste, et en deux temps : c'est la seule entrée du menu
              qui retire quelque chose de la bibliothèque. Le fichier, lui,
              n'est jamais touché — ce que dit la ligne d'explication. */}
          <div className="mt-1 border-t border-line pt-1">
            {armed ? (
              <button
                type="button"
                onClick={() => choose(onRemove)}
                className="flex w-full items-center gap-3 bg-danger/10 px-3 py-2 text-left text-[13px] font-semibold text-danger transition-colors hover:bg-danger/20"
              >
                <Icon name="trash" size={16} />
                Confirmer le retrait
              </button>
            ) : (
              <button
                type="button"
                onClick={() => setArmed(true)}
                className="flex w-full items-center gap-3 px-3 py-2 text-left text-[13px] text-ink-muted transition-colors hover:bg-elevated hover:text-danger"
              >
                <Icon name="trash" size={16} />
                Retirer de la bibliothèque
              </button>
            )}

            <p className="px-3 pb-1.5 pt-1 text-[11px] leading-snug text-ink-faint">
              Le fichier reste sur le disque, et ton historique d'écoute aussi.
            </p>
          </div>
        </div>
      )}
    </div>
  );
}

function MenuHeading({ children }: { children: React.ReactNode }) {
  return (
    <p className="mt-1 border-t border-line px-3 pb-1 pt-2 text-[11px] font-semibold uppercase tracking-[0.14em] text-ink-faint">
      {children}
    </p>
  );
}

function MenuItem({
  icon,
  onClick,
  children,
}: {
  icon: IconName;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex w-full items-center gap-3 px-3 py-2 text-left text-[13px] text-ink-muted transition-colors hover:bg-elevated hover:text-ink"
    >
      <Icon name={icon} size={16} />
      <span className="truncate">{children}</span>
    </button>
  );
}

/** Trois barres qui s'animent tant que la lecture est en cours. */
function PlayingIndicator({ animated }: { animated: boolean }) {
  return (
    <span className="flex h-3.5 items-end gap-[2px]" aria-hidden>
      {[0, 150, 300].map((delay, index) => (
        <span
          key={delay}
          className="w-[3px] rounded-sm bg-accent"
          style={
            animated
              ? {
                  height: "100%",
                  animation: `onzer-bars 900ms ${delay}ms ease-in-out infinite`,
                }
              : { height: index === 1 ? "60%" : "35%" }
          }
        />
      ))}
    </span>
  );
}
