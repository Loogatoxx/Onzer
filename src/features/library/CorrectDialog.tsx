import { useState } from "react";

import { Icon } from "@/components/Icon";
import { ipc, type TrackSummary } from "@/lib/ipc";

/**
 * Corriger un morceau à la main.
 *
 * # Pourquoi cette porte existe
 *
 * L'identification acoustique se trompe, et pas seulement sur des cas tordus :
 * un morceau nommé « Medecine » alors qu'il s'agit de « Ma go ». L'erreur se
 * propage — les paroles récupérées sont celles du mauvais titre, la pochette
 * aussi. Sans moyen de corriger, il faudrait sortir le fichier de la
 * bibliothèque et le réimporter.
 *
 * # Ce que la correction emporte
 *
 * Les paroles sont effacées : elles appartenaient à l'ancien titre. Les garder
 * ferait afficher celles d'un autre morceau — précisément le symptôme qu'on
 * répare. Un clic sur « Chercher en ligne » les reprendra sous le bon nom.
 */
export function CorrectDialog({
  track,
  onClose,
  onCorrected,
}: {
  track: TrackSummary;
  onClose: () => void;
  onCorrected: () => void;
}) {
  const [title, setTitle] = useState(track.title);
  const [artist, setArtist] = useState(track.artist ?? "");
  const [album, setAlbum] = useState(track.album ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function save() {
    setSaving(true);
    setError(null);

    try {
      await ipc.correctTrack(
        track.id,
        title,
        artist.trim() === "" ? null : artist,
        album.trim() === "" ? null : album,
      );
      onCorrected();
      onClose();
    } catch (cause) {
      setError(String(cause));
      setSaving(false);
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-base/70 p-6 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="w-full max-w-md rounded-2xl bg-surface p-6 shadow-2xl shadow-black/60"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-4">
          <div>
            <p className="text-xs font-semibold uppercase tracking-[0.16em] text-ink-faint">
              Corriger
            </p>
            <h2 className="display mt-1 text-xl text-ink">Le bon nom</h2>
          </div>

          <button
            type="button"
            aria-label="Fermer"
            onClick={onClose}
            className="text-ink-faint transition-colors hover:text-ink"
          >
            <Icon name="close" size={18} />
          </button>
        </div>

        <div className="mt-5 space-y-3">
          <Field label="Titre" value={title} onChange={setTitle} autoFocus />
          <Field label="Artiste" value={artist} onChange={setArtist} />
          <Field label="Album" value={album} onChange={setAlbum} />
        </div>

        {error !== null && <p className="mt-3 text-[12px] text-danger">{error}</p>}

        <p className="mt-4 text-[11px] leading-relaxed text-ink-faint">
          Les tags sont réécrits dans le fichier. Les paroles actuelles sont
          effacées — elles appartenaient à l&apos;ancien titre — et Onzer ne
          reproposera plus la correspondance qu&apos;il avait trouvée.
        </p>

        <div className="mt-5 flex gap-3">
          <button
            type="button"
            disabled={saving || title.trim() === ""}
            onClick={() => void save()}
            className="rounded-full bg-ink px-5 py-2 text-[13px] font-semibold text-base transition-opacity hover:opacity-90 disabled:opacity-40"
          >
            {saving ? "Enregistrement…" : "Corriger"}
          </button>

          <button
            type="button"
            onClick={onClose}
            className="rounded-full px-5 py-2 text-[13px] text-ink-muted transition-colors hover:text-ink"
          >
            Annuler
          </button>
        </div>
      </div>
    </div>
  );
}

function Field({
  label,
  value,
  onChange,
  autoFocus = false,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  autoFocus?: boolean;
}) {
  return (
    <label className="block">
      <span className="block text-[11px] font-semibold uppercase tracking-[0.14em] text-ink-faint">
        {label}
      </span>
      <input
        type="text"
        value={value}
        autoFocus={autoFocus}
        spellCheck={false}
        onChange={(event) => onChange(event.target.value)}
        className="mt-1 h-10 w-full rounded-lg bg-base px-3 text-sm text-ink focus:outline focus:outline-1 focus:outline-accent"
      />
    </label>
  );
}
