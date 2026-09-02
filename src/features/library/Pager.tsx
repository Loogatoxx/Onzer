import { Icon } from "@/components/Icon";

/**
 * Navigation par pages de la bibliothèque.
 *
 * # Pourquoi des numéros et pas seulement des flèches
 *
 * Vingt pages franchies une par une, c'est vingt clics pour aller au bout.
 * Les numéros donnent le saut direct — et surtout ils **situent** : « page 3
 * sur 21 » dit d'un coup d'œil où l'on est et ce qu'il reste, ce que le
 * défilement infini ne dit jamais.
 *
 * # Pourquoi seulement une fenêtre de numéros
 *
 * Vingt-et-un boutons rempliraient la barre et deviendraient illisibles. On
 * n'affiche que le voisinage de la page courante, plus la première et la
 * dernière — celles-là étant les seules destinations lointaines qu'on vise
 * réellement.
 */
export function Pager({
  page,
  pageCount,
  onChange,
}: {
  /** Page courante, à partir de zéro. */
  page: number;
  pageCount: number;
  onChange: (page: number) => void;
}) {
  if (pageCount <= 1) return null;

  const numbers = window(page, pageCount);

  return (
    <nav
      aria-label="Pages de la bibliothèque"
      className="flex flex-wrap items-center justify-center gap-1.5 px-6 py-8"
    >
      <Arrow
        label="Page précédente"
        icon="chevronLeft"
        disabled={page === 0}
        onClick={() => onChange(page - 1)}
      />

      {numbers.map((number, index) =>
        number === null ? (
          <span key={`saut-${index}`} className="px-1 text-[13px] text-ink-faint">
            …
          </span>
        ) : (
          <button
            key={number}
            type="button"
            aria-current={number === page ? "page" : undefined}
            onClick={() => onChange(number)}
            className={`numerals h-8 min-w-8 rounded-full px-2.5 text-[13px] transition-colors ${
              number === page
                ? "bg-ink font-semibold text-base"
                : "text-ink-muted hover:bg-elevated hover:text-ink"
            }`}
          >
            {number + 1}
          </button>
        ),
      )}

      <Arrow
        label="Page suivante"
        icon="chevronRight"
        disabled={page >= pageCount - 1}
        onClick={() => onChange(page + 1)}
      />
    </nav>
  );
}

function Arrow({
  label,
  icon,
  disabled,
  onClick,
}: {
  label: string;
  icon: "chevronLeft" | "chevronRight";
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onClick}
      className="flex h-8 w-8 items-center justify-center rounded-full text-ink-muted transition-colors hover:bg-elevated hover:text-ink disabled:pointer-events-none disabled:opacity-30"
    >
      <Icon name={icon} size={16} />
    </button>
  );
}

/**
 * Les numéros à afficher, `null` marquant une coupure.
 *
 * Toujours la première, la dernière, et deux voisines de chaque côté de la
 * page courante. La largeur du rendu ne dépend donc pas de la taille de la
 * bibliothèque.
 */
export function window(page: number, pageCount: number): (number | null)[] {
  const shown = new Set<number>([0, pageCount - 1]);

  for (let offset = -2; offset <= 2; offset += 1) {
    const candidate = page + offset;
    if (candidate >= 0 && candidate < pageCount) shown.add(candidate);
  }

  const sorted = [...shown].sort((left, right) => left - right);
  const out: (number | null)[] = [];

  sorted.forEach((number, index) => {
    const previous = sorted[index - 1];
    // Une coupure ne se marque que s'il manque **plus d'une** page : afficher
    // « … » à la place d'un seul numéro serait plus long que le numéro.
    if (previous !== undefined && number - previous > 1) out.push(null);
    out.push(number);
  });

  return out;
}
