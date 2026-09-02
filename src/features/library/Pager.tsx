import { useState } from "react";

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
      // # Pourquoi jamais de retour à la ligne
      //
      // La flèche « suivant » passait à la ligne dès que les numéros
      // remplissaient la largeur, et se retrouvait seule en dessous. Une
      // commande qui change de place selon le nombre de pages devient
      // impossible à viser d'un geste appris. `flex-nowrap` la fige à droite,
      // quoi qu'il arrive.
      className="flex flex-nowrap items-center justify-center gap-1.5 overflow-x-auto px-6 py-8"
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

      <PageJump page={page} pageCount={pageCount} onChange={onChange} />
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
 * Aller directement à une page.
 *
 * # Pourquoi il ne suffit pas d'afficher plus de numéros
 *
 * Vingt-et-une pages tiennent encore ; deux cents, non — et c'est là qu'aller
 * au milieu devient pénible. Un champ où l'on écrit le numéro ne grandit pas
 * avec la bibliothèque, contrairement à toute rangée de boutons.
 */
function PageJump({
  page,
  pageCount,
  onChange,
}: {
  page: number;
  pageCount: number;
  onChange: (page: number) => void;
}) {
  const [draft, setDraft] = useState("");

  if (pageCount < 6) return null;

  const aller = () => {
    const demandee = Number.parseInt(draft, 10);
    setDraft("");

    if (Number.isNaN(demandee)) return;
    onChange(Math.min(pageCount, Math.max(1, demandee)) - 1);
  };

  return (
    <span className="ml-2 flex shrink-0 items-center gap-1.5 text-[12px] text-ink-faint">
      <span className="hidden sm:inline">sur {pageCount}</span>
      <input
        type="text"
        inputMode="numeric"
        value={draft}
        placeholder={String(page + 1)}
        aria-label={`Aller à une page, entre 1 et ${pageCount}`}
        onChange={(event) => setDraft(event.target.value.replace(/\D/g, ""))}
        onBlur={aller}
        onKeyDown={(event) => {
          if (event.key === "Enter") event.currentTarget.blur();
        }}
        className="numerals h-8 w-12 rounded-full bg-elevated text-center text-[13px] text-ink placeholder:text-ink-faint focus:outline focus:outline-1 focus:outline-ink-faint"
      />
    </span>
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

  // Une seule voisine de chaque côté : sept numéros alignés sur un téléphone
  // deviennent une rangée illisible, et l'on ne vise jamais « la page 9 »
  // quand on en est à la 7 — on vise la suivante, ou une page lointaine, et
  // celle-là se demande par son numéro.
  for (let offset = -1; offset <= 1; offset += 1) {
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
