import { useEffect, useState } from "react";

import { Icon } from "@/components/Icon";
import { ipc, type Preferences, type RebuildReport } from "@/lib/ipc";

/**
 * Réglages.
 *
 * # Pourquoi une page et non un menu
 *
 * Deux réglages seulement, mais l'un d'eux vide la bibliothèque. Une action de
 * cette portée n'a rien à faire dans un menu déroulant, où l'on clique vite :
 * elle mérite une page, une explication et une confirmation.
 */
export function SettingsView({ onChanged }: { onChanged: () => void }) {
  const [preferences, setPreferences] = useState<Preferences | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void ipc.preferences().then(setPreferences).catch(() => undefined);
  }, []);

  async function toggle(enabled: boolean) {
    setError(null);
    setPreferences({ onlineCompletion: enabled });

    try {
      await ipc.setOnlineCompletion(enabled);
      onChanged();
    } catch (cause) {
      setError(String(cause));
      setPreferences({ onlineCompletion: !enabled });
    }
  }

  return (
    <div className="px-6 pb-10 pt-6">
      <p className="text-xs font-semibold uppercase tracking-[0.16em] text-ink-muted">
        Onzer
      </p>
      <h1 className="display-xl mt-2 text-[clamp(2rem,5.5vw,4.25rem)] text-ink">
        Réglages
      </h1>

      <div className="mt-8 max-w-2xl space-y-3">
        <Setting
          title="Compléter les métadonnées en ligne"
          description="Identification acoustique, paroles, pochettes et albums manquants. À laisser allumé quand les fichiers arrivent mal tagués — à éteindre quand ils arrivent déjà complets, d'un service qui fournit ses métadonnées."
          checked={preferences?.onlineCompletion ?? true}
          onChange={(value) => void toggle(value)}
        />

        {preferences !== null && !preferences.onlineCompletion && (
          <p className="px-4 text-[12px] leading-relaxed text-ink-faint">
            Les outils de complétion sont masqués et le cœur refuse de les
            lancer. Rien n&apos;est perdu : rallumer l&apos;interrupteur les
            remet exactement où ils étaient.
          </p>
        )}

        {error !== null && <p className="px-4 text-[12px] text-danger">{error}</p>}
      </div>

      <RebuildSection onDone={onChanged} />
    </div>
  );
}

function Setting({
  title,
  description,
  checked,
  onChange,
}: {
  title: string;
  description: string;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <div className="flex items-start gap-4 rounded-xl bg-surface p-4">
      <div className="min-w-0 flex-1">
        <p className="text-[14px] font-medium text-ink">{title}</p>
        <p className="mt-1 text-[12px] leading-relaxed text-ink-muted">{description}</p>
      </div>

      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={title}
        onClick={() => onChange(!checked)}
        className={`mt-1 flex h-6 w-11 shrink-0 items-center rounded-full px-0.5 transition-colors ${
          checked ? "bg-accent" : "bg-raised"
        }`}
      >
        <span
          className={`h-5 w-5 rounded-full bg-ink transition-transform ${
            checked ? "translate-x-5" : ""
          }`}
        />
      </button>
    </div>
  );
}

/**
 * Remise à zéro de la bibliothèque.
 *
 * # Pourquoi deux clics et non un
 *
 * L'action est de celles qu'on ne veut pas déclencher par erreur. Le premier
 * clic n'exécute rien : il **montre ce qui va se passer**, et c'est cette
 * énumération — ce qui part, ce qui reste, où vont les fichiers — qui permet
 * de dire oui en connaissance de cause.
 */
function RebuildSection({ onDone }: { onDone: () => void }) {
  const [armed, setArmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<RebuildReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function rebuild() {
    setBusy(true);
    setError(null);

    try {
      setReport(await ipc.rebuildLibrary());
      setArmed(false);
      onDone();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="mt-10 max-w-2xl">
      <h2 className="display text-lg text-ink">Repartir de zéro</h2>
      <p className="mt-2 text-[13px] leading-relaxed text-ink-muted">
        Vide la bibliothèque pour la reconstruire depuis le dépôt. À faire quand
        on remplace tous ses fichiers par de meilleurs : corriger l&apos;ancien
        un par un serait plus long que tout reprendre.
      </p>

      <div className="mt-4 rounded-xl bg-surface p-4">
        <div className="grid gap-2 text-[12px] sm:grid-cols-2">
          <Column
            tone="danger"
            title="Ce qui part"
            items={[
              "Tous les morceaux de la bibliothèque",
              "Le contenu des playlists",
              "Albums et artistes devenus vides",
            ]}
          />
          <Column
            tone="calm"
            title="Ce qui reste"
            items={[
              "Les fichiers, déplacés dans « _Ancien »",
              "L'historique d'écoute et les statistiques",
              "Les playlists : nom, image, description",
            ]}
          />
        </div>

        <p className="mt-4 text-[11px] leading-relaxed text-ink-faint">
          Aucun fichier n&apos;est supprimé du disque. L&apos;ancienne
          bibliothèque est déplacée telle quelle dans un dossier «&nbsp;_Ancien&nbsp;»
          à la racine, que tu jettes toi-même quand tu es sûr. Le dépôt
          «&nbsp;_Inbox&nbsp;» n&apos;est pas touché : c&apos;est là que les
          nouveaux fichiers attendent.
        </p>

        {report !== null && (
          <p className="mt-4 rounded-lg bg-elevated px-3 py-2 text-[12px] text-ink">
            {report.tracks} morceau{report.tracks > 1 ? "x" : ""} retiré
            {report.tracks > 1 ? "s" : ""}, {report.foldersSetAside} dossier
            {report.foldersSetAside > 1 ? "s" : ""} mis de côté. Dépose
            maintenant tes fichiers dans «&nbsp;_Inbox&nbsp;» : Onzer les range
            tout seul.
          </p>
        )}

        {error !== null && <p className="mt-3 text-[12px] text-danger">{error}</p>}

        <div className="mt-4 flex items-center gap-3">
          {!armed ? (
            <button
              type="button"
              onClick={() => setArmed(true)}
              className="flex items-center gap-2 rounded-full bg-raised px-4 py-2 text-[13px] font-semibold text-ink-muted transition-colors hover:text-danger"
            >
              <Icon name="trash" size={15} />
              Vider la bibliothèque
            </button>
          ) : (
            <>
              <button
                type="button"
                disabled={busy}
                onClick={() => void rebuild()}
                className="rounded-full bg-danger px-5 py-2 text-[13px] font-semibold text-base transition-opacity hover:opacity-90 disabled:opacity-40"
              >
                {busy ? "En cours…" : "Oui, tout retirer"}
              </button>

              <button
                type="button"
                onClick={() => setArmed(false)}
                className="rounded-full px-4 py-2 text-[13px] text-ink-muted transition-colors hover:text-ink"
              >
                Annuler
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function Column({
  tone,
  title,
  items,
}: {
  tone: "danger" | "calm";
  title: string;
  items: string[];
}) {
  return (
    <div>
      <p
        className={`text-[11px] font-semibold uppercase tracking-[0.14em] ${
          tone === "danger" ? "text-danger" : "text-ink-faint"
        }`}
      >
        {title}
      </p>
      <ul className="mt-1.5 space-y-1 text-ink-muted">
        {items.map((item) => (
          <li key={item} className="flex gap-2">
            <span className="text-ink-faint">·</span>
            {item}
          </li>
        ))}
      </ul>
    </div>
  );
}
