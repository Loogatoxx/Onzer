import { useEffect, useRef, useState } from "react";
import jsQR from "jsqr";

import { Icon } from "@/components/Icon";

/**
 * Lire le QR affiché par l'autre appareil.
 *
 * # Pourquoi le décodage est en JavaScript
 *
 * L'image vient de la caméra, elle vit déjà dans la page : la faire descendre
 * jusqu'au cœur Rust demanderait de convertir chaque trame en base64 et de la
 * pousser à travers le pont, plusieurs fois par seconde. Un mégaoctet par
 * seconde pour lire vingt caractères.
 *
 * # Pourquoi la caméra fonctionne dans la vue web
 *
 * Deux conditions, toutes deux déjà remplies. La page est servie depuis
 * `tauri.localhost`, que Chromium considère comme un contexte sûr — sans quoi
 * `getUserMedia` n'existe même pas. Et la coquille Android de Tauri implémente
 * `onPermissionRequest` : elle demande d'elle-même l'autorisation d'accès à la
 * caméra, à condition que le manifeste la déclare.
 */
export function ScannerQR({
  onLu,
  onFermer,
}: {
  /** Le contenu du QR, tel quel. */
  onLu: (contenu: string) => void;
  onFermer: () => void;
}) {
  const video = useRef<HTMLVideoElement | null>(null);
  const [erreur, setErreur] = useState<string | null>(null);
  /** Incrémenté par « Réessayer » : c'est ce qui relance l'effet. */
  const [essai, setEssai] = useState(0);

  useEffect(() => {
    let flux: MediaStream | null = null;
    let trame = 0;
    let vivant = true;

    const toile = document.createElement("canvas");
    const pinceau = toile.getContext("2d", { willReadFrequently: true });

    async function ouvrir() {
      try {
        // `environment` : l'appareil photo arrière. Sans cette précision, un
        // téléphone ouvre la caméra frontale, qui ne voit que le lecteur.
        flux = await navigator.mediaDevices.getUserMedia({
          video: { facingMode: "environment" },
        });

        if (!vivant) {
          for (const piste of flux.getTracks()) piste.stop();
          return;
        }

        if (video.current !== null) {
          video.current.srcObject = flux;

          // # Pourquoi `play()` n'est pas attendu, ni son échec fatal
          //
          // Sur Android, il arrive qu'il rejette faute de geste utilisateur —
          // alors même que `autoPlay` démarrera la lecture une fraction de
          // seconde plus tard. Traiter ce rejet comme une panne laissait un
          // rectangle gris avec un bouton de lecture au milieu : la vue web
          // proposait de démarrer une vidéo qui tournait déjà.
          void video.current.play().catch(() => undefined);
        }

        chercher();
      } catch (cause) {
        const texte = String(cause);

        setErreur(
          texte.includes("NotAllowed") || texte.includes("Permission")
            ? "L'accès à la caméra a été refusé. Autorise-le dans les réglages du téléphone, ou recopie les huit chiffres."
            : texte.includes("NotFound")
              ? "Aucune caméra trouvée sur cet appareil."
              : texte,
        );
      }
    }

    function chercher() {
      if (!vivant) return;
      trame = requestAnimationFrame(chercher);

      const source = video.current;
      if (source === null || pinceau === null || source.readyState < 2) return;

      // La trame est réduite à 480 pixels de large : un QR de vingt caractères
      // s'y lit parfaitement, et l'analyse coûte quatre fois moins qu'en pleine
      // définition — ce qui décide entre un aperçu fluide et un diaporama.
      const largeur = 480;
      const hauteur = Math.round((source.videoHeight / source.videoWidth) * largeur);
      if (!Number.isFinite(hauteur) || hauteur <= 0) return;

      toile.width = largeur;
      toile.height = hauteur;
      pinceau.drawImage(source, 0, 0, largeur, hauteur);

      const image = pinceau.getImageData(0, 0, largeur, hauteur);
      const trouve = jsQR(image.data, image.width, image.height, {
        inversionAttempts: "dontInvert",
      });

      if (trouve !== null && trouve.data !== "") {
        vivant = false;
        onLu(trouve.data);
      }
    }

    void ouvrir();

    return () => {
      vivant = false;
      cancelAnimationFrame(trame);
      if (flux !== null) {
        for (const piste of flux.getTracks()) piste.stop();
      }
    };
  }, [onLu, essai]);

  return (
    <div
      role="dialog"
      aria-label="Scanner le code"
      className="fixed inset-0 z-50 flex flex-col bg-black"
    >
      <div className="flex items-center justify-between px-4 pb-3 pt-[calc(env(safe-area-inset-top)+0.75rem)]">
        <p className="text-sm font-medium text-ink">Vise le QR de l&apos;autre appareil</p>
        <button
          type="button"
          aria-label="Fermer"
          onClick={onFermer}
          className="pression flex h-9 w-9 items-center justify-center rounded-full bg-elevated text-ink"
        >
          <Icon name="close" size={16} />
        </button>
      </div>

      <div className="relative flex-1 overflow-hidden">
        <video
          ref={video}
          autoPlay
          playsInline
          muted
          className="h-full w-full bg-black object-cover"
        />

        {/* Un cadre au centre : sans repère, on ne sait pas où viser, et l'on
            approche le téléphone jusqu'à ce que le QR sorte du champ. */}
        {erreur === null && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
            <div className="h-56 w-56 rounded-2xl border-2 border-white/70" />
          </div>
        )}

        {/* # Pourquoi l'erreur occupe le centre
            Elle était écrite en bas d'un écran noir, sous la vidéo. On voyait
            un rectangle vide et rien d'autre : l'explication existait, hors du
            regard. */}
        {erreur !== null && (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-4 bg-black px-8 text-center">
            <span className="text-warn">
              <Icon name="close" size={28} />
            </span>
            <p className="text-[14px] leading-relaxed text-ink">{erreur}</p>

            <button
              type="button"
              onClick={() => {
                setErreur(null);
                setEssai((valeur) => valeur + 1);
              }}
              className="pression rounded-full bg-elevated px-5 py-2.5 text-[13px] font-medium text-ink"
            >
              Réessayer
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
