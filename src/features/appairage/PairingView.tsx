import { useEffect, useState } from "react";

import { Icon } from "@/components/Icon";
import { ScannerQR } from "./ScannerQR";
import {
  ipc,
  type PairingInfo,
  type SyncReport,
  type TransferProgress,
  type TransferReport,
} from "@/lib/ipc";

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
export function PairingView({ onSynced }: { onSynced: () => void }) {
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
        <SeConnecter onSynced={onSynced} />
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

function SeConnecter({ onSynced }: { onSynced: () => void }) {
  const [hote, setHote] = useState("");
  const [code, setCode] = useState("");
  const [occupe, setOccupe] = useState(false);
  const [erreur, setErreur] = useState<string | null>(null);
  const [rapport, setRapport] = useState<SyncReport | null>(null);
  const [scanne, setScanne] = useState(false);
  const [transfert, setTransfert] = useState<TransferProgress | null>(null);
  const [bilanTransfert, setBilanTransfert] = useState<TransferReport | null>(null);

  // L'avancement vient du cœur : c'est lui qui télécharge, un fichier à la
  // fois, et lui seul sait où il en est.
  useEffect(() => {
    const abonnement = ipc.onSyncTransfer(setTransfert);
    return () => {
      void abonnement.then((arreter) => arreter());
    };
  }, []);

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

    // Le port voyage dans le lien : le perdre obligerait à le retaper le jour
    // où le port habituel est pris.
    setHote(lu.port === 47812 ? lu.host : `${lu.host}:${lu.port}`);
    setCode(lu.code);
  }

  async function lancer() {
    setOccupe(true);
    setErreur(null);
    setRapport(null);

    try {
      const { adresse, port } = destination();
      setRapport(await ipc.syncWithDevice(adresse, port, code));
      // La base vient de changer : sans ce rappel, l'écran garderait
      // l'ancienne vérité jusqu'au prochain démarrage.
      onSynced();
    } catch (cause) {
      setErreur(String(cause));
    } finally {
      setOccupe(false);
    }
  }

  /**
   * L'adresse peut porter son port : « 192.168.1.42:47813 ».
   *
   * Il n'y en a qu'un d'habitude, mais quand le port habituel est pris,
   * l'autre appareil en affiche un autre — et il faut bien pouvoir le saisir.
   */
  function destination() {
    const [adresse, portEcrit] = hote.trim().split(":");
    const port = Number.parseInt(portEcrit ?? "", 10);

    return { adresse: adresse ?? "", port: Number.isNaN(port) ? 47812 : port };
  }

  async function rapatrier() {
    if (rapport === null) return;

    setErreur(null);
    setTransfert({ fait: 0, total: rapport.manquants.length, titre: "" });

    try {
      const { adresse, port } = destination();
      const bilan = await ipc.fetchMissingFiles(adresse, port, code, rapport.manquants);

      setBilanTransfert(bilan);
      // Les morceaux reçus ne sont plus manquants : reproposer de les
      // télécharger les ferait descendre une seconde fois.
      setRapport({ ...rapport, manquants: [], octetsManquants: 0 });
      onSynced();
    } catch (cause) {
      setErreur(String(cause));
    } finally {
      setTransfert(null);
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

      {/* # Pourquoi le bouton n'apparaît pas toujours
          Un ordinateur de bureau sans caméra le proposerait pour rien. Et sur
          une page qui ne serait pas un contexte sûr, `mediaDevices` n'existe
          même pas — mieux vaut ne rien montrer qu'un bouton qui échoue. */}
      {typeof navigator !== "undefined" && navigator.mediaDevices !== undefined && (
        <button
          type="button"
          onClick={() => setScanne(true)}
          className="pression mt-4 flex w-full items-center justify-center gap-2 rounded-xl border border-line px-4 py-2.5 text-[13px] text-ink transition-colors hover:bg-elevated"
        >
          <Icon name="grid" size={15} />
          Scanner le QR
        </button>
      )}

      {scanne && (
        <ScannerQR
          onFermer={() => setScanne(false)}
          onLu={(contenu) => {
            setScanne(false);
            void coller(contenu);
          }}
        />
      )}

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

      {rapport !== null && rapport.manquants.length > 0 && (
        <Manquants
          rapport={rapport}
          transfert={transfert}
          onRapatrier={() => void rapatrier()}
        />
      )}

      {bilanTransfert !== null && <BilanTransfert bilan={bilanTransfert} />}
      {erreur !== null && <Erreur texte={erreur} />}
    </section>
  );
}

/** « 3,4 Go », « 812 Mo ». */
function poids(octets: number): string {
  if (octets >= 1_000_000_000) return `${(octets / 1_000_000_000).toFixed(1)} Go`;
  if (octets >= 1_000_000) return `${Math.round(octets / 1_000_000)} Mo`;
  return `${Math.max(1, Math.round(octets / 1000))} Ko`;
}

/**
 * Les morceaux que l'autre a et qu'on n'a pas.
 *
 * # Pourquoi ils ne descendent pas tout seuls
 *
 * Ce sont des fichiers. Quelques mégaoctets chacun, plusieurs gigaoctets pour
 * une bibliothèque entière : les faire venir sans prévenir remplirait un
 * téléphone en silence. Le poids est donc annoncé avant, pas découvert après.
 *
 * # Pourquoi ils étaient invisibles
 *
 * La synchronisation annonçait « déjà d'accord » dès que favoris, playlists et
 * paroles concordaient — sans un mot sur les morceaux que l'un des deux
 * n'avait tout simplement pas. C'est pourtant la première chose qu'on remarque
 * après avoir téléchargé de la musique sur l'autre appareil.
 */
function Manquants({
  rapport,
  transfert,
  onRapatrier,
}: {
  rapport: SyncReport;
  transfert: TransferProgress | null;
  onRapatrier: () => void;
}) {
  const nombre = rapport.manquants.length;
  const encours = transfert !== null && transfert.fait < transfert.total;

  return (
    <div className="mt-3 rounded-lg border border-line bg-elevated px-3.5 py-3">
      <p className="text-[13px] font-medium text-ink">
        {nombre} morceau{nombre > 1 ? "x" : ""} ne {nombre > 1 ? "sont" : "est"}{" "}
        pas ici
      </p>
      <p className="mt-0.5 text-[12px] text-ink-muted">
        {poids(rapport.octetsManquants)} à récupérer depuis {rapport.appareil}.
      </p>

      <ul className="mt-2 space-y-0.5 text-[12px] text-ink-faint">
        {rapport.manquants.slice(0, 3).map((morceau) => (
          <li key={morceau.chemin} className="truncate">
            {morceau.artiste === null
              ? morceau.titre
              : `${morceau.artiste} — ${morceau.titre}`}
          </li>
        ))}
        {nombre > 3 && <li>et {nombre - 3} autres…</li>}
      </ul>

      {encours ? (
        <div className="mt-3">
          <div className="h-1 overflow-hidden rounded-full bg-base">
            <div
              className="h-full bg-accent transition-[width] duration-300"
              style={{ width: `${(transfert.fait / Math.max(1, transfert.total)) * 100}%` }}
            />
          </div>
          <p className="numerals mt-1.5 truncate text-[12px] text-ink-muted">
            {transfert.fait} / {transfert.total} · {transfert.titre}
          </p>
        </div>
      ) : (
        <button
          type="button"
          onClick={onRapatrier}
          className="pression mt-3 w-full rounded-lg border border-line px-4 py-2.5 text-[13px] text-ink transition-colors hover:bg-surface"
        >
          Récupérer les fichiers
        </button>
      )}
    </div>
  );
}

function BilanTransfert({ bilan }: { bilan: TransferReport }) {
  return (
    <div className="mt-3 rounded-lg border border-accent/25 bg-accent/5 px-3.5 py-3 text-[13px] leading-relaxed text-ink">
      <p className="font-medium">
        {bilan.recus} morceau{bilan.recus > 1 ? "x" : ""} rapatrié
        {bilan.recus > 1 ? "s" : ""}
      </p>

      {(bilan.doublons > 0 || bilan.echecs > 0) && (
        <ul className="mt-1 space-y-0.5 text-[12px] text-ink-muted">
          {bilan.doublons > 0 && (
            <li>
              {bilan.doublons} étai{bilan.doublons > 1 ? "ent" : "t"} déjà là sous
              un autre nom
            </li>
          )}
          {bilan.echecs > 0 && (
            <li className="text-warn">
              {bilan.echecs} n&apos;{bilan.echecs > 1 ? "ont" : "a"} pas pu être
              récupéré{bilan.echecs > 1 ? "s" : ""}
              {bilan.premiereErreur !== null && ` — ${bilan.premiereErreur}`}
            </li>
          )}
        </ul>
      )}
    </div>
  );
}

function Resultat({ rapport }: { rapport: SyncReport }) {
  // « Déjà d'accord » ne vaut que si **tout** concorde, fichiers compris : le
  // dire alors que des morceaux entiers manquent est le défaut qu'on corrige.
  const rien =
    rapport.favoris === 0 &&
    rapport.paroles === 0 &&
    rapport.playlists === 0 &&
    rapport.manquants.length === 0 &&
    rapport.manquantsLaBas === 0;

  return (
    <div className="mt-4 rounded-lg border border-accent/25 bg-accent/5 px-3.5 py-3 text-[13px] leading-relaxed text-ink">
      {/* « Repris de X » suivi de rien se lit comme un échec. Ce cas arrive
          pour de bon : quand seuls des fichiers manquent, il n'y a rien à
          reprendre — et c'est une bonne nouvelle, pas une panne. */}
      <p className="font-medium">
        {rien
          ? "Déjà d'accord"
          : rapport.favoris + rapport.paroles + rapport.playlists === 0
            ? "Rien à reprendre"
            : `Repris de ${rapport.appareil}`}
      </p>

      {rien ? (
        <p className="mt-1 text-ink-muted">
          Les deux bibliothèques disaient déjà la même chose, et aucune
          n&apos;a de morceau que l&apos;autre n&apos;aurait pas.
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

      {/* On ne peut rien en faire d'ici : c'est celui qui se connecte qui
          rapatrie. Le taire laisserait croire que tout est réglé. */}
      {rapport.manquantsLaBas > 0 && (
        <p className="mt-2 text-[12px] leading-relaxed text-ink-faint">
          {rapport.manquantsLaBas} morceau{rapport.manquantsLaBas > 1 ? "x" : ""}{" "}
          d&apos;ici {rapport.manquantsLaBas > 1 ? "manquent" : "manque"} chez{" "}
          {rapport.appareil}. Pour {rapport.manquantsLaBas > 1 ? "les" : "le"} lui
          donner, ouvre la porte de ce côté et connecte-toi depuis lui.
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
