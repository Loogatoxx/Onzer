import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { Icon, IconButton, type IconName } from "@/components/Icon";
import {
  formatDate,
  formatDuration,
  type PlaylistSummary,
  type TrackSummary,
} from "@/lib/ipc";
import { useIsMobile } from "@/lib/useIsMobile";
import { Artwork } from "./Artwork";
import { useFermeture } from "@/lib/useFermeture";

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
// # Pourquoi la dernière colonne a une largeur fixe
//
// Elle valait `auto`, c'est-à-dire « aussi large que son contenu ». Les
// lignes y mettent un cœur et trois points ; l'en-tête n'y met rien. Les deux
// grilles n'avaient donc pas la même dernière colonne — et comme la largeur
// totale, elle, est la même, la différence était reprise par les colonnes
// souples. Tout ce qui suivait le titre glissait : « ALBUM » se retrouvait à
// soixante pixels de l'album, « AJOUTÉ » à quatre-vingt-dix de la date.
//
// Deux boutons de trente-deux pixels et leur écart : quatre rem un quart. Un
// seul en dessous de `lg`, où le cœur ne s'affiche pas.
// Les lectures n'apparaissent qu'au-delà de `xl` : c'est la seule largeur
// où une septième colonne ne prend rien au titre.
"grid-cols-[1.25rem_minmax(0,1fr)_2rem] lg:grid-cols-[1.75rem_minmax(0,2fr)_1.25rem_minmax(0,1.4fr)_3.25rem_4.25rem] xl:grid-cols-[1.75rem_minmax(0,2fr)_1.25rem_minmax(0,1.4fr)_7rem_3.5rem_3.25rem_4.25rem] items-center gap-2.5 lg:gap-4";

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
  /** Insère juste après le morceau en cours. */
  onPlayNext: (trackId: number) => void;
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
   * Revenir à l'ordre que la liste avait d'elle-même.
   *
   * Fourni partout sauf sur la bibliothèque, dont l'ordre « naturel » est déjà
   * un tri par date d'ajout : y proposer « ordre d'origine » à côté de
   * « Ajouté » nommerait deux fois la même chose.
   */
  onSortReset?: () => void;
  /**
   * Favoris, tenus par la coquille.
   *
   * `TrackSummary.isLoved` sert à les amorcer, mais ne peut pas rester la
   * source : le même morceau peut être aimé depuis la barre de lecture, et
   * deux copies de la vérité finiraient par diverger sous les yeux de
   * l'utilisateur.
   */
  loved: ReadonlySet<number>;
  /**
   * Les morceaux cochés, quand on est en mode sélection.
   *
   * `undefined` — et non un ensemble vide — signifie « pas de sélection en
   * cours ». Les deux états sont différents : un ensemble vide, c'est une
   * sélection ouverte dans laquelle on n'a encore rien pris.
   */
  selection?: ReadonlySet<number>;
  onSelect?: (trackId: number) => void;
  /** Playlists proposées dans le menu « Ajouter à ». */
  playlists: PlaylistSummary[];
  onAddToPlaylist: (playlistId: number, trackId: number) => void;
  /** Fourni uniquement dans une playlist : retirer la ligne à cette position. */
  onRemoveAt?: (position: number) => void;
  /**
   * Ce que « retirer » veut dire ici.
   *
   * Une playlist et une file d'attente se quittent différemment : l'une perd
   * un morceau pour de bon, l'autre seulement pour cette écoute. Le même mot
   * pour les deux ferait hésiter avant de cliquer.
   */
  libelleRetrait?: string;
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

export type SortColumn = "title" | "album" | "duration" | "added" | "plays";

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
        <span
                    className={`transition-transform duration-200 ${
                      sort?.descending === true ? "" : "rotate-180"
                    }`}
                  >
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
  onPlayNext,
  onOpenArtist,
  onRemove,
  onCorrect,
  onMatch,
  onSyncLyrics,
  onOpenAlbum,
  onOpenPlaying,
  loved,
  selection,
  onSelect,
  playlists,
  onAddToPlaylist,
  onRemoveAt,
  libelleRetrait = "Retirer de cette playlist",
  reasons,
  sort,
  onSort,
  onSortReset,
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
        <TriCompact sort={sort} onSort={onSort} onSortReset={onSortReset} />
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
        {/* # Les lectures, et lesquelles
            Le chiffre de Spotify est mondial ; celui-ci est le tien. C'est la
            seule colonne qu'un lecteur hors ligne peut remplir mieux qu'un
            service en ligne. */}
        <SortHeader
          column="plays"
          label="Lectures"
          sort={sort}
          onSort={onSort}
          className="hidden justify-end xl:flex"
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
            {...(selection === undefined || onSelect === undefined
              ? {}
              : { selectionnee: selection.has(track.id), onSelect: () => onSelect(track.id) })}
            onEnqueue={() => onEnqueue(track.id)}
            onPlayNext={() => onPlayNext(track.id)}
            onOpenArtist={() => onOpenArtist(track.id)}
            onRemove={() => onRemove(track.id)}
            onCorrect={() => onCorrect(track)}
            onMatch={() => onMatch(track)}
            onSyncLyrics={() => onSyncLyrics(track)}
            onOpenAlbum={() => onOpenAlbum(track)}
            onOpenPlaying={onOpenPlaying}
            playlists={playlists}
            onAddToPlaylist={(playlistId) => onAddToPlaylist(playlistId, track.id)}
            libelleRetrait={libelleRetrait}
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
  /** Cochée ? `undefined` quand aucune sélection n'est en cours. */
  selectionnee?: boolean;
  onSelect?: () => void;
  onPlay: () => void;
  onRadio: () => void;
  onToggleLoved: () => void;
  isLoved: boolean;
  onEnqueue: () => void;
  onPlayNext: () => void;
  onOpenArtist: () => void;
  onRemove: () => void;
  onCorrect: () => void;
  onMatch: () => void;
  onSyncLyrics: () => void;
  onOpenAlbum: () => void;
  libelleRetrait: string;
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
  selectionnee,
  onSelect,
  onPlay,
  onRadio,
  onToggleLoved,
  isLoved,
  onEnqueue,
  onPlayNext,
  onOpenArtist,
  libelleRetrait,
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
        // # Pourquoi la sélection prend toute la place
        //
        // Tant qu'elle est ouverte, un appui ne peut plus vouloir dire deux
        // choses. Laisser la lecture cohabiter obligerait à viser une case de
        // vingt pixels pour cocher, et à éviter le reste de la ligne pour ne
        // pas partir sur un autre morceau.
        if (onSelect !== undefined) {
          onSelect();
          return;
        }

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
      // Le repère que « aller au morceau en cours » vise. Un attribut plutôt
      // qu'une référence remontée : la ligne peut être n'importe où dans une
      // liste paginée, et c'est au défilement de la trouver, pas au tableau de
      // la tenir.
      data-courant={isCurrent ? "" : undefined}
      // # Pourquoi une ligne s'éclaire au lieu de s'enfoncer
      //
      // `.pression` met l'élément à l'échelle. Sur une pastille de trente-six
      // pixels, c'est un enfoncement ; sur une ligne large de tout l'écran,
      // c'est cinq pixels de chaque bord qui bougent — et surtout, une mise à
      // l'échelle crée un **bloc conteneur** : tout ce qui est `fixed` à
      // l'intérieur, comme la feuille du menu, se met à se positionner par
      // rapport à la ligne au lieu de l'écran.
      //
      // Les listes natives, elles, s'éclairent. C'est ce qu'on attend d'une
      // ligne, et cela ne piège rien.
      className={`${GRID} group rounded-md px-1.5 py-2 transition-colors hover:bg-elevated active:bg-raised lg:px-3 ${
        isCurrent ? "bg-elevated/60" : ""
      } ${unavailable ? "opacity-40" : ""}`}
    >
      {/* ── Numéro / lecture ─────────────────────────────────────────── */}
      {/* `relative` n'est pas décoratif : le numéro et le bouton de lecture
          sont posés en absolu l'un sur l'autre pour se fondre. Sans ancre
          ici, ils se positionnent par rapport à la page — tous les numéros
          s'empilent au même endroit et ne défilent plus. */}
      <div className="relative flex h-9 w-5 items-center justify-center lg:w-7">
        {selectionnee !== undefined ? (
          <span
            aria-hidden
            className={`flex h-[18px] w-[18px] items-center justify-center rounded-full border transition-colors ${
              selectionnee
                ? "border-accent bg-accent text-base"
                : "border-ink-faint text-transparent"
            }`}
          >
            <Icon name="check" size={11} />
          </span>
        ) : isCurrent ? (
          <span className="absolute transition-opacity group-hover:opacity-0">
            <PlayingIndicator animated={isPlaying} />
          </span>
        ) : (
          <span className="numerals absolute text-sm text-ink-faint transition-opacity group-hover:opacity-0">
            {index + 1}
          </span>
        )}

        {/* # Pourquoi une opacité et non un `display`

            Le numéro disparaissait et le bouton apparaissait par bascule de
            `display` : deux permutations sans état intermédiaire, sur la même
            case, à chaque ligne survolée. C'était le clignotement le plus
            visible de la bibliothèque.

            Les clics restent inertes tant que le bouton est transparent :
            sur un écran tactile le survol n'existe pas, et un bouton invisible
            posé sur le numéro capterait les appuis destinés à la ligne. */}
        <button
          type="button"
          disabled={unavailable}
          aria-label={`Lire ${track.title}`}
          onClick={onPlay}
          className="pointer-events-none absolute flex text-ink opacity-0 transition-opacity disabled:cursor-not-allowed group-hover:pointer-events-auto group-hover:opacity-100"
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
        <span className={track.hasLyrics ? "text-ink-muted" : "text-ink-faint/45"}>
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

      {/* ── Lectures ─────────────────────────────────────────────────── */}
      <span className="numerals hidden text-right text-[13px] text-ink-faint xl:block">
        {track.playCount === 0 ? "—" : track.playCount.toLocaleString("fr-FR")}
      </span>

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
          onPlayNext={onPlayNext}
          libelleRetrait={libelleRetrait}
          onOpenArtist={onOpenArtist}
          onOpenAlbum={onOpenAlbum}
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
  plays: "Lectures",
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
  onSortReset,
}: {
  sort: TrackSort | undefined;
  onSort: (column: SortColumn) => void;
  onSortReset?: (() => void) | undefined;
}) {
  const [ouvert, setOuvert] = useState(false);
  const monte = useFermeture(ouvert);
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

  const courant =
    sort === undefined
      ? onSortReset === undefined
        ? "Titre"
        : "Ordre d'origine"
      : NOMS_DE_TRI[sort.column];

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

      {monte && (
        <div className={`${ouvert ? "animate-surgir" : "animate-disparaitre"} absolute right-1.5 top-8 z-30 w-44 overflow-hidden rounded-lg border border-line bg-raised py-1 shadow-2xl shadow-black/60`}>
          {/* # Pourquoi il faut un chemin de retour

              Une playlist a **son** ordre : celui dans lequel on l'a rangée.
              Trier par titre l'écrase, et rien ne permettait d'y revenir — il
              fallait quitter la page et y retourner. C'est ce que Spotify
              appelle « Tri personnalisé » : ce n'est pas un tri de plus, c'est
              l'absence de tri, et elle mérite d'être nommée. */}
          {onSortReset !== undefined && (
            <button
              type="button"
              onClick={() => {
                onSortReset();
                setOuvert(false);
              }}
              className={`pression flex w-full items-center justify-between gap-3 px-3 py-2 text-left text-[13px] ${
                sort === undefined ? "text-ink" : "text-ink-muted hover:bg-elevated hover:text-ink"
              }`}
            >
              Ordre d&apos;origine
              {sort === undefined && <Icon name="check" size={13} />}
            </button>
          )}

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
                  <span
                    className={`transition-transform duration-200 ${
                      sort?.descending === true ? "" : "rotate-180"
                    }`}
                  >
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
  onPlayNext,
  onOpenArtist,
  onOpenAlbum,
  onRemove,
  onCorrect,
  onMatch,
  onSyncLyrics,
  onRemoveFromPlaylist,
  libelleRetrait,
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
  onPlayNext: () => void;
  onOpenArtist: () => void;
  onRemove: () => void;
  onCorrect: () => void;
  onMatch: () => void;
  onSyncLyrics: () => void;
  onOpenAlbum: () => void;
  onRemoveFromPlaylist?: () => void;
  libelleRetrait: string;
  /** L'ouverture appartient à la ligne : l'appui long la déclenche aussi. */
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const setOpen = onOpenChange;
  const monte = useFermeture(open, 200);
  const mobile = useIsMobile();
  /**
   * Ce que le doigt a tiré vers le bas, en pixels.
   *
   * Une feuille qui monte du bas se renvoie d'où elle vient : c'est le geste
   * qu'on fait partout ailleurs, et il évite de viser une croix ou de deviner
   * où finit le menu.
   */
  const [tire, setTire] = useState(0);
  /** Vrai pendant qu'elle achève sa descente, après un geste abouti. */
  const [ferme, setFerme] = useState(false);
  const depart = useRef<number | null>(null);

  // Rouvrir repart d'en bas : sans cette remise à zéro, la feuille
  // réapparaîtrait à l'endroit où on l'a laissée tomber.
  useEffect(() => {
    if (!open) return;
    setTire(0);
    setFerme(false);
  }, [open]);
  /** Deuxième clic exigé avant de retirer de la bibliothèque. */
  const [armed, setArmed] = useState(false);
  const anchor = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    // La feuille du téléphone vit à la racine du document : elle n'est plus
    // « dans » l'ancre, et ce guetteur la refermerait au premier appui, y
    // compris sur ses propres entrées. Elle a son voile pour cela.
    if (mobile) {
      const echapper = (event: KeyboardEvent) => {
        if (event.key === "Escape") setOpen(false);
      };
      document.addEventListener("keydown", echapper);
      return () => document.removeEventListener("keydown", echapper);
    }

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
  }, [open, mobile]);

  // Le menu refermé désarme la suppression : rouvrir ne doit jamais retomber
  // sur un bouton déjà à moitié pressé.
  useEffect(() => {
    if (!open) setArmed(false);
  }, [open]);

  function choose(action: () => void) {
    action();
    setOpen(false);
  }

  const entrees = (
    <EntreesDeLigne
      track={track}
      isLoved={isLoved}
      playlists={playlists}
      armed={armed}
      setArmed={setArmed}
      choose={choose}
      onEnqueue={onEnqueue}
      onPlayNext={onPlayNext}
      onToggleLoved={onToggleLoved}
      onRadio={onRadio}
      onOpenArtist={onOpenArtist}
      onOpenAlbum={onOpenAlbum}
      onCorrect={onCorrect}
      onMatch={onMatch}
      onSyncLyrics={onSyncLyrics}
      onRemove={onRemove}
      onAddToPlaylist={onAddToPlaylist}
      libelleRetrait={libelleRetrait}
      {...(onRemoveFromPlaylist === undefined ? {} : { onRemoveFromPlaylist })}
    />
  );

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

      {monte
        && (mobile ? (
          /* # Pourquoi la feuille est portée à la racine du document

             La ligne porte `.pression`, qui lui applique une mise à l'échelle
             tant que le doigt est posé. Or **un élément `fixed` placé dans un
             ancêtre transformé se positionne par rapport à lui**, pas par
             rapport à l'écran : le menu s'ouvrant au bout de l'appui long, il
             naissait dans une ligne encore enfoncée, puis sautait à sa vraie
             place quand le doigt se levait. C'est le « pas fluide » qu'on
             voyait, et aucune durée d'animation n'y aurait rien changé. */
          createPortal(
            <>
            {/* # Pourquoi une feuille, et pas le même menu

                Le menu du bureau est un rectangle de deux cent cinquante
                pixels accroché à un bouton, avec des lignes de trente-deux :
                on le vise à la souris. Au pouce, il tombe dans le coin de
                l'écran le plus difficile à atteindre, et ses lignes sont deux
                fois trop basses.

                La feuille part du bord inférieur — le seul que le pouce
                atteint sans changer de prise — et rappelle en tête **de quel
                morceau** on parle : ouvert par un appui long, le menu arrive
                sans qu'on ait vu ce qu'on a saisi. */}
            {/* # Pourquoi il ferme au `pointerdown`, et pourquoi il retient l'appui

                Il fermait au clic. Or un clic n'arrive qu'**après** le
                relâchement : entre-temps la feuille commençait sa sortie, et
                l'animation de sortie coupe les événements. Le clic tombait donc
                sur la ligne du dessous — et lançait un autre morceau.

                On ferme au premier contact, et l'on empêche le clic de naître :
                `preventDefault` sur un `pointerdown` supprime les événements de
                compatibilité qui suivraient. */}
            <div
              className={`fixed inset-0 z-40 bg-base/60 ${
                tire > 0 || ferme
                  ? ""
                  : open
                    ? "animate-apparaitre"
                    : "animate-disparaitre-voile"
              }`}
              // Le voile pâlit à mesure qu'on tire : à mi-chemin on voit déjà
              // que la feuille s'en va, ce qui rend le seuil devinable sans
              // avoir à l'écrire.
              style={
                tire > 0 || ferme
                  ? { opacity: ferme ? 0 : Math.max(0.15, 1 - tire / 320) }
                  : undefined
              }
              onPointerDown={(event) => {
                event.preventDefault();
                event.stopPropagation();
                setOpen(false);
              }}
            />

            <div
              className={`fixed inset-x-0 bottom-0 z-50 max-h-[80vh] overflow-y-auto rounded-t-2xl border-t border-line bg-raised pb-[calc(env(safe-area-inset-bottom,0px)+1rem)] shadow-2xl shadow-black/60 [&_button]:py-3.5 [&_button]:text-[15px] ${
                tire > 0 || ferme ? "" : open ? "barre-monte" : "barre-descend"
              }`}
              style={
                tire > 0 || ferme
                  ? {
                      transform: ferme ? "translateY(100%)" : `translateY(${tire}px)`,
                      transition: ferme ? "transform 200ms var(--ease-out-soft)" : "none",
                    }
                  : undefined
              }
            >
              {/* # Pourquoi une poignée dessinée

                  Un geste qui n'a pas de prise n'existe pas : rien ne disait
                  qu'on pouvait renvoyer la feuille en bas. Trente-six pixels
                  de barre au-dessus du titre le disent sans un mot, et c'est
                  la convention de toutes les feuilles qui montent du bas.

                  Tant qu'on tient, aucune transition : la moindre ferait un
                  retard sous le doigt. */}
              {/* # Pourquoi la poignée reste en haut

                  La feuille dépasse la hauteur de l'écran : on la fait
                  défiler. La poignée partait alors avec le contenu, et le
                  geste qu'elle annonce devenait impossible à saisir — éprouvé
                  sur l'appareil, poignée disparue au premier défilement. Elle
                  colle donc au bord, avec le titre qu'elle surmonte. */}
              <div
                className="sticky top-0 z-10 cursor-grab touch-none rounded-t-2xl bg-raised pt-2.5 active:cursor-grabbing"
                onTouchStart={(event) => {
                  const doigt = event.touches[0];
                  depart.current = doigt === undefined ? null : doigt.clientY;
                }}
                onTouchMove={(event) => {
                  const origine = depart.current;
                  const doigt = event.touches[0];
                  if (origine === null || doigt === undefined) return;
                  // Vers le bas seulement : tirer vers le haut décollerait la
                  // feuille de son bord, ce qui n'a nulle part où mener.
                  setTire(Math.max(0, doigt.clientY - origine));
                }}
                onTouchEnd={(event) => {
                  const origine = depart.current;
                  depart.current = null;

                  const doigt = event.changedTouches[0];
                  const parcouru =
                    origine === null || doigt === undefined ? 0 : doigt.clientY - origine;

                  if (parcouru > 90) {
                    setFerme(true);
                    setTimeout(() => setOpen(false), 190);
                    return;
                  }
                  setTire(0);
                }}
                onTouchCancel={() => {
                  depart.current = null;
                  setTire(0);
                }}
              >
                <span
                  aria-hidden
                  className="mx-auto block h-1 w-9 rounded-full bg-ink-faint/60"
                />

                <div className="mt-2 flex items-center gap-3 border-b border-line px-4 pb-3">
                  <Artwork hash={track.artworkHash} className="h-11 w-11 shrink-0 rounded" />
                  <span className="min-w-0">
                    <span className="block truncate text-[15px] font-semibold text-ink">
                      {track.title}
                    </span>
                    <span className="block truncate text-[13px] text-ink-muted">
                      {track.artist ?? "Artiste inconnu"}
                    </span>
                  </span>
                </div>
              </div>

              <div className="py-1">{entrees}</div>
            </div>
            </>,
            document.body,
          )
        ) : (
          <div
            className={`${open ? "animate-surgir" : "animate-disparaitre"} absolute right-0 top-9 z-30 w-64 overflow-hidden rounded-lg border border-line bg-raised py-1 shadow-2xl shadow-black/60`}
          >
            {entrees}
          </div>
        ))}
    </div>
  );
}

/**
 * Ce que le menu d'une ligne propose.
 *
 * Écrit une fois : la feuille du téléphone et le menu du bureau montrent la
 * même chose, et deux copies finiraient par diverger d'une entrée.
 */
function EntreesDeLigne({
  track,
  isLoved,
  playlists,
  armed,
  setArmed,
  choose,
  onEnqueue,
  onPlayNext,
  onToggleLoved,
  onRadio,
  onOpenArtist,
  onOpenAlbum,
  onCorrect,
  onMatch,
  onSyncLyrics,
  onRemoveFromPlaylist,
  onRemove,
  onAddToPlaylist,
  libelleRetrait,
}: {
  track: TrackSummary;
  isLoved: boolean;
  playlists: PlaylistSummary[];
  armed: boolean;
  setArmed: (value: boolean) => void;
  choose: (action: () => void) => void;
  onEnqueue: () => void;
  onPlayNext: () => void;
  onToggleLoved: () => void;
  onRadio: () => void;
  onOpenArtist: () => void;
  onOpenAlbum: () => void;
  onCorrect: () => void;
  onMatch: () => void;
  onSyncLyrics: () => void;
  onRemoveFromPlaylist?: () => void;
  onRemove: () => void;
  onAddToPlaylist: (playlistId: number) => void;
  libelleRetrait: string;
}) {
  return (
    <>
          <MenuItem icon="queue" onClick={() => choose(onEnqueue)}>
            Ajouter à la file d'attente
          </MenuItem>

          {/* # Pourquoi les deux existent
              « À la fin » d'une file de deux mille morceaux veut dire jamais.
              « Ensuite » est l'autre moitié du geste : ce morceau-là, tout de
              suite après, sans couper celui qui joue. */}
          <MenuItem icon="play" onClick={() => choose(onPlayNext)}>
            Lire ensuite
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

          {/* # Pourquoi il manquait, et pourquoi cela comptait
              Le nom de l'album n'est cliquable que dans sa colonne, et cette
              colonne disparaît en dessous de mille vingt-quatre pixels. Sur un
              téléphone, il n'existait donc **aucun** chemin d'un morceau vers
              son album — alors que celui vers l'artiste était là, juste
              au-dessus, dans ce même menu. */}
          {track.album !== null && (
            <MenuItem icon="library" onClick={() => choose(onOpenAlbum)}>
              Accéder à l&apos;album
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
              {libelleRetrait}
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
    </>
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
      className="pression flex w-full items-center gap-3 px-3 py-2 text-left text-[13px] text-ink-muted hover:bg-elevated hover:text-ink"
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
