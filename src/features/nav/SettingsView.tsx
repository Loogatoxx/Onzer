import { useEffect, useState } from "react";

import { Icon } from "@/components/Icon";
import { ipc, type Preferences, type RebuildReport, type ScanSummary } from "@/lib/ipc";

/**
 * Réglages.
 *
 * # Pourquoi une page et non un menu
 *
 * Deux réglages seulement, mais l'un d'eux vide la bibliothèque. Une action de
 * cette portée n'a rien à faire dans un menu déroulant, où l'on clique vite :
 * elle mérite une page, une explication et une confirmation.
 */
export function SettingsView({
  onChanged,
  libraryRoot,
}: {
  onChanged: () => void;
  /** D'où viennent les fichiers. Affiché ici et nulle part ailleurs. */
  libraryRoot: string;
}) {
  const [preferences, setPreferences] = useState<Preferences | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void ipc.preferences().then(setPreferences).catch(() => undefined);
  }, []);

  async function toggle(key: keyof Preferences, enabled: boolean) {
    if (preferences === null) return;

    setError(null);
    setPreferences({ ...preferences, [key]: enabled });

    try {
      if (key === "onlineCompletion") await ipc.setOnlineCompletion(enabled);
      else await ipc.setAutoIdentification(enabled);
      onChanged();
    } catch (cause) {
      setError(String(cause));
      setPreferences(preferences);
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
        <DossierSetting chemin={libraryRoot} />

        <RelireSetting racine={libraryRoot} onChanged={onChanged} />

        <NameSetting
          value={preferences?.displayName ?? ""}
          onSave={(name) => {
            if (preferences === null) return;
            setPreferences({ ...preferences, displayName: name });
            void ipc
              .setDisplayName(name)
              .then(onChanged)
              .catch((cause: unknown) => setError(String(cause)));
          }}
        />

        <Setting
          title="Compléter les métadonnées en ligne"
          description="Paroles, pochettes et albums manquants. À laisser allumé quand les fichiers arrivent incomplets — à éteindre quand ils arrivent déjà complets, d'un service qui fournit ses métadonnées."
          checked={preferences?.onlineCompletion ?? true}
          onChange={(value) => void toggle("onlineCompletion", value)}
        />

        <Setting
          title="Identification automatique"
          description="L'ouvrier qui reconnaît les morceaux à l'oreille et réécrit leurs titres. Inutile sur des fichiers déjà correctement tagués — et c'est le seul outil qui modifie un titre sans qu'on le lui demande."
          checked={preferences?.autoIdentification ?? true}
          onChange={(value) => void toggle("autoIdentification", value)}
        />

        {preferences !== null && (!preferences.onlineCompletion || !preferences.autoIdentification) && (
          <p className="px-4 text-[12px] leading-relaxed text-ink-faint">
            Ce qui est éteint disparaît de l&apos;interface, et le cœur refuse
            de le lancer. Rien n&apos;est perdu : rallumer l&apos;interrupteur
            remet l&apos;outil exactement où il était. La synchronisation des
            paroles depuis les fichiers, elle, marche dans tous les cas —
            elle ne parle à personne.
          </p>
        )}

        {error !== null && <p className="px-4 text-[12px] text-danger">{error}</p>}
      </div>

      <RebuildSection onDone={onChanged} />
    </div>
  );
}

/**
 * Le prénom employé par l'accueil.
 *
 * # Pourquoi il est facultatif
 *
 * « Bonsoir Carlos » n'est chaleureux que si c'est bien son nom. Le demander
 * avant de laisser entrer serait une formalité de plus ; l'inventer serait
 * pire. Vide, l'accueil dit simplement « Bonsoir ».
 */
/**
 * D'où viennent les fichiers.
 *
 * # Pourquoi ce chemin a quitté la bibliothèque
 *
 * Il s'affichait au-dessus de la liste des morceaux : « /storage/emulated/0/
 * Musique », un chemin de machine posé au milieu d'un lecteur de musique. On le
 * consulte une fois — après un import, quand on doute de l'endroit — et jamais
 * plus. Sa place est ici, à côté du bouton qui le change.
 */
function DossierSetting({ chemin }: { chemin: string }) {
  return (
    <div className="rounded-xl bg-surface px-4 py-3.5">
      <p className="text-sm font-medium text-ink">Dossier de la musique</p>
      <p className="mt-1 break-all font-mono text-[11px] leading-relaxed text-ink-faint">
        {chemin}
      </p>
    </div>
  );
}

/**
 * Relire le dossier de musique.
 *
 * # Pourquoi ce bouton manquait sur téléphone
 *
 * Sur le Mac, « Importer » vit dans la barre du haut — qui n'existe pas sur un
 * écran étroit. Le téléphone n'avait donc **aucun** moyen de découvrir un
 * fichier déposé dans son dossier de musique après le premier démarrage : on
 * copiait de nouveaux morceaux, et Onzer continuait d'afficher les anciens.
 *
 * # Pourquoi c'est le même chemin que l'import
 *
 * La commande d'import accepte la bibliothèque elle-même comme source, et ne
 * déplace pas un fichier déjà à sa place. Écrire un « scan » séparé ferait une
 * seconde façon d'entrer dans la bibliothèque, avec ses propres oublis.
 */
function RelireSetting({
  racine,
  onChanged,
}: {
  racine: string;
  onChanged: () => void;
}) {
  const [occupe, setOccupe] = useState(false);
  const [bilan, setBilan] = useState<ScanSummary | null>(null);
  const [erreur, setErreur] = useState<string | null>(null);

  async function relire() {
    setOccupe(true);
    setErreur(null);
    setBilan(null);

    try {
      setBilan(await ipc.importFolder(racine));
      onChanged();
    } catch (cause) {
      setErreur(String(cause));
    } finally {
      setOccupe(false);
    }
  }

  return (
    <div className="rounded-xl bg-surface px-4 py-3.5">
      <p className="text-sm font-medium text-ink">Chercher de nouveaux morceaux</p>
      <p className="mt-1 text-[13px] leading-relaxed text-ink-muted">
        Relit le dossier de musique et ajoute ce qui ne s&apos;y trouvait pas
        encore. À lancer après avoir déposé des fichiers soi-même.
      </p>

      <button
        type="button"
        disabled={occupe}
        onClick={() => void relire()}
        className="pression mt-3 rounded-lg border border-line px-4 py-2 text-[13px] text-ink transition-colors hover:bg-elevated disabled:opacity-40"
      >
        {occupe ? "Lecture du dossier…" : "Relire le dossier"}
      </button>

      {bilan !== null && (
        <p className="mt-2.5 text-[12px] leading-relaxed text-ink-muted">
          {bilan.imported === 0
            ? "Rien de nouveau : tout ce qui est dans le dossier est déjà dans la bibliothèque."
            : `${bilan.imported} morceau${bilan.imported > 1 ? "x" : ""} ajouté${bilan.imported > 1 ? "s" : ""}.`}
          {bilan.duplicates > 0 && ` ${bilan.duplicates} déjà connu${bilan.duplicates > 1 ? "s" : ""}.`}
          {bilan.failed > 0 && ` ${bilan.failed} illisible${bilan.failed > 1 ? "s" : ""}.`}
        </p>
      )}

      {erreur !== null && (
        <p className="mt-2.5 rounded-lg border border-danger/25 bg-danger/5 px-3 py-2 text-[12px] text-danger">
          {erreur}
        </p>
      )}
    </div>
  );
}

function NameSetting({
  value,
  onSave,
}: {
  value: string;
  onSave: (name: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  const [known, setKnown] = useState(value);

  // La valeur arrive après le premier rendu : on s'aligne dessus tant que
  // l'utilisateur n'a rien tapé.
  if (value !== known) {
    setKnown(value);
    setDraft(value);
  }

  return (
    <div className="rounded-xl bg-surface p-4">
      <p className="text-[14px] font-medium text-ink">Ton prénom</p>
      <p className="mt-1 text-[12px] leading-relaxed text-ink-muted">
        Employé par l&apos;accueil : « Bonsoir {draft.trim() === "" ? "…" : draft.trim()} ».
        Laisse vide pour qu&apos;il dise seulement « Bonsoir ».
      </p>

      <input
        type="text"
        value={draft}
        spellCheck={false}
        placeholder="Carlos"
        onChange={(event) => setDraft(event.target.value)}
        onBlur={() => onSave(draft.trim())}
        onKeyDown={(event) => {
          if (event.key === "Enter") event.currentTarget.blur();
        }}
        className="mt-3 h-10 w-full max-w-xs rounded-lg bg-base px-3 text-sm text-ink placeholder:text-ink-faint focus:outline focus:outline-1 focus:outline-accent"
      />
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
