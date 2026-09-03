/**
 * Jeu d'icônes de l'application.
 *
 * # Pourquoi les dessiner ici plutôt que d'ajouter une bibliothèque
 *
 * Une bibliothèque d'icônes apporte deux mille symboles pour en utiliser vingt,
 * et impose son propre style. En les rassemblant ici, on garantit surtout ce qui
 * fait qu'un jeu d'icônes paraît professionnel : **un trait rigoureusement
 * identique partout**. Un mélange d'épaisseurs, même léger, se remarque
 * immédiatement sans qu'on sache dire pourquoi.
 *
 * Toutes les formes sont dessinées sur une grille de 24, avec un trait de 1,75
 * et des extrémités arrondies. Les seules exceptions sont pleines — lecture,
 * pause, cœur — parce qu'un symbole d'action doit peser plus lourd qu'un
 * symbole de navigation.
 */

export type IconName =
  | "home"
  | "search"
  | "library"
  | "artist"
  | "stats"
  | "plus"
  | "heart"
  | "heartFilled"
  | "play"
  | "pause"
  | "previous"
  | "next"
  | "shuffle"
  | "repeat"
  | "repeatOne"
  | "volume"
  | "volumeMute"
  | "lyrics"
  | "queue"
  | "radio"
  | "sparkle"
  | "clock"
  | "chevronLeft"
  | "chevronRight"
  | "chevronDown"
  | "more"
  | "trash"
  | "pencil"
  | "folder"
  | "check"
  | "close"
  | "moon"
  | "devices"
  | "locate"
  | "panelRight"
  /* Un pictogramme ne désigne qu'une chose. Ces quatre-là ont été ajoutés
     parce qu'ils manquaient et qu'on en avait détourné d'autres : la roue
     dentée servait de crayon, la file d'attente de playlist, et la
     bibliothèque de « petites vignettes ». Un même dessin pour deux sens, et
     l'utilisateur cesse de faire confiance aux dessins. */
  | "settings"
  | "playlist"
  | "grid"
  | "list";

/** Formes pleines : réservées aux actions, jamais à la navigation. */
const FILLED: ReadonlySet<IconName> = new Set<IconName>([
  "play",
  "pause",
  "heartFilled",
]);

const PATHS: Record<IconName, React.ReactNode> = {
  home: <path d="M3 10.5 12 3l9 7.5V20a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z" />,
  search: (
    <>
      <circle cx="11" cy="11" r="7" />
      <path d="m20 20-3.5-3.5" />
    </>
  ),
  library: <path d="M4 4v16M9.5 4v16M15 5l4.5 14" />,
  // Tête et épaules, sur la même grille de 24 et le même trait que le reste.
  // Un rond plein aurait juré : toutes les autres formes de navigation sont
  // en filaire, et l'œil repère immédiatement l'intruse.
  artist: (
    <>
      <circle cx="12" cy="8" r="3.6" />
      <path d="M5 20a7 7 0 0 1 14 0" />
    </>
  ),
  stats: <path d="M4 20V10M10 20V4M16 20v-7M22 20H2" />,
  plus: <path d="M12 5v14M5 12h14" />,
  heart: (
    <path d="M12 20s-7.5-4.6-7.5-9.7A4.3 4.3 0 0 1 12 7.6a4.3 4.3 0 0 1 7.5 2.7C19.5 15.4 12 20 12 20Z" />
  ),
  heartFilled: (
    <path d="M12 20s-7.5-4.6-7.5-9.7A4.3 4.3 0 0 1 12 7.6a4.3 4.3 0 0 1 7.5 2.7C19.5 15.4 12 20 12 20Z" />
  ),
  play: <path d="M7 4.5v15a.6.6 0 0 0 .93.5l11.5-7.5a.6.6 0 0 0 0-1L7.93 4a.6.6 0 0 0-.93.5Z" />,
  pause: <path d="M7 4h3.2v16H7zM13.8 4H17v16h-3.2z" />,
  previous: (
    <>
      <path d="M19 19V5l-11 7 11 7Z" fill="currentColor" stroke="none" />
      <path d="M5 5v14" />
    </>
  ),
  next: (
    <>
      <path d="M5 5v14l11-7L5 5Z" fill="currentColor" stroke="none" />
      <path d="M19 5v14" />
    </>
  ),
  shuffle: <path d="M17 3h4v4M21 3l-6.5 6.5M17 21h4v-4M21 21l-6.5-6.5M3 4l5.5 5.5M3 20l6-6" />,
  repeat: <path d="m17 2 4 4-4 4M3 11v-1a4 4 0 0 1 4-4h14M7 22l-4-4 4-4M21 13v1a4 4 0 0 1-4 4H3" />,
  repeatOne: (
    <>
      <path d="m17 2 4 4-4 4M3 11v-1a4 4 0 0 1 4-4h14M7 22l-4-4 4-4M21 13v1a4 4 0 0 1-4 4H3" />
      <path d="M11.4 10.4h.9v3.4" />
    </>
  ),
  volume: (
    <>
      <path d="M11 5 6.5 9H3v6h3.5L11 19z" />
      <path d="M15 9.5a3.5 3.5 0 0 1 0 5M17.8 7a7 7 0 0 1 0 10" />
    </>
  ),
  volumeMute: (
    <>
      <path d="M11 5 6.5 9H3v6h3.5L11 19z" />
      <path d="m16 10 4 4M20 10l-4 4" />
    </>
  ),
  lyrics: <path d="M5 5h14M5 10h14M5 15h9M5 20h5" />,
  queue: (
    <>
      <path d="M4 6h16M4 11h16M4 16h8" />
      <path d="m14 16 5 3-5 3z" fill="currentColor" stroke="none" />
    </>
  ),
  radio: (
    <>
      <circle cx="12" cy="12" r="2.2" />
      <path d="M8.4 15.6a5 5 0 0 1 0-7.2M15.6 8.4a5 5 0 0 1 0 7.2" />
      <path d="M5.4 18.6a9 9 0 0 1 0-13.2M18.6 5.4a9 9 0 0 1 0 13.2" />
    </>
  ),
  sparkle: <path d="m12 3 2 6.2 6.2 2-6.2 2L12 21l-2-7.8-6.2-2 6.2-2z" />,
  clock: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7v5.3l3.2 2" />
    </>
  ),
  chevronLeft: <path d="m14.5 5-7 7 7 7" />,
  chevronRight: <path d="m9.5 5 7 7-7 7" />,
  chevronDown: <path d="m5 9.5 7 7 7-7" />,
  more: (
    <>
      <circle cx="5.5" cy="12" r="1.4" fill="currentColor" stroke="none" />
      <circle cx="12" cy="12" r="1.4" fill="currentColor" stroke="none" />
      <circle cx="18.5" cy="12" r="1.4" fill="currentColor" stroke="none" />
    </>
  ),
  trash: <path d="M4 7h16M9.5 7V4.5h5V7M6.5 7l1 13h9l1-13M10 11v5M14 11v5" />,
  pencil: <path d="M4 20h4L20 8l-4-4L4 16zM15 5l4 4" />,
  folder: <path d="M3 7a2 2 0 0 1 2-2h4l2 2.5h8a2 2 0 0 1 2 2V18a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />,
  check: <path d="m5 12.5 4.5 4.5L19 7" />,
  settings: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1A1.7 1.7 0 0 0 8.9 19a1.7 1.7 0 0 0-1.9.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.9 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1A1.7 1.7 0 0 0 4.6 8.4a1.7 1.7 0 0 0-.3-1.9l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.9.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.9-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.9V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
    </>
  ),
  playlist: (
    <>
      <path d="M4 6h11M4 11h11M4 16h6" />
      <path d="M17 17.5V11l4-1.2" />
      <circle cx="15.4" cy="17.6" r="1.9" />
    </>
  ),
  grid: (
    <>
      <rect x="4" y="4" width="6.5" height="6.5" rx="1.5" />
      <rect x="13.5" y="4" width="6.5" height="6.5" rx="1.5" />
      <rect x="4" y="13.5" width="6.5" height="6.5" rx="1.5" />
      <rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.5" />
    </>
  ),
  list: (
    <>
      <path d="M9 6h11M9 12h11M9 18h11" />
      <circle cx="4.6" cy="6" r="1.1" />
      <circle cx="4.6" cy="12" r="1.1" />
      <circle cx="4.6" cy="18" r="1.1" />
    </>
  ),
  close: <path d="m6 6 12 12M18 6 6 18" />,
  // Le croissant du minuteur de sommeil. Un réveil aurait dit l'inverse de ce
  // qu'il fait : celui-ci endort la musique, il ne réveille personne.
  moon: <path d="M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5Z" />,
  // Deux appareils qui se parlent : un écran, un téléphone, et le va-et-vient
  // entre les deux.
  // Une cible : « où suis-je dans cette liste ». Ni une loupe, qui cherche,
  // ni une flèche, qui déplace.
  locate: (
    <>
      <circle cx="12" cy="12" r="6" />
      <circle cx="12" cy="12" r="1.6" fill="currentColor" />
      <path d="M12 2v3M12 19v3M2 12h3M19 12h3" />
    </>
  ),
  devices: (
    <>
      <rect x="2" y="4" width="12" height="9" rx="1.5" />
      <rect x="15.5" y="9" width="6.5" height="11" rx="1.5" />
      <path d="M5 20h5M7.5 13v7" />
    </>
  ),
  panelRight: (
    <>
      <rect x="3" y="4.5" width="18" height="15" rx="2" />
      <path d="M15 4.5v15" />
    </>
  ),
};

interface IconProps {
  name: IconName;
  /** Taille en pixels. 18 pour un bouton, 24 pour un élément principal. */
  size?: number;
  className?: string;
}

export function Icon({ name, size = 18, className = "" }: IconProps) {
  const filled = FILLED.has(name);

  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      className={className}
      fill={filled ? "currentColor" : "none"}
      stroke={filled ? "none" : "currentColor"}
      strokeWidth={1.75}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
      focusable="false"
    >
      {PATHS[name]}
    </svg>
  );
}

/**
 * Bouton d'icône.
 *
 * L'aire cliquable dépasse toujours le dessin : une icône de 18 px dans une
 * cible de 18 px est pénible à atteindre, alors qu'une cible de 32 px se clique
 * sans y penser. Le rectangle reste invisible tant qu'on ne survole pas.
 */
export function IconButton({
  name,
  label,
  active = false,
  disabled = false,
  size = 18,
  onClick,
  className = "",
}: {
  name: IconName;
  label: string;
  active?: boolean;
  disabled?: boolean;
  size?: number;
  /**
   * L'événement est transmis : un bouton posé dans une ligne cliquable doit
   * pouvoir arrêter la propagation, sans quoi son action et celle de la ligne
   * se déclenchent ensemble.
   */
  onClick?: (event: React.MouseEvent<HTMLButtonElement>) => void;
  className?: string;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active}
      disabled={disabled}
      onClick={onClick}
      className={`pression flex h-8 w-8 shrink-0 items-center justify-center rounded-full disabled:opacity-30 ${
        active
          ? "text-accent"
          : "text-ink-muted hover:bg-elevated hover:text-ink"
      } ${className}`}
    >
      <Icon name={name} size={size} />
    </button>
  );
}
