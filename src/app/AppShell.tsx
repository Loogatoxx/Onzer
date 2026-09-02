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
import { OfflineBar } from "@/features/library/OfflineBar";
import { Pager } from "@/features/library/Pager";
import { LyricsBar } from "@/features/lyrics/LyricsBar";
import { ListenBar } from "@/features/lyrics/ListenBar";
import { Artwork } from "@/features/library/Artwork";
import { CorrectDialog } from "@/features/library/CorrectDialog";
import { MatchDialog } from "@/features/library/MatchDialog";
import { DuplicatePanel } from "@/features/library/DuplicatePanel";
import { TrackTable } from "@/features/library/TrackTable";
import { Sidebar, type Route } from "@/features/nav/Sidebar";
import { MobileTabs } from "@/features/nav/MobileTabs";
import { MiniPlayer } from "@/features/player/MiniPlayer";
import { useIsMobile } from "@/lib/useIsMobile";
import { TopBar } from "@/features/nav/TopBar";
import { NowPlayingPanel, type PanelTab } from "@/features/player/NowPlayingPanel";
import { LyricsView } from "@/features/player/LyricsView";
import { PlayerBar } from "@/features/player/PlayerBar";
import { usePlayback } from "@/features/player/usePlayback";
import { ShortcutsView } from "@/features/nav/ShortcutsView";
import { SettingsView } from "@/features/nav/SettingsView";
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
  const searching = query.trim() !== "";
  const shown = searching ? (results ?? []) : tracks;

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
      .then(() => ipc.lovedTracks())
      .then((lovedTracks) => setLoved(new Set(lovedTracks.map((track) => track.id))))
      .catch((cause: unknown) => setError(String(cause)));

    reloadPlaylists();
  }, [reloadPlaylists]);

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

  /** Sur mobile, la recherche est un mode et non une page. */
  const [searchOpen, setSearchOpen] = useState(false);

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
      || route.kind === "sync"
      || route.kind === "shortcuts"
      || route.kind === "settings"
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
        default:
          return ipc.listTracks(PAGE_SIZE, page * PAGE_SIZE);
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
  }, [route, revision, page]);

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

  const table = (
    <TrackTable
      tracks={shown}
      currentTrackId={playback.state?.current?.trackId ?? null}
      isPlaying={playback.state?.isPlaying ?? false}
      onPlay={playFrom}
      onRadio={startRadio}
      onToggleLoved={(id) => void toggleLoved(id)}
      onEnqueue={(id) => {
        void playback.enqueue([id]);
      }}
      onOpenArtist={(id) => void openArtistOf(id)}
      onCorrect={setCorrecting}
      onMatch={setMatching}
      onSyncLyrics={syncLyrics}
      onRemove={(id) => {
        void ipc
          .removeTrack(id)
          .then(() => {
            setCounts(null);
            void ipc.libraryCounts().then(setCounts).catch(() => undefined);
            bump();
          })
          .catch((cause: unknown) => setError(String(cause)));
      }}
      loved={loved}
      playlists={playlists}
      onAddToPlaylist={addToPlaylist}
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
    <div className="flex h-full flex-col bg-base">
      {/* Les feux de circulation de macOS vivent ici : la fenêtre n'a pas de
          barre de titre, il faut donc lui réserver sa bande de glissement.
          Un téléphone n'a ni fenêtre ni feux — la bande n'y serait qu'un vide
          de neuf pixels en haut de l'écran. */}
      {!mobile && <div className="drag-region h-9 shrink-0" />}

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
          className={`min-h-0 min-w-0 flex-1 overflow-y-auto bg-surface ${
            mobile ? "" : "rounded-xl"
          }`}
        >
          {mobile ? (
            <MobileSearch
              open={searchOpen}
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

          <Banners
            progress={progress}
            summary={summary}
            error={error ?? playback.error}
            onDismissSummary={() => setSummary(null)}
            onDismissError={() => {
              setError(null);
              playback.dismissError();
            }}
          />

          {searching ? (
            <>
              <SearchHeader query={query} count={shown.length} />
              {paged}
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
              onOpen={() => openLyrics()}
            />
          )}

          <MobileTabs
            active={ongletActif(route, searchOpen)}
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
function Page(props: PageProps) {
  const { route, tracks } = props;

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

  if (route.kind === "settings") {
    return <SettingsView onChanged={props.onReload} />;
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
        />
        {props.children}
      </>
    );
  }

  // ── Bibliothèque ──────────────────────────────────────────────────────
  return (
    <>
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
      />

      <div className="px-6 pb-2">
        <DiscoverBar
          disabled={props.importing || (props.counts?.tracks ?? 0) === 0}
          onGenerated={props.onGenerated}
          onError={props.onError}
        />

        {/* Les doublons ne dépendent d'aucun service : deux exemplaires du
            même morceau se repèrent sans rien demander à personne, et cette
            question-là reste valable même sur une bibliothèque impeccable. */}
        <div className="mt-3 space-y-2">
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
        </div>

        <p className="mt-3 truncate font-mono text-[11px] text-ink-faint">
          {props.libraryRoot}
        </p>
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

function Banners({
  progress,
  summary,
  error,
  onDismissSummary,
  onDismissError,
}: {
  progress: ScanProgress | null;
  summary: ScanSummary | null;
  error: string | null;
  onDismissSummary: () => void;
  onDismissError: () => void;
}) {
  return (
    <>
      {progress !== null && <ProgressBar progress={progress} />}

      {summary !== null && (
        <div className="mx-4 mt-2 rounded-lg bg-elevated px-4 py-3">
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
        <div className="mx-4 mt-2 flex items-start justify-between gap-4 rounded-lg bg-danger/10 px-4 py-3">
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
    </>
  );
}

function ProgressBar({ progress }: { progress: ScanProgress }) {
  const ratio = progress.total === 0 ? 0 : progress.processed / progress.total;

  return (
    <div className="mx-4 mt-2 rounded-lg bg-elevated px-4 py-3">
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
    case "settings":
    case "shortcuts":
    case "sync":
      return "settings";
    default:
      return "library";
  }
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
