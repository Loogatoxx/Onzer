import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import { CoverTile, HeaderAction, PageHeader } from "@/components/PageHeader";
import { Icon } from "@/components/Icon";
import { DiscoverBar } from "@/features/discover/DiscoverBar";
import { ArtistsView } from "@/features/artists/ArtistsView";
import { HomeView } from "@/features/home/HomeView";
import { IdentifyPanel } from "@/features/identify/IdentifyPanel";
import { SuspectPanel } from "@/features/identify/SuspectPanel";
import { ArtworkBar } from "@/features/lyrics/ArtworkBar";
import { AlbumBar } from "@/features/library/AlbumBar";
import { MaintenanceCard } from "@/features/library/MaintenanceCard";
import { OfflineBar } from "@/features/library/OfflineBar";
import { Pager } from "@/features/library/Pager";
import { LyricsBar } from "@/features/lyrics/LyricsBar";
import { ListenBar } from "@/features/lyrics/ListenBar";
import { Artwork } from "@/features/library/Artwork";
import { CorrectDialog } from "@/features/library/CorrectDialog";
import { MatchDialog } from "@/features/library/MatchDialog";
import { DuplicatePanel } from "@/features/library/DuplicatePanel";
import { TrackTable } from "@/features/library/TrackTable";
import { Sidebar, routeKey, type Route } from "@/features/nav/Sidebar";
import type { SortColumn, TrackSort } from "@/features/library/TrackTable";
import { MobileTabs } from "@/features/nav/MobileTabs";
import { useSwipeOnglets, type Sens } from "@/features/nav/useSwipeOnglets";
import {
  BarreFiltres,
  ListeRegroupements,
  useRegroupements,
  type FiltreRecherche,
} from "@/features/search/SearchFilters";
import { MiniPlayer } from "@/features/player/MiniPlayer";
import { useIsMobile } from "@/lib/useIsMobile";
import {
  oublierRecherches,
  recherchesRecentes,
  retenirRecherche,
} from "@/lib/recherchesRecentes";
import { TopBar } from "@/features/nav/TopBar";
import { NowPlayingPanel, type PanelTab } from "@/features/player/NowPlayingPanel";
import { LyricsView } from "@/features/player/LyricsView";
import { NowPlayingView } from "@/features/player/NowPlayingView";
import { PlayerBar } from "@/features/player/PlayerBar";
import { usePlayback } from "@/features/player/usePlayback";
import { ShortcutsView } from "@/features/nav/ShortcutsView";
import { SettingsView } from "@/features/nav/SettingsView";
import { PairingView } from "@/features/appairage/PairingView";
import { MoreView } from "@/features/nav/MoreView";
import { AlbumsView } from "@/features/library/AlbumsView";
import { AlbumRow } from "@/features/library/AlbumRow";
import { PlaylistsView } from "@/features/library/PlaylistsView";
import { SyncView } from "@/features/sync/SyncView";
import { WrappedView } from "@/features/stats/WrappedView";
import {
  formatDurationLong,
  ipc,
  type GeneratedPlaylist,
  type LibraryCounts,
  type PlaylistSummary,
  type QueueItem,
  type ScanProgress,
  type ScanSummary,
  type TrackSummary,
} from "@/lib/ipc";

/**
 * Pas de déplacement d'une flèche, selon le nombre de répétitions.
 *
 * Cinq secondes tant qu'on tapote — de quoi rattraper un passage manqué —
 * puis des paliers de plus en plus larges quand la touche reste enfoncée.
 */
function seekStep(repeats: number): number {
  if (repeats < 6) return 5_000;
  if (repeats < 15) return 15_000;
  return 30_000;
}

/** Délai avant de lancer une recherche, pour ne pas requêter à chaque frappe. */
const SEARCH_DEBOUNCE_MS = 200;

/**
 * Nombre de morceaux affichés d'un coup.
 *
 * # Pourquoi des pages, et pourquoi cent
 *
 * Le chargement à la volée réglait la question de la base ; il laissait
 * entière celle du **navigateur**. À deux mille lignes réellement dessinées —
 * chacune avec son menu, ses boutons et ses infobulles — le défilement
 * saccade, et c'est le rendu qui coûte, pas la requête.
 *
 * Cent lignes tiennent largement plus d'un écran, se dessinent
 * instantanément, et donnent un repère que le défilement infini n'offre
 * pas : on sait où l'on est et combien il reste.
 */
const PAGE_SIZE = 100;

/**
 * Coquille de l'application.
 *
 * # Une seule page, quatre zones
 *
 * ```text
 * ┌──────────────────────────────────────────────────┐
 * │  zone de glissement (feux de circulation macOS)  │
 * ├──────────┬───────────────────────────┬───────────┤
 * │ latérale │  barre du haut (collante) │ en cours  │
 * │          ├───────────────────────────┤ de        │
 * │ nav      │  page                     │ lecture   │
 * │ playlist │  (défile)                 │ + paroles │
 * ├──────────┴───────────────────────────┴───────────┤
 * │  barre de lecture, commandes centrées            │
 * └──────────────────────────────────────────────────┘
 * ```
 *
 * Rien n'est jamais démonté d'une navigation à l'autre : changer de page ne
 * doit pas donner l'impression d'avoir coupé la musique. C'est aussi ce qui
 * permet aux paroles de continuer à défiler pendant qu'on fouille sa
 * bibliothèque.
 */
export function AppShell({ libraryRoot }: { libraryRoot: string }) {
  // ── Navigation ────────────────────────────────────────────────────────
  // Une pile plutôt qu'un simple état courant : les flèches précédent/suivant
  // de la barre du haut n'ont de sens que s'il y a un historique.
  // L'accueil est la première page : ouvrir sur une liste de morceaux déjà
  // tous connus ne donne envie de rien.
  const [stack, setStack] = useState<Route[]>([{ kind: "home" }]);
  const [cursor, setCursor] = useState(0);
  const route: Route = stack[cursor] ?? { kind: "home" };

  const navigate = useCallback((next: Route) => {
    // La recherche est une **surimpression**, pas une destination : tant qu'un
    // terme est saisi, elle masque la page. Naviguer sans l'effacer donnait
    // l'impression que la barre latérale ne répondait plus — il fallait vider
    // le champ caractère par caractère pour retrouver l'application.
    setQuery("");
    setStack((previous) => [...previous.slice(0, cursor + 1), next]);
    setCursor((previous) => previous + 1);
  }, [cursor]);

  // ── Données ───────────────────────────────────────────────────────────
  const [tracks, setTracks] = useState<TrackSummary[]>([]);
  const [results, setResults] = useState<TrackSummary[] | null>(null);
  const [counts, setCounts] = useState<LibraryCounts | null>(null);
  const [playlists, setPlaylists] = useState<PlaylistSummary[]>([]);
  const [loved, setLoved] = useState<ReadonlySet<number>>(new Set());
  const [generated, setGenerated] = useState<GeneratedPlaylist | null>(null);

  const [query, setQuery] = useState("");
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const [summary, setSummary] = useState<ScanSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [panel, setPanel] = useState<PanelTab | "closed">("lyrics");
  /** Morceau en cours de correction manuelle. */
  const [correcting, setCorrecting] = useState<TrackSummary | null>(null);
  const [matching, setMatching] = useState<TrackSummary | null>(null);

  /**
   * Morceau dont on cale les paroles, et ce que l'écoute a donné.
   *
   * Le modèle prend une trentaine de secondes : sans retour visible, le clic
   * aurait l'air de n'avoir rien fait, et l'utilisateur cliquerait à nouveau.
   */
  const [syncing, setSyncing] = useState<TrackSummary | null>(null);
  const [syncNote, setSyncNote] = useState<string | null>(null);

  /**
   * Ouvre l'album d'un morceau connu par son seul identifiant.
   *
   * La file de lecture ne porte pas l'album : il faut le demander à la base,
   * comme on le fait déjà pour l'artiste.
   */
  async function openAlbumOf(trackId: number) {
    const [track] = await ipc.tracksByIds([trackId]);
    if (track?.albumId == null) return;

    navigate({
      kind: "album",
      id: track.albumId,
      name: track.album ?? "Album",
      artist: track.artist,
    });
  }

  function syncLyrics(track: TrackSummary) {
    setSyncing(track);
    setSyncNote(null);

    void ipc
      .syncTrack(track.id)
      .then(() => {
        setSyncNote(`« ${track.title} » : paroles calées.`);
        bump();
      })
      .catch((cause: unknown) => setSyncNote(String(cause)))
      .finally(() => setSyncing(null));
  }

  /** Incrémenté pour forcer un rechargement après une écriture. */
  const [revision, setRevision] = useState(0);
  const bump = useCallback(() => setRevision((value) => value + 1), []);

  const playback = usePlayback();

  /** Répétitions de la flèche en cours de maintien, pour l'accélération. */
  const repeats = useRef(0);

  const importing = progress !== null;
  /**
   * Tri courant de la bibliothèque.
   *
   * `null` : l'ordre d'entrée, celui qui ne prétend rien. La bibliothèque
   * étant paginée, le tri se fait **en base** — trier les cent lignes
   * affichées rendrait une page rangée au milieu d'un tout qui ne l'est pas.
   * Les listes qui arrivent entières se trient à l'écran, plus bas.
   */
  const [sort, setSort] = useState<TrackSort | null>(null);

  /** Un clic sur une colonne : on la prend, ou on l'inverse. */
  const toggleSort = useCallback((column: SortColumn) => {
    setPage(0);
    setSort((courant) =>
      courant?.column === column
        ? { column, descending: !courant.descending }
        : { column, descending: column === "added" },
    );
  }, []);


  const searching = query.trim() !== "";

  /**
   * Retenir la recherche, une fois qu'elle a donné quelque chose.
   *
   * Pas à la frappe : « a », « ad », « adè » ne sont pas des recherches, ce
   * sont les étapes d'une seule. On attend qu'elle aboutisse.
   */
  useEffect(() => {
    if (!searching || results === null || results.length === 0) return;

    const minuteur = setTimeout(() => retenirRecherche(query), 1200);
    return () => clearTimeout(minuteur);
  }, [searching, results, query]);

  /**
   * Les listes qui arrivent entières se trient ici.
   *
   * La bibliothèque, elle, est triée par la base : elle est paginée, et cent
   * lignes rangées au milieu de deux mille qui ne le sont pas seraient pires
   * que pas de tri.
   */
  const shown = useMemo(() => {
    const liste = searching ? (results ?? []) : tracks;
    if (sort === null || route.kind === "library") return liste;

    const sens = sort.descending ? -1 : 1;
    const texte = (value: string | null) => (value ?? "").toLocaleLowerCase("fr");

    return [...liste].sort((gauche, droite) => {
      switch (sort.column) {
        case "title":
          return sens * texte(gauche.title).localeCompare(texte(droite.title), "fr");
        case "album":
          return sens * texte(gauche.album).localeCompare(texte(droite.album), "fr");
        case "duration":
          return sens * (gauche.durationMs - droite.durationMs);
        default:
          return sens * (gauche.addedAt - droite.addedAt);
      }
    });
  }, [searching, results, tracks, sort, route.kind]);

  /** Le morceau en cours. Dérivé tôt : les raccourcis clavier s'en servent. */
  const current = playback.state?.current ?? null;

  // ── Chargements ───────────────────────────────────────────────────────

  const reloadPlaylists = useCallback(() => {
    void ipc.listPlaylists().then(setPlaylists).catch(() => undefined);
  }, []);

  // Contrôle de présence des fichiers avant tout : le SSD a pu être débranché
  // depuis la dernière session.
  //
  // Une seule fois, au démarrage : c'est un parcours du disque entier, hors de
  // proportion avec un simple rafraîchissement d'écran.
  useEffect(() => {
    void ipc
      .refreshAvailability()
      .catch((cause: unknown) => setError(String(cause)));
  }, []);

  /**
   * Favoris et playlists, relus à chaque révision.
   *
   * # Le défaut que ça corrige
   *
   * Ils n'étaient chargés qu'au démarrage, puis tenus à jour « à la main » :
   * cocher un cœur mettait la liste à jour localement, sans jamais la
   * redemander. Tant que l'interface était seule à écrire, cela tenait.
   *
   * La synchronisation a cassé cette hypothèse. Quand l'autre appareil se
   * connecte, c'est **lui** qui écrit dans notre base : les favoris arrivaient
   * bien, et n'apparaissaient nulle part. On voyait exactement ce qu'on
   * verrait si la synchronisation ne marchait pas — d'où « liké sur le
   * téléphone, ça ne passe pas vers le Mac », alors que si, ça passait.
   */
  useEffect(() => {
    void ipc
      .lovedTracks()
      .then((lovedTracks) => setLoved(new Set(lovedTracks.map((track) => track.id))))
      .catch(() => undefined);

    reloadPlaylists();
  }, [revision, reloadPlaylists]);

  // L'autre appareil vient d'écrire chez nous : tout est à relire.
  useEffect(() => {
    const abonnement = ipc.onSyncApplied(() => bump());

    return () => {
      void abonnement.then((arreter) => arreter());
    };
  }, [bump]);

  /**
   * Les compteurs de l'en-tête, relus à chaque changement.
   *
   * Ils étaient chargés une fois pour toutes au démarrage : après une reprise
   * des fichiers écartés, l'en-tête annonçait encore « 153 hors ligne » alors
   * que cent cinquante venaient d'être réparés. Un chiffre faux est pire qu'un
   * chiffre absent — il fait douter du travail qui vient d'être fait.
   *
   * Ce sont trois `COUNT` sur une base indexée : les relire coûte moins cher
   * que de se demander s'ils sont encore justes.
   */
  useEffect(() => {
    void ipc.libraryCounts().then(setCounts).catch(() => undefined);
  }, [revision]);

  /**
   * Page courante de la bibliothèque, à partir de zéro.
   *
   * Seule la bibliothèque pagine : une playlist, un artiste ou une catégorie
   * arrivent entiers, leur taille étant bornée par nature.
   */
  const [page, setPage] = useState(0);

  /** Le conteneur qui défile, pour le ramener en haut au changement de page. */
  const scroller = useRef<HTMLElement | null>(null);


  /**
   * Disposition étroite : onglets en bas, pas de barre latérale ni de panneau.
   *
   * La bascule tient à la **largeur**, pas au système : une fenêtre de bureau
   * rétrécie a le même problème qu'un téléphone, et la même réponse lui va.
   */
  const mobile = useIsMobile();

  /**
   * Le glissement entre onglets, et ce qu'on en voit.
   *
   * `ecart` est le déplacement du doigt pendant le geste ; `sens` dit d'où la
   * nouvelle page doit arriver une fois le geste accompli. Sans lui, elle
   * apparaissait simplement à la place de l'ancienne — le mouvement portait
   * une direction, et l'affichage la jetait.
   */
  /**
   * La recherche est-elle chez elle ?
   *
   * Sur téléphone, elle appartient à l'onglet Bibliothèque : partir vers un
   * artiste, un album ou l'écran de lecture doit montrer cette page-là, pas
   * des résultats. Sur un bureau, elle occupe la zone principale quelle que
   * soit la route — le lecteur y a son propre panneau.
   */
  const recherchePlacee = !mobile || route.kind === "library";

  const [ecart, setEcart] = useState(0);

  // # Pourquoi la direction est attachée à une destination
  //
  // Elle ne vaut que pour **la** page que ce geste-là a appelée. Gardée
  // seule, elle survivrait au glissement : ouvrir ensuite un artiste depuis
  // une ligne le ferait entrer par le côté, comme si on avait glissé — un
  // mouvement qui raconterait quelque chose qui n'a pas eu lieu.
  const [sens, setSens] = useState<{ cle: string; sens: Sens } | null>(null);

  // Le geste ne vaut que sur les quatre racines, et jamais pendant une
  // recherche : le résultat affiché n'appartient à aucun onglet.
  const glissement = useSwipeOnglets(
    route,
    (destination, direction) => {
      setSens({ cle: routeKey(destination), sens: direction });
      navigate(destination);
    },
    mobile && !searching,
    setEcart,
  );

  const [filtre, setFiltre] = useState<FiltreRecherche>("titres");
  const regroupements = useRegroupements(searching ? (results ?? []) : []);

  // Une nouvelle requête ne rend pas forcément des albums : rester sur un
  // filtre vide afficherait une page blanche pour des résultats qui existent.
  // Retomber sur les titres vaut mieux que ne rien montrer.
  const filtreVide =
    (filtre === "artistes" && regroupements.artistes.length === 0) ||
    (filtre === "albums" && regroupements.albums.length === 0);
  const filtreActif: FiltreRecherche = filtreVide ? "titres" : filtre;

  /** Sur mobile, la recherche est un mode et non une page. */
  const [searchOpen, setSearchOpen] = useState(false);

  /**
   * Le geste « retour » d'Android remonte l'historique.
   *
   * # Ce qu'il faisait, et pourquoi c'était faux
   *
   * Le geste sortait de l'application, d'un coup, depuis n'importe où. Sur
   * Android, il signifie « reviens en arrière » — pas « quitte ». Depuis une
   * page de paroles, on se retrouvait sur son bureau sans comprendre.
   *
   * # Comment on l'intercepte sans plugin
   *
   * L'historique du navigateur. Chaque destination y pousse une entrée, et le
   * geste déclenche `popstate` : on recule alors d'un cran dans notre propre
   * pile. Quand il n'y a plus rien derrière, on laisse passer — et c'est bien
   * l'application qui se ferme, cette fois au bon moment.
   */
  useEffect(() => {
    if (!mobile) return;

    const reculer = () => {
      // Une recherche ouverte se ferme avant tout le reste : c'est la
      // surimpression la plus proche de l'utilisateur.
      if (searchOpen || query !== "") {
        setQuery("");
        setSearchOpen(false);
        window.history.pushState(null, "");
        return;
      }

      setCursor((position) => {
        if (position <= 0) return position;
        window.history.pushState(null, "");
        return position - 1;
      });
    };

    window.addEventListener("popstate", reculer);
    return () => window.removeEventListener("popstate", reculer);
  }, [mobile, searchOpen, query]);

  /**
   * Une entrée d'historique par destination.
   *
   * Sans elle, il n'y a rien à dépiler : le premier geste sortirait de
   * l'application, exactement ce qu'on veut éviter.
   */
  useEffect(() => {
    if (!mobile) return;
    window.history.pushState(null, "");
  }, [mobile, cursor, searchOpen]);


  /**
   * La complétion en ligne est-elle proposée ?
   *
   * Relue à chaque rechargement : l'éteindre depuis les réglages doit faire
   * disparaître les bandeaux sans redémarrage.
   */
  const [onlineCompletion, setOnlineCompletion] = useState(true);
  const [autoIdentification, setAutoIdentification] = useState(true);

  useEffect(() => {
    void ipc
      .preferences()
      .then((preferences) => {
        setOnlineCompletion(preferences.onlineCompletion);
        setAutoIdentification(preferences.autoIdentification);
      })
      .catch(() => undefined);
  }, [revision]);

  // Contenu de la page courante. Une playlist générée fait exception : son
  // ordre vient du moteur et ne se recharge pas depuis la base.
  useEffect(() => {
    if (
      route.kind === "stats"
      || route.kind === "generated"
      || route.kind === "home"
      || route.kind === "artists"
      || route.kind === "lyrics"
      || route.kind === "playing"
      || route.kind === "sync"
      || route.kind === "shortcuts"
      || route.kind === "settings"
      || route.kind === "more"
      || route.kind === "albums"
      || route.kind === "playlists"
    ) {
      return;
    }

    let active = true;
    const load = (): Promise<TrackSummary[]> => {
      switch (route.kind) {
        case "loved":
          return ipc.lovedTracks();
        case "playlist":
          return ipc.playlistTracks(route.id);
        case "category":
          return ipc.categoryTracks(route.key);
        case "artist":
          return ipc.artistTracks(route.id);
        case "album":
          return ipc.albumTracks(route.id);
        default:
          return ipc.listTracks(PAGE_SIZE, page * PAGE_SIZE, sort ?? undefined);
      }
    };

    void load()
      .then((loaded) => {
        if (!active) return;
        setTracks(loaded);

        // Les favoris se rafraîchissent au passage : chaque liste porte déjà
        // l'information, autant s'en servir plutôt que de la redemander.
        setLoved((previous) => {
          const next = new Set(previous);
          for (const track of loaded) {
            if (track.isLoved) next.add(track.id);
            else next.delete(track.id);
          }
          return next;
        });
      })
      .catch((cause: unknown) => {
        if (active) setError(String(cause));
      });

    return () => {
      active = false;
    };
  }, [route, revision, page, sort]);

  // Changer de destination remet à la première page : rester à la page 12
  // d'une autre liste n'aurait aucun sens.
  useEffect(() => {
    setPage(0);
  }, [route]);

  // Et l'on remonte en haut : rester au milieu d'une liste qu'on vient de
  // remplacer désoriente.
  useEffect(() => {
    scroller.current?.scrollTo({ top: 0 });
  }, [page, route]);

  // Recherche différée.
  useEffect(() => {
    const trimmed = query.trim();
    if (trimmed === "") {
      setResults(null);
      return;
    }

    const timer = setTimeout(() => {
      void ipc
        .searchTracks(trimmed)
        .then(setResults)
        .catch((cause: unknown) => setError(String(cause)));
    }, SEARCH_DEBOUNCE_MS);

    return () => clearTimeout(timer);
  }, [query]);

  // Progression des imports.
  useEffect(() => {
    let stop: (() => void) | null = null;
    void ipc.onScanProgress(setProgress).then((unlisten) => {
      stop = unlisten;
    });
    return () => stop?.();
  }, []);

  /**
   * Raccourcis clavier : espace pour lecture/pause, flèches pour se déplacer.
   *
   * # L'accélération au maintien
   *
   * Un saut fixe de 5 s par répétition impose une trentaine d'appuis pour
   * traverser un morceau. Le pas grandit donc avec la durée du maintien, comme
   * sur une télécommande : les premières répétitions restent fines pour viser
   * un passage précis, les suivantes couvrent du terrain.
   *
   * Le compteur est remis à zéro au relâchement — sans quoi deux appuis
   * successifs et distincts s'additionneraient, et la seconde flèche
   * traverserait le morceau.
   */
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const tag = target?.tagName ?? "";
      if (tag === "INPUT" || tag === "TEXTAREA" || target?.isContentEditable === true) {
        return;
      }

      const state = playback.state;

      if (event.code === "Escape") {
        setQuery("");
        return;
      }

      // ⌘ est réservé à la navigation : tout le reste tient à une touche, sans
      // quoi on ne s'en sert jamais.
      if (event.metaKey) {
        if (event.code === "KeyF") {
          event.preventDefault();
          document.querySelector<HTMLInputElement>('input[type="search"]')?.focus();
          return;
        }
        if (event.key === "?") {
          event.preventDefault();
          navigate({ kind: "shortcuts" });
          return;
        }
        if (event.code === "ArrowRight") {
          event.preventDefault();
          void playback.next();
          return;
        }
        if (event.code === "ArrowLeft") {
          event.preventDefault();
          void playback.previous();
          return;
        }
        return;
      }

      switch (event.code) {
        case "Space":
          event.preventDefault();
          void playback.toggle();
          return;

        case "KeyL":
          if (current !== null) void toggleLoved(current.trackId);
          return;

        case "KeyS":
          if (state !== null) void playback.toggleShuffle(!state.shuffle);
          return;

        case "KeyR":
          if (state !== null) void playback.cycleRepeat(state.repeat);
          return;

        case "KeyM":
          if (state !== null) {
            void playback.setVolume(state.volume === 0 ? 0.8 : 0);
          }
          return;

        case "ArrowUp":
        case "ArrowDown": {
          if (state === null) return;
          event.preventDefault();
          const step = event.code === "ArrowUp" ? 0.05 : -0.05;
          void playback.setVolume(Math.min(1, Math.max(0, state.volume + step)));
          return;
        }

        default:
          break;
      }

      const direction =
        event.code === "ArrowRight" ? 1 : event.code === "ArrowLeft" ? -1 : 0;
      if (direction === 0) return;

      event.preventDefault();

      if (state?.current == null) return;

      const step = seekStep(repeats.current);
      repeats.current += 1;

      const duration = state.durationMs || state.current.durationMs;
      const target_ms = Math.min(
        Math.max(0, state.positionMs + direction * step),
        Math.max(0, duration - 1000),
      );

      void playback.seek(target_ms);
    };

    const onKeyUp = (event: KeyboardEvent) => {
      if (event.code === "ArrowRight" || event.code === "ArrowLeft") {
        repeats.current = 0;
      }
    };

    window.addEventListener("keydown", onKey);
    window.addEventListener("keyup", onKeyUp);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("keyup", onKeyUp);
    };
  }, [playback, current, navigate]);

  // ── Actions ───────────────────────────────────────────────────────────

  /**
   * Ouvre les paroles en pleine largeur.
   *
   * Le panneau latéral bascule sur la file : afficher les mêmes paroles à deux
   * tailles côte à côte ne dit rien de plus, et gaspille la seule colonne qui
   * pouvait montrer autre chose. « À suivre » y est plus utile — on lit en
   * écoutant, et la question qui vient ensuite est celle du morceau suivant.
   */
  function openLyrics() {
    setPanel("queue");
    navigate({ kind: "lyrics" });
  }

  function playFrom(index: number) {
    void playback.play(
      shown.map((track) => track.id),
      index,
    );
  }

  /**
   * Lance toute la liste affichée.
   *
   * Le mode aléatoire est réglé **avant** de charger la file, et les deux appels
   * sont enchaînés : lancés en parallèle, ils retournent chacun un instantané,
   * et c'est le dernier arrivé qui l'emporterait — parfois celui d'avant la
   * lecture.
   */
  /**
   * Ajoute toute la liste affichée à la file d'attente.
   *
   * # Pourquoi la liste affichée, et non la route
   *
   * C'est déjà ce que fait « Lire » : le bouton agit sur ce qu'on a sous les
   * yeux — une playlist, un album, un artiste, un résultat de recherche.
   * Interroger la route pour reconstituer la même chose ferait un second
   * chemin, qui finirait par ne plus dire la même chose que le premier.
   */
  function enqueueAll() {
    if (shown.length === 0) return;

    void playback.enqueue(shown.map((track) => track.id));

    // Une file s'allonge sans que rien ne bouge à l'écran : sans un mot, on ne
    // sait pas si le bouton a été entendu.
    const nombre = shown.length;
    const message = `${nombre} morceau${nombre > 1 ? "x" : ""} ajouté${
      nombre > 1 ? "s" : ""
    } à la file`;

    setSyncNote(message);
    setTimeout(
      () => setSyncNote((actuel) => (actuel === message ? null : actuel)),
      4000,
    );
  }

  async function playAll(shuffle: boolean) {
    if (shown.length === 0) return;

    await playback.toggleShuffle(shuffle);
    await playback.play(
      shown.map((track) => track.id),
      0,
    );
  }

  /**
   * Ouvre la page de l'artiste principal d'un morceau.
   *
   * La table n'a que le nom de l'artiste, pas son identifiant : le renvoyer sur
   * chacune des trois cents lignes pour servir un clic occasionnel serait payer
   * cher un cas rare. On le retrouve dans la liste des artistes au moment du
   * clic.
   */
  async function openArtistOf(trackId: number) {
    const track = shown.find((entry) => entry.id === trackId);
    if (track?.artist == null) return;

    try {
      const all = await ipc.listArtists();
      const found = all.find((artist) => artist.name === track.artist);
      if (found === undefined) return;

      navigate({
        kind: "artist",
        id: found.id,
        name: found.name,
        coverHash: found.coverHash,
      });
    } catch (cause) {
      setError(String(cause));
    }
  }

  async function toggleLoved(trackId: number) {
    try {
      const nowLoved = await ipc.toggleLoved(trackId);
      setLoved((previous) => {
        const next = new Set(previous);
        if (nowLoved) next.add(trackId);
        else next.delete(trackId);
        return next;
      });

      // La page des favoris se retire un morceau sous les pieds : il faut la
      // recharger pour que la ligne disparaisse.
      if (route.kind === "loved") bump();
    } catch (cause) {
      setError(String(cause));
    }
  }

  function addToPlaylist(playlistId: number, trackId: number) {
    void ipc
      .addToPlaylist(playlistId, [trackId])
      .then(() => {
        reloadPlaylists();
        bump();
      })
      .catch((cause: unknown) => setError(String(cause)));
  }

  function createPlaylist(name: string) {
    void ipc
      .createPlaylist(name)
      .then((id) => {
        reloadPlaylists();
        navigate({ kind: "playlist", id, name });
      })
      .catch((cause: unknown) => setError(String(cause)));
  }

  function showGenerated(playlist: GeneratedPlaylist) {
    setError(null);
    setQuery("");
    setGenerated(playlist);

    // La lecture a démarré côté backend : on s'assure que la barre apparaisse
    // sans attendre le prochain changement de morceau.
    playback.refresh();

    const ids = playlist.tracks.map((track) => track.trackId);
    void ipc
      // Par identifiants, et non en piochant dans une page de bibliothèque :
      // tout ce qui se serait trouvé au-delà de la page disparaissait de la
      // playlist sans le moindre message.
      .tracksByIds(ids)
      .then((found) => {
        setTracks(found);
        navigate({ kind: "generated" });
      })
      .catch((cause: unknown) => setError(String(cause)));
  }

  function startRadio(trackId: number) {
    void ipc
      .startRadio(trackId)
      .then(showGenerated)
      .catch((cause: unknown) => setError(String(cause)));
  }

  async function importFolder() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Choisir le dossier à importer",
    });
    if (typeof selected !== "string") return;

    setError(null);
    setSummary(null);
    setProgress({ processed: 0, total: 0, imported: 0, duplicates: 0, failed: 0, current: "" });

    try {
      setSummary(await ipc.importFolder(selected));
      setCounts(await ipc.libraryCounts());
      bump();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setProgress(null);
    }
  }

  // ── Rendu ─────────────────────────────────────────────────────────────

  /**
   * Les vingt prochains morceaux de la file, en entier.
   *
   * # Pourquoi une requête et non ce que la file contient déjà
   *
   * La file porte le strict nécessaire pour jouer : un titre, un artiste, un
   * chemin. Pas de pastille de paroles, pas de favori, pas d'album cliquable —
   * et ce sont précisément les colonnes qui font une ligne de bibliothèque.
   * Vingt identifiants suffisent à les récupérer.
   *
   * # Pourquoi vingt
   *
   * La file en contient des milliers. Les afficher tous ferait de cet écran
   * une seconde bibliothèque, sans tri ni recherche — moins bonne que la
   * vraie, qui est à un onglet de là.
   */
  const departFile = (playback.state?.queueIndex ?? -1) + 1;

  /**
   * Combien de morceaux à venir sont affichés.
   *
   * Vingt pour commencer, puis vingt de plus à chaque demande. Le décompte
   * seul — « et 56 autres dans la file » — énonçait un fait sans rien en
   * faire : on apprenait qu'il y avait autre chose, et qu'on n'y avait pas
   * accès.
   */
  const [suiteVisible, setSuiteVisible] = useState(20);

  // Un nouveau morceau, une nouvelle file : la fenêtre repart de vingt.
  useEffect(() => {
    setSuiteVisible(20);
  }, [playback.state?.current?.trackId]);

  const idsSuite = useMemo(
    () =>
      (playback.state?.queue ?? [])
        .slice(departFile, departFile + suiteVisible)
        .map((item) => item.trackId),
    [playback.state?.queue, departFile, suiteVisible],
  );

  const [suite, setSuite] = useState<TrackSummary[]>([]);

  useEffect(() => {
    if (idsSuite.length === 0) {
      setSuite([]);
      return;
    }

    let vivant = true;
    void ipc
      .tracksByIds(idsSuite)
      .then((fiches) => {
        if (!vivant) return;

        // La requête rend les morceaux dans l'ordre de la base, et un même
        // morceau peut figurer deux fois dans la file : c'est la file qui
        // décide de l'ordre, jamais la base.
        const parId = new Map(fiches.map((fiche) => [fiche.id, fiche]));
        setSuite(
          idsSuite
            .map((id) => parId.get(id))
            .filter((fiche): fiche is TrackSummary => fiche !== undefined),
        );
      })
      .catch(() => undefined);

    return () => {
      vivant = false;
    };
  }, [idsSuite]);

  /**
   * Les gestes d'une ligne, définis une seule fois.
   *
   * La file d'attente affiche le **même** tableau que la bibliothèque : même
   * numéro, même pastille de paroles, mêmes trois points, mêmes bords. Deux
   * jeux de gestes séparés garantiraient qu'un menu gagne une entrée d'un côté
   * et pas de l'autre.
   */
  const gestes = {
    onRadio: startRadio,
    onToggleLoved: (id: number) => void toggleLoved(id),
    onEnqueue: (id: number) => {
      void playback.enqueue([id]);
    },
    onOpenArtist: (id: number) => void openArtistOf(id),
    onCorrect: setCorrecting,
    onMatch: setMatching,
    onSyncLyrics: syncLyrics,
    onOpenPlaying: () => navigate({ kind: "playing" }),
    onOpenAlbum: (track: TrackSummary) => {
      if (track.albumId !== null) {
        navigate({
          kind: "album",
          id: track.albumId,
          name: track.album ?? "Album",
          artist: track.artist,
        });
      }
    },
    onRemove: (id: number) => {
      void ipc
        .removeTrack(id)
        .then(() => {
          setCounts(null);
          void ipc.libraryCounts().then(setCounts).catch(() => undefined);
          bump();
        })
        .catch((cause: unknown) => setError(String(cause)));
    },
    loved,
    playlists,
    onAddToPlaylist: addToPlaylist,
  };

  const table = (
    <TrackTable
      tracks={shown}
      currentTrackId={playback.state?.current?.trackId ?? null}
      isPlaying={playback.state?.isPlaying ?? false}
      onPlay={playFrom}
      {...gestes}
      {...(sort === null ? {} : { sort })}
      onSort={toggleSort}
      {...(route.kind === "playlist" && !searching
        ? {
            onRemoveAt: (position: number) => {
              void ipc
                .removeFromPlaylist(route.id, position)
                .then(() => {
                  reloadPlaylists();
                  bump();
                })
                .catch((cause: unknown) => setError(String(cause)));
            },
          }
        : {})}
      {...(route.kind === "generated" && generated !== null
        ? {
            reasons: new Map(
              generated.tracks.map((track) => [track.trackId, track.reason]),
            ),
          }
        : {})}
      emptyMessage={
        searching
          ? `Aucun résultat pour « ${query.trim()} ».`
          : "Rien à afficher ici pour l'instant."
      }
    />
  );

  /** « À suivre », rendu avec le tableau de la bibliothèque. */
  const restantsFile =
    (playback.state?.queue.length ?? 0) - departFile - suite.length;

  const tableauFile =
    suite.length === 0 ? null : (
      <section className="mt-9">
        <h2 className="px-3 text-[11px] font-semibold uppercase tracking-[0.14em] text-ink-faint">
          À suivre
        </h2>

        <TrackTable
          tracks={suite}
          currentTrackId={null}
          isPlaying={false}
          onPlay={(index) => void playback.jump(departFile + index)}
          {...gestes}
        />

        {restantsFile > 0 && (
          <div className="px-3 pb-6">
            <button
              type="button"
              onClick={() => setSuiteVisible((visible) => visible + 20)}
              className="pression w-full rounded-lg border border-line px-4 py-2.5 text-[13px] text-ink-muted transition-colors hover:bg-elevated hover:text-ink"
            >
              Montrer plus
              <span className="numerals ml-1.5 text-ink-faint">
                ({restantsFile} restant{restantsFile > 1 ? "s" : ""})
              </span>
            </button>
          </div>
        )}
      </section>
    );

  // La pagination ne concerne que la bibliothèque : ailleurs, tout est déjà
  // là. Elle disparaît aussi pendant une recherche, dont les résultats sont
  // rendus entiers.
  const paged =
    route.kind === "library" && !searching ? (
      <>
        {table}
        <Pager
          page={page}
          pageCount={Math.max(1, Math.ceil((counts?.tracks ?? 0) / PAGE_SIZE))}
          onChange={setPage}
        />
      </>
    ) : (
      table
    );

  return (
    <div
      className="flex h-full flex-col bg-base"
      // L'application se dessine sous la barre d'état — c'est ce qui permet à
      // la couleur de fond de la remplir plutôt que de laisser une bande
      // noire. Encore faut-il que le contenu, lui, commence en dessous.
      style={mobile ? { paddingTop: "env(safe-area-inset-top)" } : undefined}
    >
      {/* Les feux de circulation de macOS vivent ici : la fenêtre n'a pas de
          barre de titre, il faut donc lui réserver sa bande de glissement.
          Un téléphone n'a ni fenêtre ni feux — la bande n'y serait qu'un vide
          de neuf pixels en haut de l'écran. */}
      {!mobile && <div className="drag-region h-9 shrink-0" />}

      {/* Les bandeaux flottent **au-dessus** de la page.
          Insérés dans le flux, ils poussaient tout le contenu vers le bas à
          chaque import, et disparaissaient dès qu'on changeait de page — au
          moment précis où l'on voulait savoir où en était le transfert. */}
      <Banners
        progress={progress}
        summary={summary}
        error={error ?? playback.error}
        mobile={mobile}
        onDismissSummary={() => setSummary(null)}
        onDismissError={() => {
          setError(null);
          playback.dismissError();
        }}
      />

      <div className={`flex min-h-0 flex-1 ${mobile ? "" : "gap-2 px-2"}`}>
        {!mobile && (
          <Sidebar
            route={route}
            onNavigate={navigate}
            playlists={playlists}
            onCreatePlaylist={createPlaylist}
          />
        )}

        <main
          ref={scroller}
          {...glissement}
          // # Pourquoi l'horizontale est fermée
          //
          // La page se déplace latéralement pendant un glissement entre
          // onglets. Ce déplacement déborde du conteneur, et le conteneur fait
          // ce qu'on lui a appris à faire : il propose une barre de défilement
          // horizontale. Une barre grise apparaissait donc en bas de l'écran à
          // chaque geste, avec son curseur — un rouage interne rendu visible
          // par une animation.
          className={`min-h-0 min-w-0 flex-1 overflow-y-auto overflow-x-hidden bg-surface ${
            mobile ? "" : "rounded-xl"
          }`}
        >
          {mobile ? (
            <MobileSearch
              open={searchOpen && recherchePlacee}
              query={query}
              onQuery={setQuery}
              onClose={() => {
                setQuery("");
                setSearchOpen(false);
              }}
            />
          ) : (
          <TopBar
            query={query}
            onQuery={setQuery}
            onEscape={() => setQuery("")}
            canGoBack={cursor > 0}
            canGoForward={cursor < stack.length - 1}
            onBack={() => setCursor((value) => Math.max(0, value - 1))}
            onForward={() => setCursor((value) => Math.min(stack.length - 1, value + 1))}
            importing={importing}
            onImport={() => void importFolder()}
            panelOpen={panel !== "closed"}
            onTogglePanel={() => setPanel((value) => (value === "closed" ? "lyrics" : "closed"))}
          />
          )}

          {/* # Deux enveloppes, deux rôles
              Celle du dehors suit le doigt pendant le geste : elle ne change
              jamais de clé, donc rien ne se remonte et le mouvement est
              continu. `transform: none` quand rien ne bouge — et non
              `translateX(0)` — pour ne pas laisser derrière soi un bloc
              conteneur qui déplacerait les surcouches en plein écran.

              Celle du dedans change de clé à chaque destination : c'est ce
              qui fait rejouer l'animation d'entrée, React remontant alors le
              sous-arbre. */}
          <div
            className={ecart === 0 ? "glissement-retour" : undefined}
            style={ecart === 0 ? { transform: "none" } : { transform: `translateX(${ecart}px)` }}
          >
          <div
            key={routeKey(route)}
            className={
              sens?.cle !== routeKey(route)
                ? "page-entree"
                : sens.sens === "droite"
                  ? "page-depuis-droite"
                  : "page-depuis-gauche"
            }
          >
          {/* # Pourquoi la recherche ne recouvre plus que sa page
              Elle s'affichait dès qu'une requête était écrite, **quelle que
              soit la destination**. Appuyer sur le lecteur depuis les
              résultats changeait donc bien de route — et ne montrait rien : la
              recherche restait par-dessus. Il fallait l'annuler à la main pour
              voir l'écran de lecture, ce qui se raconte exactement comme « la
              lecture ne marche pas depuis la recherche ».

              Sur un bureau, la recherche remplace la zone principale quoi
              qu'il arrive : le lecteur y a son propre panneau, elle ne le
              cache pas. */}
          {mobile && searchOpen && recherchePlacee && !searching ? (
            // Sous le champ vide, la bibliothèque entière n'a rien à faire :
            // elle est à un onglet de là, et l'afficher ici laisse croire que
            // ce sont des résultats.
            <RecherchesRecentes onChoose={setQuery} />
          ) : searching && recherchePlacee ? (
            <>
              <SearchHeader query={query} count={shown.length} />

              <BarreFiltres
                actif={filtreActif}
                onChange={setFiltre}
                compte={{
                  titres: shown.length,
                  artistes: regroupements.artistes.length,
                  albums: regroupements.albums.length,
                }}
              />

              {filtreActif === "titres" ? (
                paged
              ) : (
                <ListeRegroupements
                  entrees={
                    filtreActif === "artistes"
                      ? regroupements.artistes
                      : regroupements.albums
                  }
                  rond={filtreActif === "artistes"}
                  onOuvrir={(trackId) =>
                    void (filtreActif === "artistes"
                      ? openArtistOf(trackId)
                      : openAlbumOf(trackId))
                  }
                />
              )}
            </>
          ) : (
            <Page
              route={route}
              tracks={tracks}
              counts={counts}
              libraryRoot={libraryRoot}
              playlists={playlists}
              generated={generated}
              importing={importing}
              onPlayAll={(shuffle) => void playAll(shuffle)}
              onEnqueueAll={enqueueAll}
              onPlayTracks={(list, index) =>
                void playback.play(list.map((track) => track.id), index)
              }
              onOpenCategory={(key, title) =>
                navigate({ kind: "category", key, name: title })
              }
              onOpenArtist={(artist) =>
                navigate({
                  kind: "artist",
                  id: artist.id,
                  name: artist.name,
                  coverHash: artist.coverHash,
                })
              }
              onPlayOne={(id) => void playback.play([id], 0)}
              isPlaying={playback.state?.isPlaying ?? false}
              currentTrack={current}
              positionMs={playback.state?.positionMs ?? 0}
              onSeek={(position) => void playback.seek(position)}
              onReload={bump}
              onNavigate={navigate}
              onCreatePlaylist={() => createPlaylist("Nouvelle playlist")}
              playback={
                playback.state === null || current === null
                  ? null
                  : {
                      state: playback.state,
                      isLoved: loved.has(current.trackId),
                      onToggle: () => void playback.toggle(),
                      onNext: () => void playback.next(),
                      onPrevious: () => void playback.previous(),
                      onSeek: (position: number) => void playback.seek(position),
                      onToggleLoved: () => void toggleLoved(current.trackId),
                      onOpenLyrics: () => openLyrics(),
                      onOpenArtist: () => void openArtistOf(current.trackId),
                      onOpenAlbum: () => void openAlbumOf(current.trackId),
                      onShuffle: (shuffle: boolean) => void playback.toggleShuffle(shuffle),
                      onRepeat: () =>
                        void playback.cycleRepeat(playback.state?.repeat ?? "off"),
                      onClose: () => setCursor((position) => Math.max(0, position - 1)),
                      fileDAttente: tableauFile,
                    }
              }
              onlineCompletion={onlineCompletion}
              autoIdentification={autoIdentification}
              onGenerated={showGenerated}
              onError={setError}
              onRenamePlaylist={(id, name) => {
                void ipc
                  .renamePlaylist(id, name)
                  .then(() => {
                    reloadPlaylists();
                    setStack((previous) =>
                      previous.map((entry) =>
                        entry.kind === "playlist" && entry.id === id
                          ? { ...entry, name }
                          : entry,
                      ),
                    );
                  })
                  .catch((cause: unknown) => setError(String(cause)));
              }}
              onPickPlaylistCover={async (id) => {
                const chosen = await open({
                  multiple: false,
                  title: "Choisir une image de playlist",
                  filters: [{ name: "Images", extensions: ["jpg", "jpeg", "png", "webp"] }],
                });
                if (typeof chosen !== "string") return;

                try {
                  await ipc.setPlaylistCover(id, chosen);
                  reloadPlaylists();
                } catch (cause) {
                  setError(String(cause));
                }
              }}
              onPlaylistDescription={(id, text) => {
                void ipc
                  .setPlaylistDescription(id, text)
                  .then(reloadPlaylists)
                  .catch((cause: unknown) => setError(String(cause)));
              }}
              onDeletePlaylist={(id) => {
                void ipc
                  .deletePlaylist(id)
                  .then(() => {
                    reloadPlaylists();
                    setStack([{ kind: "library" }]);
                    setCursor(0);
                  })
                  .catch((cause: unknown) => setError(String(cause)));
              }}
            >
              {paged}
            </Page>
          )}
          </div>
          </div>
        </main>

        {!mobile && panel !== "closed" && current !== null && (
          <NowPlayingPanel
            track={current}
            positionMs={playback.state?.positionMs ?? 0}
            isLoved={loved.has(current.trackId)}
            tab={panel}
            queue={playback.state?.queue ?? []}
            queueIndex={playback.state?.queueIndex ?? null}
            onTab={setPanel}
            onClose={() => setPanel("closed")}
            onToggleLoved={() => void toggleLoved(current.trackId)}
            onSeek={(position) => void playback.seek(position)}
            onJump={(index) => void playback.jump(index)}
            onRadio={startRadio}
            onExpandLyrics={() => openLyrics()}
            lyricsExpanded={route.kind === "lyrics"}
          />
        )}
      </div>

      {correcting !== null && (
        <CorrectDialog
          track={correcting}
          onClose={() => setCorrecting(null)}
          onCorrected={bump}
          onSearchElsewhere={() => {
            setMatching(correcting);
            setCorrecting(null);
          }}
        />
      )}

      {(syncing !== null || syncNote !== null) && (
        <div className="fixed bottom-28 left-1/2 z-40 -translate-x-1/2 rounded-full bg-surface px-5 py-2.5 text-[13px] text-ink shadow-2xl shadow-black/50">
          {syncing !== null ? (
            <span className="flex items-center gap-2.5">
              <span className="animate-pulse text-accent">
                <Icon name="sparkle" size={15} />
              </span>
              Écoute de « {syncing.title} »…
            </span>
          ) : (
            <span className="flex items-center gap-3">
              {syncNote}
              <button
                type="button"
                aria-label="Fermer"
                onClick={() => setSyncNote(null)}
                className="text-ink-faint transition-colors hover:text-ink"
              >
                <Icon name="close" size={14} />
              </button>
            </span>
          )}
        </div>
      )}

      {matching !== null && (
        <MatchDialog
          track={matching}
          onClose={() => setMatching(null)}
          onApplied={bump}
        />
      )}

      {mobile && (
        <>
          {playback.state !== null && (
            <MiniPlayer
              state={playback.state}
              onToggle={() => void playback.toggle()}
              onNext={() => void playback.next()}
              onOpen={() => navigate({ kind: "playing" })}
              onSeek={(position) => void playback.seek(position)}
            />
          )}

          <MobileTabs
            active={ongletActif(route, searchOpen && recherchePlacee)}
            onNavigate={(destination) => {
              setSearchOpen(false);
              setQuery("");
              navigate(destination);
            }}
            onSearch={() => {
              setSearchOpen(true);
              navigate({ kind: "library" });
            }}
          />
        </>
      )}

      {!mobile && playback.state !== null && (
        <PlayerBar
          state={playback.state}
          isLoved={current !== null && loved.has(current.trackId)}
          panel={panel}
          onToggle={() => void playback.toggle()}
          onNext={() => void playback.next()}
          onPrevious={() => void playback.previous()}
          onSeek={(position) => void playback.seek(position)}
          onVolume={(volume) => void playback.setVolume(volume)}
          onShuffle={(shuffle) => void playback.toggleShuffle(shuffle)}
          onRepeat={() => void playback.cycleRepeat(playback.state?.repeat ?? "off")}
          onToggleLoved={() => {
            if (current !== null) void toggleLoved(current.trackId);
          }}
          onOpenPanel={(tab) => {
            // Les paroles s'ouvrent en grand : c'est ce qu'on veut quand on
            // clique dessus. Le panneau latéral reste accessible depuis
            // l'onglet, pour les suivre du coin de l'œil.
            if (tab === "lyrics") {
              openLyrics();
              return;
            }
            setPanel((value) => (value === tab ? "closed" : tab));
          }}
        />
      )}
    </div>
  );
}

// ════════════════════════════════════════════════════════════════════════════
//  Pages
// ════════════════════════════════════════════════════════════════════════════

interface PageProps {
  route: Route;
  tracks: TrackSummary[];
  counts: LibraryCounts | null;
  libraryRoot: string;
  playlists: PlaylistSummary[];
  generated: GeneratedPlaylist | null;
  importing: boolean;
  onPlayAll: (shuffle: boolean) => void;
  /** Ajoute la liste affichée à la file, sans interrompre l'écoute. */
  onEnqueueAll: () => void;
  /** Lance une liste arbitraire — la rangée de reprise de l'accueil. */
  onPlayTracks: (tracks: TrackSummary[], index: number) => void;
  onOpenCategory: (key: string, title: string) => void;
  onOpenArtist: (artist: {
    id: number;
    name: string;
    coverHash: string | null;
  }) => void;
  /** Écoute un morceau seul, sans toucher à la file affichée. */
  onPlayOne: (trackId: number) => void;
  isPlaying: boolean;
  /** Morceau en cours, pour la page des paroles. */
  currentTrack: QueueItem | null;
  positionMs: number;
  onSeek: (positionMs: number) => void;
  /** Recharge la liste affichée après une correction de tags. */
  onReload: () => void;
  /** Navigation depuis la page « Plus ». */
  onNavigate: (route: Route) => void;
  onCreatePlaylist: () => void;
  /** Tout ce qu'il faut à l'écran de lecture, ou `null` si rien ne joue. */
  playback: React.ComponentProps<typeof NowPlayingView> | null;
  /** Les outils de complétion en ligne sont-ils proposés ? */
  onlineCompletion: boolean;
  /** L'identification acoustique est-elle proposée ? */
  autoIdentification: boolean;
  onGenerated: (playlist: GeneratedPlaylist) => void;
  onError: (message: string) => void;
  onRenamePlaylist: (id: number, name: string) => void;
  onPickPlaylistCover: (id: number) => Promise<void>;
  onPlaylistDescription: (id: number, description: string) => void;
  onDeletePlaylist: (id: number) => void;
  children: React.ReactNode;
}

/**
 * Aiguillage des pages.
 *
 * Toutes partagent le même en-tête et la même table : ce qui change, c'est ce
 * que la page raconte d'elle-même. Une page « Favoris » qui ne ressemblerait
 * pas à une page « Playlist » demanderait à l'utilisateur de réapprendre la
 * même chose deux fois.
 */
/**
 * Le sélecteur de collection, au-dessus des listes de morceaux.
 *
 * # Pourquoi les mêmes destinations à deux endroits
 *
 * Les favoris et les playlists vivent dans la barre latérale sur le bureau, et
 * derrière « Plus » sur téléphone. Les retrouver **au-dessus de la
 * bibliothèque** ne fait pas doublon : c'est là qu'on se trouve quand l'envie
 * vient de basculer de « tout » vers « ce que j'aime ». Une porte ne devient
 * pas inutile parce qu'il en existe une autre ailleurs.
 */
function CollectionSwitch({
  route,
  onNavigate,
}: {
  route: Route;
  onNavigate: (route: Route) => void;
}) {
  // Les quatre façons d'entrer dans sa propre collection. Elles ne changent
  // pas avec le contenu : une bibliothèque sans playlist doit quand même dire
  // que les playlists existent, sinon rien n'apprend qu'on peut en créer.
  const onglets: { cle: string; label: string; route: Route }[] = [
    { cle: "library", label: "Titres", route: { kind: "library" } },
    { cle: "loved", label: "J'aime", route: { kind: "loved" } },
    { cle: "albums", label: "Albums", route: { kind: "albums" } },
    { cle: "playlists", label: "Playlists", route: { kind: "playlists" } },
  ];

  const actif = onglets.some((onglet) => onglet.cle === routeKey(route))
    ? routeKey(route)
    : route.kind === "playlist"
      ? "playlists"
      : "";

  return (
    // Le même dégradé que l'en-tête juste en dessous : posé sur le fond de
    // base, le sélecteur formait une bande d'une autre couleur, comme collée
    // par-dessus la page.
    <div className="flex gap-2 overflow-x-auto bg-gradient-to-b from-elevated/70 to-elevated/40 px-6 pb-1 pt-4">
      {onglets.map((onglet) => (
        <button
          key={onglet.cle}
          type="button"
          aria-current={onglet.cle === actif ? "page" : undefined}
          onClick={() => onNavigate(onglet.route)}
          className={`shrink-0 rounded-full px-4 py-1.5 text-[13px] font-medium transition-colors ${
            onglet.cle === actif
              ? "bg-ink text-base"
              : "bg-elevated text-ink-muted hover:text-ink"
          }`}
        >
          {onglet.label}
        </button>
      ))}
    </div>
  );
}

function Page(props: PageProps) {
  const { route, tracks } = props;

  // Les trois pages qui listent « une collection à soi » partagent le même
  // sélecteur : passer de l'une à l'autre est un geste courant, et redescendre
  // dans un menu pour cela serait un détour.
  const collection =
    route.kind === "library"
    || route.kind === "loved"
    || route.kind === "playlist"
    || route.kind === "albums"
    || route.kind === "playlists" ? (
      <CollectionSwitch route={route} onNavigate={props.onNavigate} />
    ) : null;

  const totalMs = useMemo(
    () => tracks.reduce((sum, track) => sum + track.durationMs, 0),
    [tracks],
  );

  const meta = (
    <>
      <span className="font-semibold text-ink">
        {tracks.length} titre{tracks.length > 1 ? "s" : ""}
      </span>
      {tracks.length > 0 && (
        <>
          <span className="text-ink-faint">·</span>
          <span>{formatDurationLong(totalMs)}</span>
        </>
      )}
    </>
  );

  const play = tracks.length === 0 ? null : () => props.onPlayAll(false);
  const shuffle = tracks.length === 0 ? undefined : () => props.onPlayAll(true);
  const enfiler = tracks.length === 0 ? undefined : props.onEnqueueAll;

  if (route.kind === "stats") {
    return <WrappedView />;
  }

  if (route.kind === "home") {
    return (
      <HomeView
        onPlayTrack={props.onPlayTracks}
        onGenerated={props.onGenerated}
        onError={props.onError}
        onOpenCategory={props.onOpenCategory}
      />
    );
  }

  if (route.kind === "loved") {
    return (
      <>
        {collection}
        <PageHeader
          eyebrow="Playlist"
          title="Titres likés"
          meta={meta}
          cover={
            <div className="flex h-40 w-40 items-center justify-center bg-gradient-to-br from-accent to-accent-soft sm:h-52 sm:w-52">
              <Icon name="heartFilled" size={72} className="text-base" />
            </div>
          }
          onPlay={play}
          {...(shuffle === undefined ? {} : { onShuffle: shuffle })}
          {...(enfiler === undefined ? {} : { onEnqueue: enfiler })}
        />
        {props.children}
      </>
    );
  }

  if (route.kind === "sync") {
    return <SyncView />;
  }

  if (route.kind === "shortcuts") {
    return <ShortcutsView />;
  }

  if (route.kind === "albums") {
    return (
      <>
        {collection}
        <AlbumsView
          onOpen={(album) =>
            props.onNavigate({
              kind: "album",
              id: album.id,
              name: album.title,
              artist: album.artist,
            })
          }
        />
      </>
    );
  }

  if (route.kind === "playlists") {
    return (
      <>
        {collection}
        <PlaylistsView
          playlists={props.playlists}
          onOpen={(playlist) =>
            props.onNavigate({ kind: "playlist", id: playlist.id, name: playlist.name })
          }
          onCreate={props.onCreatePlaylist}
        />
      </>
    );
  }

  if (route.kind === "more") {
    return (
      <MoreView
        playlists={props.playlists}
        onNavigate={props.onNavigate}
        onCreatePlaylist={props.onCreatePlaylist}
      />
    );
  }

  if (route.kind === "pairing") {
    return <PairingView onSynced={props.onReload} />;
  }

  if (route.kind === "settings") {
    return <SettingsView onChanged={props.onReload} libraryRoot={props.libraryRoot} />;
  }

  if (route.kind === "playing") {
    if (props.playback === null) {
      return (
        <p className="px-6 py-20 text-center text-sm text-ink-muted">
          Lance un morceau pour le voir ici.
        </p>
      );
    }

    return <NowPlayingView {...props.playback} />;
  }

  if (route.kind === "lyrics") {
    return (
      <LyricsView
        track={props.currentTrack}
        positionMs={props.positionMs}
        onSeek={props.onSeek}
      />
    );
  }

  if (route.kind === "artists") {
    return <ArtistsView onOpen={props.onOpenArtist} />;
  }

  if (route.kind === "artist") {
    return (
      <>
        <PageHeader
          eyebrow="Artiste"
          title={route.name}
          meta={meta}
          cover={
            route.coverHash === null ? (
              <CoverTile name="artist" />
            ) : (
              <Artwork hash={route.coverHash} className="h-40 w-40 rounded-full sm:h-52 sm:w-52" />
            )
          }
          onPlay={play}
          {...(shuffle === undefined ? {} : { onShuffle: shuffle })}
          {...(enfiler === undefined ? {} : { onEnqueue: enfiler })}
        />

        <AlbumRow
          artistId={route.id}
          onOpen={(album) =>
            props.onNavigate({
              kind: "album",
              id: album.id,
              name: album.title,
              artist: album.artist,
            })
          }
        />

        {props.children}
      </>
    );
  }

  if (route.kind === "album") {
    return (
      <>
        <PageHeader
          eyebrow={route.artist ?? "Album"}
          title={route.name}
          meta={meta}
          cover={
            tracks[0]?.artworkHash == null ? (
              <CoverTile name="library" />
            ) : (
              <Artwork
                hash={tracks[0].artworkHash}
                className="h-40 w-40 rounded-lg sm:h-52 sm:w-52"
              />
            )
          }
          onPlay={play}
          {...(shuffle === undefined ? {} : { onShuffle: shuffle })}
          {...(enfiler === undefined ? {} : { onEnqueue: enfiler })}
        />
        {props.children}
      </>
    );
  }

  if (route.kind === "category") {
    return (
      <>
        <PageHeader
          eyebrow="Ambiance"
          title={route.name}
          meta={
            <>
              {meta}
              <span className="text-ink-faint">·</span>
              <span>calculé sur le son, relatif à ta bibliothèque</span>
            </>
          }
          cover={<CoverTile name="sparkle" />}
          onPlay={play}
          {...(shuffle === undefined ? {} : { onShuffle: shuffle })}
          {...(enfiler === undefined ? {} : { onEnqueue: enfiler })}
        />
        {props.children}
      </>
    );
  }

  if (route.kind === "playlist") {
    const summary = props.playlists.find((entry) => entry.id === route.id);

    return (
      <>
        <PageHeader
          eyebrow="Playlist"
          title={summary?.name ?? route.name}
          meta={meta}
          cover={
            summary?.coverHash != null ? (
              <Artwork hash={summary.coverHash} className="h-40 w-40 sm:h-52 sm:w-52" />
            ) : (
              <CoverTile name="library" />
            )
          }
          onPlay={play}
          {...(shuffle === undefined ? {} : { onShuffle: shuffle })}
          {...(enfiler === undefined ? {} : { onEnqueue: enfiler })}
          onRename={(name) => props.onRenamePlaylist(route.id, name)}
          onPickCover={() => void props.onPickPlaylistCover(route.id)}
          description={summary?.description ?? null}
          onDescription={(text) => props.onPlaylistDescription(route.id, text)}
          extra={<DeleteButton onConfirm={() => props.onDeletePlaylist(route.id)} />}
        />
        {props.children}
      </>
    );
  }

  if (route.kind === "generated" && props.generated !== null) {
    return (
      <>
        <PageHeader
          eyebrow="Générée pour toi"
          title={props.generated.title}
          meta={
            <>
              <span>{props.generated.subtitle}</span>
              <span className="text-ink-faint">·</span>
              {meta}
            </>
          }
          cover={<CoverTile name="sparkle" />}
          onPlay={play}
          {...(shuffle === undefined ? {} : { onShuffle: shuffle })}
          {...(enfiler === undefined ? {} : { onEnqueue: enfiler })}
        />
        {props.children}
      </>
    );
  }

  // ── Bibliothèque ──────────────────────────────────────────────────────
  return (
    <>
      {collection}
      <PageHeader
        eyebrow="Ta bibliothèque"
        title="Toute ta musique"
        meta={
          <>
            {props.counts !== null && (
              <>
                <span className="font-semibold text-ink">
                  {props.counts.tracks.toLocaleString("fr-FR")} morceaux
                </span>
                <span className="text-ink-faint">·</span>
                <span>{props.counts.artists.toLocaleString("fr-FR")} artistes</span>
                <span className="text-ink-faint">·</span>
                <span>{props.counts.albums.toLocaleString("fr-FR")} albums</span>
                {props.counts.unavailable > 0 && (
                  <>
                    <span className="text-ink-faint">·</span>
                    <span className="text-warn">
                      {props.counts.unavailable} hors ligne
                    </span>
                  </>
                )}
              </>
            )}
          </>
        }
        cover={<CoverTile name="library" />}
        onPlay={play}
        {...(shuffle === undefined ? {} : { onShuffle: shuffle })}
        {...(enfiler === undefined ? {} : { onEnqueue: enfiler })}
      />

      <div className="px-6 pb-2">
        <DiscoverBar
          disabled={props.importing || (props.counts?.tracks ?? 0) === 0}
          onGenerated={props.onGenerated}
          onError={props.onError}
        />

        {/* Six bandeaux empilés faisaient un écran de défilement avant le
            premier morceau. Ils vivent sous une seule porte, qui annonce
            d'avance s'il y a quelque chose à faire. */}
        <div className="mt-3">
          <MaintenanceCard pending={props.counts?.unavailable ?? 0}>
          {props.autoIdentification && (
            <>
              <IdentifyPanel />
              <SuspectPanel onRestored={props.onReload} />
            </>
          )}

          <DuplicatePanel
            onRemoved={props.onReload}
            onPlay={props.onPlayOne}
            currentTrackId={props.currentTrack?.trackId ?? null}
            isPlaying={props.isPlaying}
          />

          {props.onlineCompletion && (
            <>
              <LyricsBar onChanged={props.onReload} />
              <ArtworkBar />
              <AlbumBar />
            </>
          )}

          {/* L'écoute ne dépend d'aucun service : le modèle tourne sur la
              machine. Elle reste donc proposée même complétion éteinte. */}
          <ListenBar />

          <OfflineBar count={props.counts?.unavailable ?? 0} onChanged={props.onReload} />
          </MaintenanceCard>
        </div>

      </div>

      {props.children}
    </>
  );
}

/**
 * Suppression d'une playlist, en deux temps.
 *
 * Pas de boîte de dialogue : un second clic sur un bouton devenu rouge suffit
 * à écarter le geste accidentel, et se rétracte tout seul si on part ailleurs.
 */
function DeleteButton({ onConfirm }: { onConfirm: () => void }) {
  const [armed, setArmed] = useState(false);

  useEffect(() => {
    if (!armed) return;
    const timer = setTimeout(() => setArmed(false), 4000);
    return () => clearTimeout(timer);
  }, [armed]);

  if (!armed) {
    return (
      <HeaderAction name="trash" label="Supprimer la playlist" onClick={() => setArmed(true)} />
    );
  }

  return (
    <button
      type="button"
      onClick={onConfirm}
      className="rounded-full bg-danger/15 px-4 py-2 text-[13px] font-semibold text-danger transition-colors hover:bg-danger/25"
    >
      Confirmer la suppression
    </button>
  );
}

function SearchHeader({ query, count }: { query: string; count: number }) {
  return (
    <div className="px-6 pb-2 pt-6">
      <p className="text-xs font-semibold uppercase tracking-[0.16em] text-ink-muted">
        Résultats
      </p>
      <h1 className="display mt-2 text-[clamp(1.5rem,3.5vw,2.5rem)] text-ink">
        {query.trim()}
      </h1>
      <p className="mt-2 text-[13px] text-ink-muted">
        {count} morceau{count > 1 ? "x" : ""} trouvé{count > 1 ? "s" : ""}
      </p>
    </div>
  );
}

// ════════════════════════════════════════════════════════════════════════════
//  Bandeaux
// ════════════════════════════════════════════════════════════════════════════

/**
 * Les nouvelles passagères : import en cours, bilan, erreur.
 *
 * # Pourquoi elles flottent
 *
 * Dans le flux de la page, elles poussaient tout le contenu vers le bas à
 * chaque import — la liste sautait, la position de défilement se perdait — et
 * elles disparaissaient au premier changement de page, c'est-à-dire au moment
 * où l'on voulait justement suivre le transfert.
 *
 * # Pourquoi le bilan s'efface tout seul
 *
 * « 2351 importés » est une nouvelle, pas un état. Une fois lue, elle n'a plus
 * rien à dire et n'a aucune raison d'attendre un clic. L'erreur, elle, reste :
 * on ne fait pas disparaître un problème au bout de dix secondes.
 */
function Banners({
  progress,
  summary,
  error,
  mobile,
  onDismissSummary,
  onDismissError,
}: {
  progress: ScanProgress | null;
  summary: ScanSummary | null;
  error: string | null;
  mobile: boolean;
  onDismissSummary: () => void;
  onDismissError: () => void;
}) {
  useEffect(() => {
    if (summary === null) return;

    const minuteur = setTimeout(onDismissSummary, 10_000);
    return () => clearTimeout(minuteur);
  }, [summary, onDismissSummary]);

  if (progress === null && summary === null && error === null) return null;

  return (
    <div
      className={`pointer-events-none fixed left-1/2 z-50 w-full max-w-lg -translate-x-1/2 space-y-2 px-3 ${
        mobile ? "top-[calc(env(safe-area-inset-top)+0.75rem)]" : "top-12"
      }`}
    >
      {progress !== null && <ProgressBar progress={progress} />}

      {summary !== null && (
        <div className="pointer-events-auto rounded-2xl bg-elevated px-4 py-3 shadow-2xl shadow-black/50">
          <div className="flex items-start justify-between gap-4 text-[13px] text-ink-muted">
            <div>
              <p>
                <span className="text-ok">{summary.imported} importé(s)</span>
                {summary.duplicates > 0 && <> · {summary.duplicates} doublon(s) ignoré(s)</>}
                {summary.failed > 0 && (
                  <>
                    {" · "}
                    <span className="text-danger">{summary.failed} échec(s)</span>
                  </>
                )}
              </p>

              {summary.errors.length > 0 && (
                <ul className="mt-1.5 space-y-0.5 text-xs text-ink-faint">
                  {summary.errors.map((message) => (
                    <li key={message} className="truncate">
                      {message}
                    </li>
                  ))}
                </ul>
              )}
            </div>

            <button
              type="button"
              onClick={onDismissSummary}
              className="shrink-0 text-ink-faint transition-colors hover:text-ink"
            >
              <Icon name="close" size={16} />
            </button>
          </div>
        </div>
      )}

      {error !== null && (
        <div className="pointer-events-auto flex items-start justify-between gap-4 rounded-2xl bg-danger/15 px-4 py-3 shadow-2xl shadow-black/50 backdrop-blur-sm">
          <p className="text-[13px] leading-relaxed text-danger">{error}</p>
          <button
            type="button"
            onClick={onDismissError}
            className="shrink-0 text-danger/70 transition-colors hover:text-danger"
          >
            <Icon name="close" size={16} />
          </button>
        </div>
      )}
    </div>
  );
}

function ProgressBar({ progress }: { progress: ScanProgress }) {
  const ratio = progress.total === 0 ? 0 : progress.processed / progress.total;

  return (
    <div className="pointer-events-auto rounded-2xl bg-elevated px-4 py-3 shadow-2xl shadow-black/50">
      <div className="h-1 overflow-hidden rounded-full bg-raised">
        <div
          className="h-full rounded-full bg-ink transition-[width] duration-150"
          style={{ width: `${ratio * 100}%` }}
        />
      </div>

      <p className="mt-2 flex items-center justify-between gap-4 text-xs text-ink-muted">
        <span className="truncate">{progress.current || "Analyse du dossier…"}</span>
        <span className="numerals shrink-0">
          {progress.processed} / {progress.total}
        </span>
      </p>
    </div>
  );
}

/**
 * L'onglet du bas correspondant à la page affichée.
 *
 * La recherche prime : c'est un mode qui se superpose à une page, et tant
 * qu'il est ouvert c'est lui que l'utilisateur habite.
 */
function ongletActif(route: Route, searchOpen: boolean): string {
  if (searchOpen) return "search";

  switch (route.kind) {
    case "home":
      return "home";
    case "artists":
    case "artist":
      return "artists";
    case "more":
    case "settings":
    case "pairing":
    case "shortcuts":
    case "sync":
    case "stats":
    case "loved":
    case "playlist":
      return "more";
    default:
      return "library";
  }
}

/**
 * Ce qu'un champ de recherche vide a d'utile à proposer.
 *
 * # Pourquoi les précédentes plutôt que des suggestions
 *
 * Suggérer demanderait de deviner ; rappeler ne demande que de se souvenir. Et
 * dans une bibliothèque qu'on a soi-même constituée, ce qu'on a cherché hier
 * est le meilleur indice de ce qu'on cherche aujourd'hui.
 */
function RecherchesRecentes({ onChoose }: { onChoose: (terme: string) => void }) {
  const [recentes, setRecentes] = useState<string[]>(() => recherchesRecentes());

  if (recentes.length === 0) {
    return (
      <p className="px-6 py-16 text-center text-[13px] leading-relaxed text-ink-faint">
        Cherche un titre, un artiste ou un album.
      </p>
    );
  }

  return (
    <div className="px-4 pb-6 pt-2">
      <div className="flex items-center justify-between px-2">
        <p className="text-[11px] font-semibold uppercase tracking-[0.14em] text-ink-faint">
          Recherches récentes
        </p>
        <button
          type="button"
          onClick={() => {
            oublierRecherches();
            setRecentes([]);
          }}
          className="text-[12px] text-ink-faint transition-colors hover:text-ink"
        >
          Effacer
        </button>
      </div>

      <ul className="mt-1">
        {recentes.map((terme) => (
          <li key={terme}>
            <button
              type="button"
              onClick={() => onChoose(terme)}
              className="flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-colors hover:bg-elevated"
            >
              <span className="text-ink-faint">
                <Icon name="clock" size={15} />
              </span>
              <span className="min-w-0 flex-1 truncate text-[15px] text-ink">{terme}</span>
              <span className="text-ink-faint">
                <Icon name="chevronRight" size={15} />
              </span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

/**
 * La recherche sur écran étroit.
 *
 * # Pourquoi un champ qui apparaît et non un champ permanent
 *
 * En haut de chaque page, il coûterait cinquante pixels de hauteur en
 * permanence pour une action qu'on fait rarement. Ouvert depuis son onglet, il
 * prend la place qu'il faut au moment où l'on en a besoin, et la rend ensuite.
 */
function MobileSearch({
  open,
  query,
  onQuery,
  onClose,
}: {
  open: boolean;
  query: string;
  onQuery: (query: string) => void;
  onClose: () => void;
}) {
  if (!open) return null;

  return (
    <div className="sticky top-0 z-10 flex items-center gap-2 bg-surface px-4 pb-3 pt-4">
      <div className="flex min-w-0 flex-1 items-center gap-2 rounded-full bg-elevated px-4 py-2.5">
        <Icon name="search" size={16} />
        <input
          autoFocus
          value={query}
          onChange={(event) => onQuery(event.target.value)}
          placeholder="Titre, artiste, album…"
          className="min-w-0 flex-1 bg-transparent text-[15px] text-ink placeholder:text-ink-faint focus:outline-none"
        />
        {query !== "" && (
          <button
            type="button"
            aria-label="Effacer"
            onClick={() => onQuery("")}
            className="text-ink-faint"
          >
            <Icon name="close" size={15} />
          </button>
        )}
      </div>

      <button
        type="button"
        onClick={onClose}
        className="shrink-0 text-[14px] text-ink-muted"
      >
        Annuler
      </button>
    </div>
  );
}
