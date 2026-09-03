import { useEffect, useState } from "react";

import { Icon } from "@/components/Icon";
import { ipc, type PairingInfo, type SyncReport } from "@/lib/ipc";

/**
 * Synchroniser deux appareils, sur le réseau local.
 *
 * # Pourquoi un écran à deux moitiés et non deux écrans
 *
 * Les deux appareils font la même chose : l'un ouvre une porte, l'autre y
 * entre. Lequel fait quoi n'a aucune importance — c'est le même code qui
 * fusionne des deux côtés. Deux écrans séparés obligeraient à choisir un rôle
 * avant de comprendre en quoi il consiste ; ici, on ouvre chez soi **ou** on
 * saisit ce que l'autre affiche, et la question ne se pose pas.
 *
 * # Ce qui ne traverse pas
 *
 * Les fichiers audio — ils sont déjà des deux côtés et pèsent des dizaines de
 * gigaoctets. Les titres et les albums non plus : corriger deux mille titres
 * d'un coup sans qu'on l'ait demandé est exactement ce qu'Onzer ne fait pas.
 */
export function PairingView() {
  return (
    <div className="px-6 pb-10 pt-6">
      <p className="text-xs font-semibold uppercase tracking-[0.16em] text-ink-muted">
        Deux appareils
      </p>
      <h1 className="display-xl mt-2 text-[clamp(2rem,5.5vw,4.25rem)] text-ink">
        Synchroniser
      </h1>
      <p className="mt-3 max-w-xl text-sm leading-relaxed text-ink-muted">
        Les favoris, les playlists et les paroles passent d&apos;un appareil à
        l&apos;autre, par le Wi-Fi de la maison. Rien ne sort d&apos;ici : pas
        de compte, pas de serveur, pas d&apos;intermédiaire.
      </p>

      <div className="mt-8 grid max-w-4xl gap-3 lg:grid-cols-2">
        <Recevoir />
        <SeConnecter />
      </div>
    </div>
  );
}

// ════════════════════════════════════════════════════════════════════════════
//  Ouvrir la porte
// ════════════════════════════════════════════════════════════════════════════

function Recevoir() {
  const [infos, setInfos] = useState<PairingInfo | null>(null);
  const [erreur, setErreur] = useState<string | null>(null);
  const [occupe, setOccupe] = useState(false);

  // # Pourquoi la fermeture est un nettoyage et non un bouton
  //
  // Une porte qu'on oublie de fermer reste ouverte. La rattacher à la vie du
  // composant garantit qu'elle se referme en changeant de page, en fermant la
  // fenêtre, ou simplement en passant à autre chose — c'est-à-dire dans tous
  // les cas où l'on aurait oublié de cliquer.
  useEffect(() => {
    return () => {
      void ipc.closePairing().catch(() => undefined);
    };
  }, []);

  async function ouvrir() {
    setOccupe(true);
    setErreur(null);
    try {
      setInfos(await ipc.openPairing());
    } catch (cause) {
      setErreur(String(cause));
    } finally {
      setOccupe(false);
    }
  }

  function fermer() {
    setInfos(null);
    void ipc.closePairing().catch(() => undefined);
  }

  return (
    <section className="rounded-xl bg-surface p-5">
      <h2 className="text-sm font-semibold text-ink">Recevoir</h2>
      <p className="mt-1 text-[13px] leading-relaxed text-ink-muted">
        Ouvre une porte le temps de l&apos;échange. L&apos;autre appareil
        scanne le QR, ou recopie les huit chiffres.
      </p>

      {infos === null ? (
        <button
          type="button"
          disabled={occupe}
          onClick={() => void ouvrir()}
          className="pression mt-5 w-full rounded-xl bg-gradient-to-br from-accent to-accent-alt px-4 py-3 text-sm font-medium text-base transition-opacity hover:opacity-90 disabled:opacity-40"
        >
          {occupe ? "Ouverture…" : "Ouvrir la porte"}
        </button>
      ) : (
        <div className="mt-5">
          <CodeQR matrice={infos.qr} />

          <p className="numerals mt-5 text-center text-[28px] font-semibold tracking-[0.2em] text-ink">
            {infos.code.slice(0, 4)} {infos.code.slice(4)}
          </p>
          <p className="mt-1 text-center font-mono text-[12px] text-ink-faint">
            {infos.hote}:{infos.port}
          </p>

          {/* # Pourquoi les autres adresses sont montrées
              Une machine en a souvent plusieurs — le Wi-Fi et la carte SIM sur
              un téléphone, le Wi-Fi et le câble sur un ordinateur. On affiche
              la plus probable, mais « probable » n'est pas « certaine » : si
              l'autre appareil ne répond pas, la bonne est peut-être juste en
              dessous. */}
          {infos.autres.length > 0 && (
            <p className="mt-1.5 text-center font-mono text-[11px] leading-relaxed text-ink-faint/70">
              ou {infos.autres.map((autre) => `${autre}:${infos.port}`).join(" · ")}
            </p>
          )}

          <p className="mt-4 rounded-lg bg-elevated px-3 py-2 text-[12px] leading-relaxed text-ink-faint">
            La porte se referme d&apos;elle-même en quittant cette page. Cinq
            codes erronés la referment aussi.
          </p>

          <button
            type="button"
            onClick={fermer}
            className="pression mt-3 w-full rounded-xl border border-line px-4 py-2.5 text-sm text-ink-muted transition-colors hover:bg-elevated hover:text-ink"
          >
            Refermer maintenant
          </button>
        </div>
      )}

      {erreur !== null && <Erreur texte={erreur} />}
    </section>
  );
}

/**
 * Le QR, dessiné en SVG.
 *
 * # Pourquoi pas une image
 *
 * Le cœur rend une matrice de booléens ; en faire un PNG demanderait un
 * encodeur, une taille à choisir, et donnerait une image floue dès qu'on
 * l'agrandit. Un carré par point, en SVG, est net à toutes les tailles et tient
 * en dix lignes.
 */
function CodeQR({ matrice }: { matrice: boolean[][] }) {
  if (matrice.length === 0) return null;

  const cote = matrice.length;
  // Une marge blanche de quatre modules : les lecteurs de QR en ont besoin pour
  // trouver les bords. Sans elle, beaucoup refusent simplement de lire.
  const marge = 4;
  const total = cote + marge * 2;

  return (
    <svg
      viewBox={`0 0 ${total} ${total}`}
      role="img"
      aria-label="Code à scanner"
      className="mx-auto block w-full max-w-[240px] rounded-lg bg-white p-2"
    >
      {matrice.map((ligne, y) =>
        ligne.map((point, x) =>
          point ? (
            <rect
              key={`${x}-${y}`}
              x={x + marge}
              y={y + marge}
              width={1}
              height={1}
              fill="#08080a"
            />
          ) : null,
        ),
      )}
    </svg>
  );
}

// ════════════════════════════════════════════════════════════════════════════
//  Entrer chez l'autre
// ════════════════════════════════════════════════════════════════════════════

function SeConnecter() {
  const [hote, setHote] = useState("");
  const [code, setCode] = useState("");
  const [occupe, setOccupe] = useState(false);
  const [erreur, setErreur] = useState<string | null>(null);
  const [rapport, setRapport] = useState<SyncReport | null>(null);

  /**
   * Un lien collé remplit les deux champs.
   *
   * C'est ce qu'on obtient en scannant le QR avec l'appareil photo du
   * téléphone : il ouvre un lien qu'on peut copier. Le reconnaître ici évite
   * d'avoir à demander la caméra — une permission de plus pour faire ce que le
   * téléphone sait déjà faire.
   */
  async function coller(texte: string) {
    setHote(texte);
    if (!texte.includes("onzer://")) return;

    const lu = await ipc.readPairingLink(texte).catch(() => null);
    if (lu === null) return;

    setHote(lu.host);
    setCode(lu.code);
  }

  async function lancer() {
    setOccupe(true);
    setErreur(null);
    setRapport(null);

    try {
      // L'adresse peut porter son port : « 192.168.1.42:47813 ». Il n'y en a
      // qu'un d'habitude, mais quand le port habituel est pris, l'autre
      // appareil en affiche un autre — et il faut bien pouvoir le saisir.
      const [adresse, portEcrit] = hote.trim().split(":");
      const port = Number.parseInt(portEcrit ?? "", 10);

      setRapport(
        await ipc.syncWithDevice(adresse ?? "", Number.isNaN(port) ? 47812 : port, code),
      );
    } catch (cause) {
      setErreur(String(cause));
    } finally {
      setOccupe(false);
    }
  }

  const pret = hote.trim() !== "" && code.replace(/\D/g, "").length === 8;

  return (
    <section className="rounded-xl bg-surface p-5">
      <h2 className="text-sm font-semibold text-ink">Se connecter</h2>
      <p className="mt-1 text-[13px] leading-relaxed text-ink-muted">
        Recopie ce que l&apos;autre appareil affiche. Ou colle le lien du QR :
        les deux champs se remplissent seuls.
      </p>

      <label className="mt-5 block text-[11px] font-semibold uppercase tracking-[0.14em] text-ink-faint">
        Adresse
      </label>
      <input
        value={hote}
        placeholder="192.168.1.42"
        onChange={(event) => void coller(event.target.value)}
        className="mt-1.5 w-full rounded-lg border border-line bg-base px-3 py-2.5 font-mono text-[13px] text-ink placeholder:text-ink-faint focus:border-accent focus:outline-none"
      />

      <label className="mt-4 block text-[11px] font-semibold uppercase tracking-[0.14em] text-ink-faint">
        Code
      </label>
      <input
        inputMode="numeric"
        value={code}
        placeholder="1234 5678"
        onChange={(event) => setCode(event.target.value)}
        className="numerals mt-1.5 w-full rounded-lg border border-line bg-base px-3 py-2.5 text-[15px] tracking-[0.15em] text-ink placeholder:text-ink-faint focus:border-accent focus:outline-none"
      />

      <button
        type="button"
        disabled={!pret || occupe}
        onClick={() => void lancer()}
        className="pression mt-5 w-full rounded-xl bg-gradient-to-br from-accent to-accent-alt px-4 py-3 text-sm font-medium text-base transition-opacity hover:opacity-90 disabled:opacity-40"
      >
        {occupe ? "Échange en cours…" : "Synchroniser"}
      </button>

      {rapport !== null && <Resultat rapport={rapport} />}
      {erreur !== null && <Erreur texte={erreur} />}
    </section>
  );
}

function Resultat({ rapport }: { rapport: SyncReport }) {
  const rien =
    rapport.favoris === 0 && rapport.paroles === 0 && rapport.playlists === 0;

  return (
    <div className="mt-4 rounded-lg border border-accent/25 bg-accent/5 px-3.5 py-3 text-[13px] leading-relaxed text-ink">
      <p className="font-medium">
        {rien ? "Déjà d'accord" : `Repris de ${rapport.appareil}`}
      </p>

      {rien ? (
        <p className="mt-1 text-ink-muted">
          Les deux bibliothèques disaient déjà la même chose.
        </p>
      ) : (
        <ul className="mt-1.5 space-y-0.5 text-ink-muted">
          {rapport.favoris > 0 && (
            <li>
              {rapport.favoris} favori{rapport.favoris > 1 ? "s" : ""}
            </li>
          )}
          {rapport.paroles > 0 && (
            <li>
              {rapport.paroles} morceau{rapport.paroles > 1 ? "x" : ""} avec des
              paroles
            </li>
          )}
          {rapport.playlists > 0 && (
            <li>
              {rapport.playlists} playlist{rapport.playlists > 1 ? "s" : ""}
            </li>
          )}
        </ul>
      )}

      {rapport.arbitrages > 0 && (
        <p className="mt-2 text-[12px] text-ink-faint">
          {rapport.arbitrages} désaccord{rapport.arbitrages > 1 ? "s" : ""} —
          le plus récent a gagné, et ce qui a été remplacé est consigné.
        </p>
      )}
    </div>
  );
}

function Erreur({ texte }: { texte: string }) {
  return (
    <p className="mt-4 flex items-start gap-2 rounded-lg border border-danger/25 bg-danger/5 px-3 py-2.5 text-[12px] leading-relaxed text-danger">
      <span className="mt-0.5 shrink-0">
        <Icon name="close" size={13} />
      </span>
      {texte}
    </p>
  );
}
