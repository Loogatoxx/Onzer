import { useEffect, useState } from "react";

import { Icon } from "@/components/Icon";
import { ipc } from "@/lib/ipc";

/**
 * Une note personnelle sur un morceau.
 *
 * # Pourquoi une bibliothèque personnelle mérite ça
 *
 * Un catalogue en ligne sait tout d'un morceau sauf l'essentiel : **pourquoi il
 * est chez toi**. « Découvert en Corse », « celui que passait mon frère », « à
 * réécouter au casque » — rien de cela n'existe dans MusicBrainz, et rien ne
 * pourra jamais l'y mettre. C'est la seule information qu'un service, aussi
 * complet soit-il, ne peut pas fournir à ta place.
 *
 * # Ce qu'elle n'est pas
 *
 * Elle n'entre ni dans la recommandation ni dans les statistiques : ce serait
 * transformer un souvenir en donnée. Et elle ne va pas dans les tags du
 * fichier, contrairement aux paroles — une note voyagerait alors avec le
 * fichier, jusque chez quelqu'un d'autre.
 */
export function TrackNote({ trackId }: { trackId: number }) {
  const [note, setNote] = useState("");
  const [editing, setEditing] = useState(false);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    setEditing(false);
    void ipc
      .trackNote(trackId)
      .then((found) => setNote(found ?? ""))
      .catch(() => setNote(""));
  }, [trackId]);

  async function save() {
    try {
      await ipc.setTrackNote(trackId, note);
      setEditing(false);
      setSaved(true);
      setTimeout(() => setSaved(false), 1500);
    } catch {
      // Une note non enregistrée n'a pas à interrompre l'écoute.
    }
  }

  if (!editing && note.trim() === "") {
    return (
      <button
        type="button"
        onClick={() => setEditing(true)}
        className="mt-4 flex w-full items-center justify-center gap-2 rounded-lg border border-dashed border-line py-2 text-[12px] text-ink-faint transition-colors hover:border-ink-faint hover:text-ink-muted"
      >
        <Icon name="pencil" size={13} />
        Ajouter une note
      </button>
    );
  }

  if (!editing) {
    return (
      <button
        type="button"
        onClick={() => setEditing(true)}
        className="mt-4 block w-full rounded-lg bg-elevated p-3 text-left transition-colors hover:bg-raised"
      >
        <span className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-[0.14em] text-ink-faint">
          <Icon name="pencil" size={11} />
          Ta note
        </span>
        <span className="mt-1.5 block whitespace-pre-wrap text-[13px] leading-relaxed text-ink-muted">
          {note}
        </span>
      </button>
    );
  }

  return (
    <div className="mt-4">
      <textarea
        autoFocus
        value={note}
        onChange={(event) => setNote(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape") setEditing(false);
          // Entrée valide, Maj+Entrée passe à la ligne : une note tient en une
          // phrase la plupart du temps.
          if (event.key === "Enter" && !event.shiftKey) {
            event.preventDefault();
            void save();
          }
        }}
        placeholder="Pourquoi ce morceau compte…"
        className="h-20 w-full resize-none rounded-lg bg-base p-3 text-[13px] leading-relaxed text-ink placeholder:text-ink-faint focus:outline focus:outline-1 focus:outline-accent"
      />

      <div className="mt-2 flex items-center gap-2">
        <button
          type="button"
          onClick={() => void save()}
          className="rounded-full bg-ink px-4 py-1 text-[12px] font-semibold text-base transition-opacity hover:opacity-90"
        >
          Enregistrer
        </button>
        <button
          type="button"
          onClick={() => setEditing(false)}
          className="text-[12px] text-ink-faint transition-colors hover:text-ink"
        >
          Annuler
        </button>
        {saved && <span className="text-[11px] text-ok">Enregistrée</span>}
      </div>
    </div>
  );
}
