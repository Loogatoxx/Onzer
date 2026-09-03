import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import { ipc, type RootSuggestions } from "@/lib/ipc";

/**
 * Premier lancement : choix de la racine de bibliothèque.
 *
 * # Pourquoi les propositions viennent du cœur
 *
 * Le chemin était écrit ici, en dur : « /Volumes/Lexar/Musique ». Sur un
 * téléphone, c'est un dossier qui n'existe pas, proposé par un bouton qui ne
 * pouvait qu'échouer. Seul le cœur sait sur quelle machine il tourne.
 *
 * # Pourquoi le sélecteur disparaît sur Android
 *
 * Le système n'y rend pas un chemin de fichier mais une autorisation d'accès à
 * un arbre de documents, que le scanner ne sait pas lire. Le bouton restait
 * donc sans effet : un clic, rien, et le doute sur tout le reste. Tant qu'il
 * n'y a rien derrière, mieux vaut ne pas l'afficher.
 */
export function SetupView({ onReady }: { onReady: () => void }) {
  const [suggestions, setSuggestions] = useState<RootSuggestions | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void ipc
      .suggestedRoots()
      .then(setSuggestions)
      .catch((cause: unknown) => setError(String(cause)));
  }, []);

  async function choose(path: string | null) {
    if (path === null) return;

    setBusy(true);
    setError(null);
    try {
      await ipc.setLibraryRoot(path);
      onReady();
    } catch (cause) {
      setError(String(cause));
      setBusy(false);
    }
  }

  async function browse() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Choisir le dossier de la bibliothèque",
    });
    await choose(typeof selected === "string" ? selected : null);
  }

  return (
    <div className="mx-auto w-full max-w-md text-center">
      <h2 className="text-xl font-semibold text-ink">Où ranger ta musique ?</h2>
      <p className="mt-2 text-sm leading-relaxed text-ink-muted">
        Onzer déplacera tes fichiers dans ce dossier et les rangera par artiste,
        année et album.
      </p>

      <div className="mt-7 space-y-2.5">
        {suggestions === null && (
          <p className="text-sm text-ink-faint">Recherche des dossiers…</p>
        )}

        {(suggestions?.roots ?? []).map((root, index) => (
          <button
            key={root}
            type="button"
            disabled={busy}
            onClick={() => void choose(root)}
            className={
              index === 0
                ? "w-full rounded-xl bouton-accent px-4 py-3 text-sm font-medium"
                : "w-full rounded-xl border border-line bg-surface px-4 py-3 text-sm text-ink transition-colors hover:bg-elevated disabled:opacity-40"
            }
          >
            {index === 0 ? `Utiliser ${root}` : root}
          </button>
        ))}

        {suggestions?.canBrowse === true && (
          <button
            type="button"
            disabled={busy}
            onClick={() => void browse()}
            className="w-full rounded-xl border border-line bg-surface px-4 py-3 text-sm text-ink transition-colors hover:bg-elevated disabled:opacity-40"
          >
            Choisir un autre dossier…
          </button>
        )}
      </div>

      {error !== null && (
        <p className="mt-4 rounded-lg border border-danger/25 bg-danger/5 px-3 py-2 text-xs text-danger">
          {error}
        </p>
      )}

      <p className="mt-6 text-xs leading-relaxed text-ink-faint">
        Le dossier est créé s&apos;il n&apos;existe pas encore. Seuls les
        fichiers audio y sont déplacés — ta base de données et tes statistiques
        restent à part.
      </p>
    </div>
  );
}
