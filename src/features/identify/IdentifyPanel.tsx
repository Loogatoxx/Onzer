import { useEffect, useState } from "react";

import { ipc, type IdentificationStatus } from "@/lib/ipc";

/**
 * Panneau d'identification par empreinte acoustique.
 *
 * # Le défaut que cette version corrige
 *
 * La précédente masquait tout dès que la clé était renseignée : plus moyen de
 * la revoir, de la corriger, ni de savoir pourquoi l'ouvrier n'avançait pas.
 * Une clé mal collée enfermait l'utilisateur devant une jauge figée à 0/43,
 * **sans aucune porte de sortie**.
 *
 * Trois règles en découlent, valables bien au-delà de ce panneau :
 *
 * 1. Un réglage saisi une fois doit rester **modifiable** ;
 * 2. Un travail qui n'avance pas doit **dire pourquoi** ;
 * 3. Une valeur secrète doit rester **vérifiable** sans être révélée — d'où
 *    l'aperçu masqué, qui permet de repérer un collage tronqué.
 */
const POLL_MS = 4000;

export function IdentifyPanel() {
  const [status, setStatus] = useState<IdentificationStatus | null>(null);
  const [editing, setEditing] = useState(false);
  const [key, setKey] = useState("");
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    const read = () => {
      void ipc.identificationStatus().then(setStatus).catch(() => undefined);
    };

    read();
    const timer = setInterval(read, POLL_MS);
    return () => clearInterval(timer);
  }, []);

  async function save() {
    setSaveError(null);
    try {
      await ipc.setAcoustidKey(key);
      setKey("");
      setEditing(false);
      setStatus(await ipc.identificationStatus());
    } catch (cause) {
      setSaveError(String(cause));
    }
  }

  if (status === null) return null;

  const { progress } = status;
  const settled = progress.identified + progress.notFound + progress.failed;
  const ratio = progress.total === 0 ? 0 : settled / progress.total;
  const running = status.configured && progress.pending > 0;

  return (
    <div className="rounded-xl border border-line bg-surface/60 px-3 py-2">
      {/* Ligne toujours présente : l'accès à la clé ne disparaît jamais. */}
      <div className="flex items-center gap-2.5">
        <Wand active={running} />

        <div className="min-w-0 flex-1">
          {status.configured ? (
            <p className="truncate text-xs text-ink">
              Identification automatique
              <span className="mx-1.5 text-ink-faint">·</span>
              <span className="font-mono text-ink-faint">{status.keyHint}</span>
            </p>
          ) : (
            <p className="truncate text-xs text-ink">
              Retrouver automatiquement titres, albums et pochettes
            </p>
          )}

          <p className="truncate text-[11px] text-ink-faint">
            {status.configured
              ? `${settled}/${progress.total} traités${
                  progress.notFound > 0 ? ` · ${progress.notFound} introuvable(s)` : ""
                }`
              : "Onzer reconnaît tes morceaux au son. Une clé gratuite suffit."}
          </p>
        </div>

        <button
          type="button"
          onClick={() => {
            setKey("");
            setEditing((open) => !open);
          }}
          className="shrink-0 rounded-lg border border-line px-2.5 py-1 text-[11px] text-ink-muted transition-colors hover:border-accent/50 hover:text-ink"
        >
          {editing ? "Fermer" : status.configured ? "Modifier la clé" : "Configurer"}
        </button>
      </div>

      {/* Barre de progression, tant qu'il reste du travail. */}
      {running && (
        <div className="mt-2 h-1 overflow-hidden rounded-full bg-elevated">
          <div
            className="h-full rounded-full bg-gradient-to-r from-accent to-accent-alt transition-[width] duration-500"
            style={{ width: `${ratio * 100}%` }}
          />
        </div>
      )}

      {/* La raison du blocage, plutôt qu'une jauge figée sans explication. */}
      {status.lastError !== null && !editing && (
        <div className="mt-2 rounded-lg border border-warn/25 bg-warn/5 px-2.5 py-1.5">
          <p className="text-[11px] leading-relaxed text-warn">{status.lastError}</p>
          <Remedy error={status.lastError} onFix={() => setEditing(true)} />
        </div>
      )}

      {/* Saisie de la clé. */}
      {editing && (
        <div className="mt-2.5 border-t border-line pt-2.5">
          <p className="text-[11px] leading-relaxed text-ink-muted">
            AcoustID distribue <span className="text-ink">deux clés distinctes</span>.
            Celle attendue ici est la{" "}
            <span className="text-ink">clé d'application</span>, créée sur{" "}
            <span className="font-mono text-ink-faint">
              acoustid.org/new-application
            </span>
            . Ce n'est pas la clé personnelle affichée sur ta page de profil,
            qui ne sert qu'à soumettre des empreintes.
          </p>

          <div className="mt-2 flex items-center gap-2">
            <input
              type="text"
              value={key}
              autoFocus
              spellCheck={false}
              autoCorrect="off"
              autoCapitalize="off"
              onChange={(event) => setKey(event.target.value.trim())}
              onKeyDown={(event) => {
                if (event.key === "Enter") void save();
                if (event.key === "Escape") setEditing(false);
              }}
              placeholder={status.configured ? "nouvelle clé" : "clé AcoustID"}
              className="flex-1 rounded-lg border border-line bg-base px-3 py-1.5 font-mono text-xs text-ink placeholder:text-ink-faint focus:border-accent focus:outline-none"
            />
            <button
              type="button"
              disabled={key.length === 0}
              onClick={() => void save()}
              className="rounded-lg bg-gradient-to-br from-accent to-accent-alt px-3 py-1.5 text-xs font-medium text-base transition-opacity hover:opacity-90 disabled:opacity-40"
            >
              Enregistrer
            </button>
          </div>

          {status.configured && (
            <button
              type="button"
              onClick={() => {
                setKey("");
                void ipc.setAcoustidKey("").then(async () => {
                  setEditing(false);
                  setStatus(await ipc.identificationStatus());
                });
              }}
              className="mt-2 text-[11px] text-ink-faint transition-colors hover:text-danger"
            >
              Retirer la clé enregistrée
            </button>
          )}

          {saveError !== null && (
            <p className="mt-2 text-[11px] text-danger">{saveError}</p>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * Traduit une erreur de service en geste à faire.
 *
 * Afficher « 400 Bad Request » ne sert à rien ; dire quelle clé aller chercher,
 * si.
 */
function Remedy({ error, onFix }: { error: string; onFix: () => void }) {
  const lowered = error.toLowerCase();

  if (lowered.includes("api key")) {
    return (
      <p className="mt-1 text-[11px] leading-relaxed text-ink-muted">
        AcoustID distribue deux clés. Celle attendue ici est la{" "}
        <span className="text-ink">clé d'application</span>, créée sur
        acoustid.org/new-application — et non la clé personnelle de ta page de
        profil.{" "}
        <button
          type="button"
          onClick={onFix}
          className="text-accent underline underline-offset-2"
        >
          Corriger
        </button>
      </p>
    );
  }

  if (lowered.includes("injoignable") || lowered.includes("timeout")) {
    return (
      <p className="mt-1 text-[11px] text-ink-muted">
        Vérifie ta connexion. L'identification reprendra toute seule.
      </p>
    );
  }

  return null;
}

/** Baguette : le geste « magique » de l'identification. */
function Wand({ active }: { active: boolean }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className={`h-4 w-4 shrink-0 ${active ? "animate-pulse text-accent" : "text-ink-faint"}`}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="m15 4 5 5L9 20H4v-5z" />
      <path d="M14 5 19 10" />
      <path d="M5 3v4M3 5h4" />
    </svg>
  );
}
