import { useEffect, useRef, useState } from "react";

import { Artwork } from "@/features/library/Artwork";
import { Icon } from "@/components/Icon";
import { formatDuration, ipc, type PlaybackSnapshot } from "@/lib/ipc";
import { useFermeture } from "@/lib/useFermeture";

/**
 * L'écran de lecture : ce qu'on regarde quand on écoute.
 *
 * # Pourquoi il ne montre pas les paroles d'emblée
 *
 * Toucher le morceau en cours menait droit au texte. Or on n'ouvre pas toujours
 * un morceau pour le lire : on veut souvent juste **voir ce qui joue** — la
 * pochette en grand, l'album, la durée. Les paroles sont une intention à part,
 * elles méritent leur bouton.
 *
 * # Pourquoi la pochette occupe autant de place
 *
 * C'est la seule image de toute l'application. Le reste est du texte sur du
 * gris ; ici, la couleur vient du disque lui-même. La réduire pour caser des
 * commandes reviendrait à remplacer la seule chose qu'on regarde par des
 * choses qu'on cherche.
 */
/**
 * La page est-elle déjà tout en haut ?
 *
 * Sans cette question, tirer vers le bas au milieu d'une page qui défile
 * emporterait la feuille **et** la liste : deux réponses à un seul geste.
 */
function auSommet(depuis: HTMLElement): boolean {
  let noeud: HTMLElement | null = depuis;

  while (noeud !== null) {
    const style = window.getComputedStyle(noeud).overflowY;
    if (style === "auto" || style === "scroll") return noeud.scrollTop <= 0;
    noeud = noeud.parentElement;
  }

  return true;
}

export function NowPlayingView({
  state,
  isLoved,
  onToggle,
  onNext,
  onPrevious,
  onSeek,
  onToggleLoved,
  onOpenLyrics,
  onOpenArtist,
  onOpenAlbum,
  onShuffle,
  onRepeat,
  onClose,
  fileDAttente,
}: {
  state: PlaybackSnapshot;
  isLoved: boolean;
  onToggle: () => void;
  onNext: () => void;
  onPrevious: () => void;
  onSeek: (positionMs: number) => void;
  onToggleLoved: () => void;
  onOpenLyrics: () => void;
  onOpenArtist: () => void;
  onOpenAlbum: () => void;
  onShuffle: (shuffle: boolean) => void;
  onRepeat: () => void;
  /** Referme l'écran, appelé par le glissement vers le bas. */
  onClose: () => void;
  /**
   * La suite de la file, rendue par la coquille.
   *
   * # Pourquoi elle vient de l'extérieur
   *
   * Ces lignes doivent être **exactement** celles de la bibliothèque : même
   * numéro, même pastille de paroles, mêmes trois points, mêmes bords. Les
   * redessiner ici garantirait qu'elles s'en écartent à la première
   * correction — c'est déjà ce qui est arrivé avec l'en-tête et la grille.
   * La coquille tient le tableau et tous ses gestes ; elle le prête.
   */
  fileDAttente: React.ReactNode;
}) {
  const [agrandie, setAgrandie] = useState(false);

  /**
   * Un glissement vers le bas referme l'écran.
   *
   * C'est le geste que tous les lecteurs de téléphone ont adopté pour cet
   * écran-là, et le seul qui n'oblige pas à remonter le pouce jusqu'en haut.
   * Le seuil est haut — cent vingt pixels — parce qu'un défilement vertical
   * commence exactement de la même façon.
   */
  const depart = useRef<number | null>(null);
  /**
   * Ce que le doigt a tiré vers le bas, en pixels.
   *
   * L'écran ne suivait pas du tout : on tirait, rien ne bougeait, puis à cent
   * vingt pixels il **disparaissait d'un coup**. Rien ne disait que le geste
   * était compris, ni combien il en restait — et la sortie n'existait pas.
   */
  const [tire, setTire] = useState(0);
  /** Vrai le temps que la feuille achève de descendre. */
  const [ferme, setFerme] = useState(false);
  /** Le geste n'a le droit de tirer que si la page est déjà en haut. */
  const enHaut = useRef(true);
  const track = state.current;

  if (track === null) {
    return (
      <p className="px-6 py-20 text-center text-sm text-ink-muted">
        Lance un morceau pour le voir ici.
      </p>
    );
  }

  const ratio = state.durationMs > 0 ? state.positionMs / state.durationMs : 0;

  return (
    <div
      className="mx-auto flex w-full max-w-md flex-col px-6 pb-10 pt-6 lg:max-w-lg"
      // # Pourquoi le saut de quinze secondes n'est plus ici
      //
      // Il couvrait tout l'écran, curseur compris : essayer de poser la tête
      // de lecture à un endroit précis déclenchait un saut de quinze secondes
      // au lieu de la déplacer. Deux gestes horizontaux superposés, dont le
      // plus grossier gagnait toujours. Il reste sur le petit lecteur, où il
      // n'y a pas de curseur à viser.
      //
      // Le glissement vers le bas, lui, se lit sur les événements tactiles :
      // le pointeur est annulé dès que le navigateur croit à un défilement, et
      // un glissement vers le bas est précisément ce qui y ressemble le plus.
      style={{
        transform: ferme ? "translateY(100%)" : `translateY(${tire}px)`,
        // Le fondu accompagne la descente : à mi-chemin on voit déjà que
        // l'écran s'en va, ce qui rend le seuil devinable sans l'écrire.
        opacity: ferme ? 0 : 1 - Math.min(0.45, tire / 500),
        // Pendant qu'on tient, aucune transition : la moindre en ferait un
        // retard. Au relâchement, la feuille rejoint sa place ou s'en va.
        transition:
          tire > 0 && !ferme
            ? "none"
            : "transform 260ms var(--ease-out-soft), opacity 260ms var(--ease-out-soft)",
      }}
      onTouchStart={(event) => {
        const doigt = event.touches[0];
        depart.current = doigt === undefined ? null : doigt.clientY;
        enHaut.current = auSommet(event.currentTarget);
      }}
      onTouchMove={(event) => {
        const origine = depart.current;
        const doigt = event.touches[0];
        if (origine === null || doigt === undefined || !enHaut.current) return;

        // Vers le bas seulement : tirer vers le haut ne ferme rien, et
        // décoller la feuille de son bord donnerait un jeu qui n'existe pas.
        setTire(Math.max(0, doigt.clientY - origine));
      }}
      onTouchEnd={(event) => {
        const origine = depart.current;
        depart.current = null;

        const doigt = event.changedTouches[0];
        if (origine !== null && doigt !== undefined && doigt.clientY - origine > 120) {
          // Elle finit sa descente avant de céder la page : une fermeture qui
          // se voit est une fermeture qu'on a comprise.
          setFerme(true);
          setTimeout(onClose, 220);
          return;
        }
        setTire(0);
      }}
      onTouchCancel={() => {
        depart.current = null;
        setTire(0);
      }}
    >
      {/* # Toucher la pochette l'agrandit
          Elle est la seule image de l'application, et sur un téléphone elle
          reste bridée par la marge du texte. Un appui la met plein écran, sur
          fond noir : c'est le geste qu'on a partout ailleurs devant une image,
          et il ne coûte rien à qui ne le connaît pas. */}
      <button
        type="button"
        aria-label="Agrandir la pochette"
        onClick={() => setAgrandie(true)}
        className="pression w-full"
      >
        <Artwork
          hash={track.artworkHash}
          className="aspect-square w-full rounded-2xl shadow-2xl shadow-black/50"
        />
      </button>

      {agrandie && (
        <div
          role="dialog"
          aria-label="Pochette"
          onClick={() => setAgrandie(false)}
          className="fixed inset-0 z-50 flex items-center justify-center bg-base/95 p-4"
        >
          <Artwork hash={track.artworkHash} className="max-h-full w-full rounded-xl" />
        </div>
      )}

      <div className="mt-7">
        <h1 className="display text-[clamp(1.4rem,5vw,2rem)] leading-tight text-ink">
          {track.title}
        </h1>

        <p className="mt-2 flex flex-wrap items-center gap-x-2 text-[15px] text-ink-muted">
          <button
            type="button"
            onClick={onOpenArtist}
            className="transition-colors hover:text-ink hover:underline"
          >
            {track.artist ?? "Artiste inconnu"}
          </button>

          {track.album !== null && (
            <>
              <span className="text-ink-faint">·</span>
              <button
                type="button"
                onClick={onOpenAlbum}
                className="transition-colors hover:text-ink hover:underline"
              >
                {track.album}
              </button>
            </>
          )}
        </p>
      </div>

      {/* ── Position ─────────────────────────────────────────────────── */}
      <div className="mt-7">
        <input
          type="range"
          min={0}
          max={Math.max(1, state.durationMs)}
          value={state.positionMs}
          onChange={(event) => onSeek(Number(event.target.value))}
          aria-label="Position dans le morceau"
          className="curseur-lecture"
          style={
            {
              "--piste": `linear-gradient(to right, var(--color-ink) ${ratio * 100}%, var(--color-raised) ${ratio * 100}%)`,
            } as React.CSSProperties
          }
        />

        <div className="mt-1.5 flex justify-between text-[11px] text-ink-faint">
          <span className="numerals">{formatDuration(state.positionMs)}</span>
          <span className="numerals">{formatDuration(state.durationMs)}</span>
        </div>
      </div>

      {/* ── Commandes ────────────────────────────────────────────────── */}
      <div className="mt-6 flex items-center justify-center gap-5">
        {/* Aléatoire et répétition encadrent le transport, comme sur le
            bureau : ce sont des **états** de la file, pas des actions, et les
            mettre au même rang que « lire » les ferait déclencher par erreur.
            Leur couleur dit s'ils sont actifs. */}
        <button
          type="button"
          aria-label="Lecture aléatoire"
          aria-pressed={state.shuffle}
          onClick={() => onShuffle(!state.shuffle)}
          className={`transition-colors ${
            state.shuffle ? "text-accent" : "text-ink-faint hover:text-ink-muted"
          }`}
        >
          <Icon name="shuffle" size={20} />
        </button>

        <button
          type="button"
          aria-label="Morceau précédent"
          onClick={onPrevious}
          className="text-ink-muted transition-colors hover:text-ink"
        >
          <Icon name="previous" size={28} />
        </button>

        <button
          type="button"
          aria-label={state.isPlaying ? "Pause" : "Lire"}
          onClick={onToggle}
          className="flex h-16 w-16 items-center justify-center rounded-full bg-ink text-base transition-transform duration-150 active:scale-95"
        >
          <span className={state.isPlaying ? "" : "translate-x-[2px]"}>
            <Icon name={state.isPlaying ? "pause" : "play"} size={26} />
          </span>
        </button>

        <button
          type="button"
          aria-label="Morceau suivant"
          onClick={onNext}
          className="text-ink-muted transition-colors hover:text-ink"
        >
          <Icon name="next" size={28} />
        </button>

        <button
          type="button"
          aria-label={
            state.repeat === "off"
              ? "Répéter"
              : state.repeat === "all"
                ? "Répéter la file"
                : "Répéter ce morceau"
          }
          onClick={onRepeat}
          className={`transition-colors ${
            state.repeat === "off" ? "text-ink-faint hover:text-ink-muted" : "text-accent"
          }`}
        >
          <Icon name={state.repeat === "one" ? "repeatOne" : "repeat"} size={20} />
        </button>
      </div>

      {/* ── Ce qu'on peut vouloir ensuite ────────────────────────────── */}
      <div className="mt-8 flex items-center justify-center gap-3">
        <button
          type="button"
          onClick={onToggleLoved}
          className={`flex items-center gap-2 rounded-full px-4 py-2.5 text-[13px] font-medium transition-colors ${
            isLoved ? "bg-elevated text-accent" : "bg-elevated text-ink-muted hover:text-ink"
          }`}
        >
          <Icon name={isLoved ? "heartFilled" : "heart"} size={16} />
          {isLoved ? "Aimé" : "J'aime"}
        </button>

        <button
          type="button"
          onClick={onOpenLyrics}
          className="flex items-center gap-2 rounded-full bg-elevated px-4 py-2.5 text-[13px] font-medium text-ink-muted transition-colors hover:text-ink"
        >
          <Icon name="lyrics" size={16} />
          Paroles
        </button>

        <Minuteur restantDuMorceau={Math.max(0, state.durationMs - state.positionMs)} />
      </div>

      {/* La file rend les mêmes lignes que la bibliothèque : elle doit donc
          avoir la même largeur qu'elle. Les vingt-quatre pixels de marge de
          cet écran, qui aèrent la pochette et le titre, rétréciraient des
          lignes déjà calculées pour tenir sur 375 px. */}
      <div className="-mx-6">{fileDAttente}</div>
    </div>
  );
}

/**
 * Le minuteur de sommeil.
 *
 * # Pourquoi le compte à rebours n'est pas ici
 *
 * Une minuterie en JavaScript s'arrête quand le système gèle la page — c'est-à-
 * dire exactement quand on éteint l'écran, le seul moment où ce minuteur sert.
 * Le cœur tient l'échéance ; cet écran ne fait que la demander et l'afficher.
 *
 * # Pourquoi « fin du morceau » est un délai comme un autre
 *
 * On pourrait guetter la fin de la piste dans le cœur. Ce serait un second
 * mécanisme, avec sa propre façon de se tromper, pour un résultat que
 * l'arithmétique donne déjà : ce qu'il reste à jouer est une durée.
 */
function Minuteur({ restantDuMorceau }: { restantDuMorceau: number }) {
  const [ouvert, setOuvert] = useState(false);
  const monte = useFermeture(ouvert);
  const [restant, setRestant] = useState<number | null>(null);
  const ancre = useRef<HTMLDivElement>(null);

  // Rafraîchi toutes les quinze secondes : le bouton n'affiche que des
  // minutes, et interroger le cœur plus souvent ne changerait rien à l'écran.
  useEffect(() => {
    const lire = () => void ipc.sleepTimer().then(setRestant).catch(() => undefined);
    lire();

    const battement = setInterval(lire, 15_000);
    return () => clearInterval(battement);
  }, []);

  useEffect(() => {
    if (!ouvert) return;

    const fermer = (event: MouseEvent) => {
      if (!ancre.current?.contains(event.target as Node)) setOuvert(false);
    };
    document.addEventListener("mousedown", fermer);
    return () => document.removeEventListener("mousedown", fermer);
  }, [ouvert]);

  function armer(delai: number | null) {
    setOuvert(false);
    void ipc
      .setSleepTimer(delai)
      .then(setRestant)
      .catch(() => undefined);
  }

  const arme = restant !== null && restant > 0;
  const minutes = arme ? Math.max(1, Math.round(restant / 60_000)) : 0;

  const choix: { libelle: string; delai: number }[] = [
    { libelle: "15 minutes", delai: 15 * 60_000 },
    { libelle: "30 minutes", delai: 30 * 60_000 },
    { libelle: "1 heure", delai: 60 * 60_000 },
    { libelle: "Fin du morceau", delai: restantDuMorceau },
  ];

  return (
    <div ref={ancre} className="relative">
      <button
        type="button"
        onClick={() => setOuvert(!ouvert)}
        aria-expanded={ouvert}
        className={`pression flex items-center gap-2 rounded-full px-4 py-2.5 text-[13px] font-medium transition-colors ${
          arme ? "bg-elevated text-accent" : "bg-elevated text-ink-muted hover:text-ink"
        }`}
      >
        <Icon name="moon" size={16} />
        {arme ? `${minutes} min` : "Minuteur"}
      </button>

      {monte && (
        <div
          className={`${ouvert ? "animate-surgir" : "animate-disparaitre"} vers-le-haut absolute bottom-12 right-0 z-30 w-48 overflow-hidden rounded-lg border border-line bg-raised py-1 shadow-2xl shadow-black/60`}
        >
          {choix.map((entree) => (
            <button
              key={entree.libelle}
              type="button"
              onClick={() => armer(entree.delai)}
              className="flex w-full px-3 py-2 text-left text-[13px] text-ink-muted transition-colors hover:bg-elevated hover:text-ink"
            >
              {entree.libelle}
            </button>
          ))}

          {arme && (
            <button
              type="button"
              onClick={() => armer(null)}
              className="mt-1 flex w-full border-t border-line px-3 py-2 text-left text-[13px] text-ink transition-colors hover:bg-elevated"
            >
              Arrêter le minuteur
            </button>
          )}
        </div>
      )}
    </div>
  );
}
