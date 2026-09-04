/**
 * Demande une image à l'utilisateur, et la rend en octets.
 *
 * # Pourquoi un champ de fichier plutôt que la boîte de dialogue native
 *
 * La boîte native rend un **chemin**. Sur un téléphone, il n'y en a pas : le
 * sélecteur d'Android rend un `content://`, une adresse que seul le résolveur
 * de contenu du système sait ouvrir. Le cœur recevait donc un chemin
 * inexistant, et se plaignait — à juste titre — de ne pas trouver le fichier.
 *
 * Un champ de fichier, lui, ne parle pas d'adresses : il donne l'image. La
 * WebView d'Android le prend en charge, le bureau aussi, et les deux ouvrent le
 * sélecteur du système.
 *
 * # Pourquoi elle est réduite avant de partir
 *
 * Une photo de téléphone fait vingt mégaoctets, soit vingt-sept une fois
 * encodée pour traverser la frontière. Le cœur n'en garde de toute façon qu'une
 * vignette de cinq cent douze pixels : lui envoyer l'original reviendrait à
 * faire voyager vingt-six mégaoctets pour en jeter vingt-six.
 */
const COTE_MAX = 1024;

export async function choisirImage(): Promise<string | null> {
  const fichier = await demander();
  if (fichier === null) return null;

  const brut = await lire(fichier);
  if (brut === null) return null;

  return (await reduire(brut)) ?? brut;
}

function demander(): Promise<File | null> {
  return new Promise((resoudre) => {
    const champ = document.createElement("input");
    champ.type = "file";
    champ.accept = "image/*";

    let rendu = false;
    const rendre = (fichier: File | null) => {
      if (rendu) return;
      rendu = true;
      resoudre(fichier);
    };

    champ.addEventListener("change", () => rendre(champ.files?.[0] ?? null));

    // Annuler ne déclenche aucun événement fiable sur toutes les plateformes :
    // la promesse resterait en suspens, et l'appelant attendrait pour rien. Le
    // retour du focus sur la page est le seul signal commun — avec un délai,
    // car il précède parfois le `change` de quelques millisecondes.
    window.addEventListener(
      "focus",
      // Une seconde et demie : le retour du focus précède parfois de peu
      // l'arrivée du fichier, et répondre trop tôt jetterait l'image qu'on
      // vient de choisir. Trop tôt coûte le résultat ; trop tard ne coûte
      // qu'une attente qu'on ne regarde pas.
      () => setTimeout(() => rendre(champ.files?.[0] ?? null), 1500),
      { once: true },
    );

    // # Pourquoi il entre dans le document
    //
    // Un `input` détaché ouvre bien le sélecteur sur un navigateur de bureau ;
    // la WebView d'Android, elle, **ne le voit pas** — `onShowFileChooser`
    // n'est jamais appelé, et rien ne se passe. Éprouvé sur l'appareil : la
    // pochette touchée, aucun sélecteur, aucune erreur.
    //
    // Il est donc inséré, hors champ de vision, et retiré ensuite.
    champ.style.position = "fixed";
    champ.style.left = "-9999px";
    document.body.appendChild(champ);
    champ.click();

    // Le retirer tout de suite annulerait la demande : on attend que la
    // réponse soit arrivée, quelle qu'elle soit.
    const retirer = () => champ.remove();
    champ.addEventListener("change", retirer);
    setTimeout(retirer, 120_000);
  });
}

function lire(fichier: File): Promise<string | null> {
  return new Promise((resoudre) => {
    const lecteur = new FileReader();
    lecteur.onload = () =>
      resoudre(typeof lecteur.result === "string" ? lecteur.result : null);
    lecteur.onerror = () => resoudre(null);
    lecteur.readAsDataURL(fichier);
  });
}

/** Réduit l'image si elle dépasse, et la rend en JPEG. */
function reduire(source: string): Promise<string | null> {
  return new Promise((resoudre) => {
    const image = new Image();

    image.onload = () => {
      const cote = Math.max(image.width, image.height);
      if (cote <= COTE_MAX) {
        resoudre(source);
        return;
      }

      const facteur = COTE_MAX / cote;
      const toile = document.createElement("canvas");
      toile.width = Math.round(image.width * facteur);
      toile.height = Math.round(image.height * facteur);

      const contexte = toile.getContext("2d");
      if (contexte === null) {
        resoudre(source);
        return;
      }

      contexte.drawImage(image, 0, 0, toile.width, toile.height);
      resoudre(toile.toDataURL("image/jpeg", 0.92));
    };

    // Une image illisible n'est pas une raison d'échouer ici : le cœur la
    // refusera, et son message sera plus juste que le nôtre.
    image.onerror = () => resoudre(null);
    image.src = source;
  });
}
