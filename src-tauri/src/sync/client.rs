//! Le côté qui se connecte.
//!
//! Rien de plus qu'un aller-retour : on envoie ce qu'on a, on reçoit l'union,
//! on l'applique. La décision, elle, est prise deux fois — une fois de chaque
//! côté, par le même code.

use serde::Serialize;
use sqlx::SqlitePool;

use crate::core::{OnzerError, PathResolver, Result};
use crate::library::importer::{self, FileHandling, ImportOutcome};

use super::etat;
use super::fusion::{fusionner, Changement, EtatSync, Manquant};

/// Ce qu'une synchronisation a fait, en clair.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RapportSync {
    /// Le nom de l'appareil d'en face.
    pub appareil: String,
    /// Favoris repris de l'autre côté.
    pub favoris: usize,
    /// Morceaux qui ont gagné des paroles.
    pub paroles: usize,
    /// Playlists créées ou complétées.
    pub playlists: usize,
    /// Décisions où les deux côtés se contredisaient.
    pub arbitrages: usize,
    /// Ce que l'autre a et que nous n'avons pas — annoncé, pas rapatrié.
    pub manquants: Vec<Manquant>,
    /// Le poids total de ces morceaux, en octets.
    pub octets_manquants: i64,
    /// Ce que **nous** avons et qui manque chez l'autre.
    ///
    /// On ne peut rien en faire d'ici — c'est celui qui se connecte qui
    /// rapatrie, jamais celui qui ouvre. Mais le taire laisserait croire que
    /// tout est réglé alors qu'il reste un échange à faire dans l'autre sens.
    pub manquants_la_bas: usize,
}

impl RapportSync {
    fn compter(
        appareil: &str,
        changements: &[Changement],
        arbitrages: usize,
        manquants: Vec<Manquant>,
        manquants_la_bas: usize,
    ) -> Self {
        let octets_manquants = manquants.iter().map(|morceau| morceau.taille).sum();

        let mut rapport = Self {
            appareil: appareil.to_string(),
            arbitrages,
            manquants,
            octets_manquants,
            manquants_la_bas,
            ..Self::default()
        };

        for changement in changements {
            match changement {
                Changement::Aime { .. } => rapport.favoris += 1,
                Changement::Paroles { .. } => rapport.paroles += 1,
                Changement::Playlist { .. } => rapport.playlists += 1,
            }
        }

        rapport
    }
}

/// Se connecte à l'autre appareil et fusionne.
///
/// # Pourquoi un délai généreux
///
/// Six mégaoctets de paroles montent, six redescendent, et l'autre côté fait
/// une transaction SQLite entre les deux. Sur un Wi-Fi domestique c'est
/// l'affaire de quelques secondes ; sur un réseau encombré, trente ne sont pas
/// de trop. Le défaut de `reqwest` — trente secondes — suffit, mais il vaut
/// mieux l'écrire que le supposer.
pub async fn synchroniser(
    pool: &SqlitePool,
    hote: &str,
    port: u16,
    code: &str,
) -> Result<RapportSync> {
    let local = etat::lire(pool).await?;

    let reponse = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|erreur| OnzerError::Invalid(format!("client HTTP : {erreur}")))?
        .post(format!("http://{hote}:{port}/sync/v1/fusion"))
        .bearer_auth(code)
        .json(&local)
        .send()
        .await
        .map_err(|erreur| {
            OnzerError::Invalid(format!(
                "impossible de joindre {hote} : {erreur}. \
                 Les deux appareils sont-ils sur le même Wi-Fi ?"
            ))
        })?;

    if reponse.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(OnzerError::Invalid(
            "code refusé. Vérifie les huit chiffres affichés sur l'autre appareil.".to_string(),
        ));
    }

    if !reponse.status().is_success() {
        let code_http = reponse.status();
        let detail = reponse.text().await.unwrap_or_default();
        return Err(OnzerError::Invalid(format!(
            "l'autre appareil a répondu {code_http} : {detail}"
        )));
    }

    let distant: EtatSync = reponse
        .json()
        .await
        .map_err(|erreur| OnzerError::Invalid(format!("réponse illisible : {erreur}")))?;

    let alias = etat::alias(pool).await?;
    let resultat = fusionner(&local, &distant, &alias);

    etat::appliquer(
        pool,
        &distant.appareil,
        &resultat.changements,
        &resultat.arbitrages,
    )
    .await?;

    // La même fusion, arguments inversés : ce que nous avons et qu'ils n'ont
    // pas. Le calcul est local et ne coûte rien — les deux états sont déjà là.
    let chez_eux = fusionner(&distant, &local, &alias).manquants.len();

    Ok(RapportSync::compter(
        &distant.appareil,
        &resultat.changements,
        resultat.arbitrages.len(),
        resultat.manquants,
        chez_eux,
    ))
}

// ════════════════════════════════════════════════════════════════════════════
//  Rapatrier les fichiers
// ════════════════════════════════════════════════════════════════════════════

/// Ce qu'un transfert a donné.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RapportTransfert {
    pub recus: usize,
    /// Déjà présents sous un autre nom : le fichier n'a pas été gardé.
    pub doublons: usize,
    pub echecs: usize,
    /// Le premier message d'erreur, quand il y en a eu.
    pub premiere_erreur: Option<String>,
}

/// L'avancement, poussé vers l'interface morceau par morceau.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Avancement {
    fait: usize,
    total: usize,
    titre: String,
}

/// Rapatrie les fichiers que l'autre appareil possède et pas nous.
///
/// # Pourquoi un par un, et pas une archive
///
/// Une archive serait plus rapide sur le réseau et bien pire partout ailleurs :
/// il faudrait la constituer (donc la place pour la stocker deux fois), la
/// transmettre en entier avant d'en tirer quoi que ce soit, et tout perdre si
/// la connexion tombe à quatre-vingt-dix pour cent. Fichier par fichier,
/// chaque morceau arrivé est acquis, et l'interface peut dire où elle en est.
///
/// # Pourquoi l'import passe par le même chemin que les autres
///
/// Le fichier descend dans un dossier temporaire, puis `import_file` le range
/// selon les règles de la bibliothèque — même nommage, même dédoublonnage,
/// même journal. Écrire directement à l'emplacement annoncé par l'autre
/// appareil irait plus vite et créerait une seconde façon d'entrer dans la
/// bibliothèque, avec ses propres oublis.
pub async fn telecharger(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    paths: &PathResolver,
    hote: &str,
    port: u16,
    code: &str,
    morceaux: &[Manquant],
) -> Result<RapportTransfert> {
    use tauri::Emitter;

    if !paths.is_library_online() {
        return Err(OnzerError::Invalid(
            "bibliothèque hors ligne : le dossier de musique est introuvable".to_string(),
        ));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|erreur| OnzerError::Invalid(format!("client HTTP : {erreur}")))?;

    let mut rapport = RapportTransfert::default();

    for (index, morceau) in morceaux.iter().enumerate() {
        let _ = app.emit(
            super::EVENEMENT_TRANSFERT,
            Avancement {
                fait: index,
                total: morceaux.len(),
                titre: morceau.titre.clone(),
            },
        );

        match recevoir_un(&client, pool, paths, hote, port, code, morceau).await {
            Ok(ImportOutcome::Imported { .. }) | Ok(ImportOutcome::Restored { .. }) => {
                rapport.recus += 1;
            }
            Ok(ImportOutcome::Duplicate { existing_id, .. }) => {
                rapport.doublons += 1;

                // # Pourquoi retenir ce que l'import vient de découvrir
                //
                // Il a reconnu le morceau en lisant son contenu — la seule
                // façon d'y arriver quand ni le chemin ni les tags ne
                // concordent. Sans mémoire, la fusion suivante le redemandera,
                // on le retéléchargera, et il finira encore à la poubelle.
                // Onze morceaux revenaient ainsi à chaque synchronisation.
                if let Ok(Some(local)) = sqlx::query_scalar::<_, String>(
                    "SELECT relative_path FROM tracks WHERE id = ?",
                )
                .bind(existing_id)
                .fetch_optional(pool)
                .await
                {
                    let _ = etat::noter_alias(pool, &morceau.chemin, &local).await;
                }
            }
            Err(erreur) => {
                tracing::warn!(chemin = %morceau.chemin, %erreur, "morceau non rapatrié");
                rapport.echecs += 1;
                if rapport.premiere_erreur.is_none() {
                    rapport.premiere_erreur = Some(erreur.to_string());
                }
            }
        }
    }

    let _ = app.emit(
        super::EVENEMENT_TRANSFERT,
        Avancement {
            fait: morceaux.len(),
            total: morceaux.len(),
            titre: String::new(),
        },
    );

    Ok(rapport)
}

/// Encode un chemin pour le mettre dans une URL.
///
/// # Pourquoi à la main
///
/// Les chemins de la bibliothèque contiennent des espaces, des accents, des
/// crochets et des esperluettes — `Adèle Castillon/2024 - Inversions [Deluxe]`.
/// Passé tel quel, le `&` couperait la requête en deux paramètres et le morceau
/// demandé ne serait pas celui reçu. Seuls les caractères que la norme déclare
/// sûrs traversent ; tout le reste passe en `%XX`, y compris les `/`, qui n'ont
/// rien à faire dans une valeur de paramètre.
fn encoder(texte: &str) -> String {
    let mut sortie = String::with_capacity(texte.len() * 3 / 2);

    for octet in texte.as_bytes() {
        match octet {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                sortie.push(*octet as char);
            }
            _ => sortie.push_str(&format!("%{octet:02X}")),
        }
    }

    sortie
}

async fn recevoir_un(
    client: &reqwest::Client,
    pool: &SqlitePool,
    paths: &PathResolver,
    hote: &str,
    port: u16,
    code: &str,
    morceau: &Manquant,
) -> Result<ImportOutcome> {
    let reponse = client
        .get(format!(
            "http://{hote}:{port}/sync/v1/fichier?chemin={}",
            encoder(&morceau.chemin)
        ))
        .bearer_auth(code)
        .send()
        .await
        .map_err(|erreur| OnzerError::Invalid(format!("téléchargement : {erreur}")))?;

    if !reponse.status().is_success() {
        // L'explication du serveur vaut mieux que son code : « 404 Not Found »
        // se lit comme une panne, « ce morceau est hors ligne » se comprend.
        let code = reponse.status();
        let detail = reponse.text().await.unwrap_or_default();

        return Err(OnzerError::Invalid(if detail.trim().is_empty() {
            format!("l'autre appareil a répondu {code}")
        } else {
            detail
        }));
    }

    let octets = reponse
        .bytes()
        .await
        .map_err(|erreur| OnzerError::Invalid(format!("lecture interrompue : {erreur}")))?;

    // L'extension décide de l'importabilité : un fichier temporaire sans
    // extension serait refusé avant même d'être regardé.
    let extension = std::path::Path::new(&morceau.chemin)
        .extension()
        .and_then(|valeur| valeur.to_str())
        .unwrap_or("mp3");

    // # Pourquoi pas le dossier temporaire du système
    //
    // `std::env::temp_dir()` rend `/tmp` quand `TMPDIR` n'est pas défini — et
    // sur Android, `/tmp` n'existe pas. Le transfert aurait échoué sur le seul
    // appareil pour lequel il a été écrit, avec une erreur d'écriture parlant
    // d'un dossier dont personne n'a jamais entendu parler. Le dossier de
    // données de l'application, lui, existe toujours : c'est là que vit la base.
    let atelier = paths.data_dir().join("_transfert");
    std::fs::create_dir_all(&atelier)?;

    let fichier = atelier.join(format!("{}.{extension}", uuid::Uuid::new_v4()));
    std::fs::write(&fichier, &octets)?;

    let resultat =
        importer::import_file(pool, paths, &fichier, FileHandling::Organize, "auto_import").await;

    // L'import déplace le fichier quand il l'accepte, et le laisse quand il le
    // refuse comme doublon. Sans ce ménage, l'atelier grossirait de tout ce
    // qui a été refusé.
    let _ = std::fs::remove_file(&fichier);

    resultat
}

/// Découpe `onzer://appairage?hote=…&port=…&code=…`.
///
/// # Pourquoi un lien et pas seulement trois champs
///
/// Le QR doit tenir dans un seul texte, et ce texte doit pouvoir être ouvert
/// par l'appareil photo du téléphone — celui qu'on a déjà, qui sait lire un QR
/// et qui n'a besoin d'aucune permission qu'Onzer devrait demander.
pub fn lire_lien(lien: &str) -> Option<(String, u16, String)> {
    let question = lien.split_once('?')?.1;

    let mut hote = None;
    let mut port = None;
    let mut code = None;

    for paire in question.split('&') {
        match paire.split_once('=') {
            Some(("hote", valeur)) => hote = Some(valeur.to_string()),
            Some(("port", valeur)) => port = valeur.parse::<u16>().ok(),
            Some(("code", valeur)) => code = Some(valeur.to_string()),
            _ => {}
        }
    }

    Some((hote?, port?, code?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_chemin_encode_ne_coupe_pas_la_requete() {
        // L'esperluette d'un titre couperait l'URL en deux paramètres.
        assert_eq!(encoder("A & B.mp3"), "A%20%26%20B.mp3");
        // Les barres obliques n'ont rien à faire dans une valeur.
        assert_eq!(encoder("a/b.mp3"), "a%2Fb.mp3");
        // Les accents passent en UTF-8, deux octets chacun.
        assert_eq!(encoder("Adèle"), "Ad%C3%A8le");
        // Ce qui est sûr reste lisible.
        assert_eq!(encoder("un-deux_trois.mp3"), "un-deux_trois.mp3");
    }

    #[test]
    fn un_lien_complet_se_lit() {
        let lu = lire_lien("onzer://appairage?hote=192.168.1.42&port=47812&code=12345678");
        assert_eq!(
            lu,
            Some(("192.168.1.42".to_string(), 47812, "12345678".to_string()))
        );
    }

    #[test]
    fn un_lien_incomplet_ne_se_lit_pas() {
        // Mieux vaut ne rien proposer qu'une adresse à moitié devinée.
        assert!(lire_lien("onzer://appairage?hote=192.168.1.42").is_none());
        assert!(lire_lien("onzer://appairage").is_none());
        assert!(lire_lien("bonjour").is_none());
    }

    #[test]
    fn un_port_absurde_ne_se_lit_pas() {
        assert!(lire_lien("onzer://appairage?hote=a&port=999999&code=1").is_none());
    }
}
