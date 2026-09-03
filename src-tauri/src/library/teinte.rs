//! La couleur qu'une pochette donne à sa page.
//!
//! # Pourquoi cette couleur existe
//!
//! La charte du projet dit, depuis le premier jour : « les couleurs de l'écran
//! viennent des albums, pas du châssis ». Elle était restée une intention — le
//! châssis est en niveaux de gris, et les albums ne teintaient rien.
//!
//! Le voile de l'en-tête est l'endroit où cette phrase peut enfin être vraie.
//!
//! # Pourquoi la teinte est ramenée à une luminosité fixe
//!
//! Une couleur dominante brute est inutilisable : une pochette blanche donne du
//! blanc, une pochette fluo donne du fluo, et le titre en blanc devient
//! illisible sur l'une comme sur l'autre. On ne garde donc de la pochette que
//! sa **teinte** — sa position sur la roue — et l'on impose la luminosité et la
//! saturation. Toutes les pages gardent ainsi la même valeur, le même contraste
//! avec le texte, et ne changent que de couleur.
//!
//! C'est aussi ce qui empêche l'escalier : le voile reste un aplat.

/// Luminosité imposée au voile, dans l'espace TSL.
///
/// 0,135 place le voile à peu près là où était le gris qu'il remplace (29 sur
/// 255 une fois composé), donc sans toucher au contraste du titre.
const LUMINOSITE: f32 = 0.135;

/// Saturation minimale et maximale du voile.
///
/// En dessous de 0,22 la teinte ne se voit pas et l'on a fait le travail pour
/// rien ; au-dessus de 0,55, le voile prend le pas sur la pochette qu'il est
/// censé accompagner.
const SATURATION: (f32, f32) = (0.22, 0.55);

/// Nombre de secteurs de teinte.
///
/// Vingt-quatre secteurs de quinze degrés : assez fin pour distinguer un rouge
/// d'un orange, assez large pour qu'un dégradé de ciel ne se disperse pas en
/// autant de votes qu'il a de nuances.
const SECTEURS: usize = 24;

/// Ce qu'on refuse de compter dans le vote.
///
/// Un pixel presque noir n'a pas de teinte fiable — les arrondis y font tourner
/// la couleur d'un secteur à l'autre. Un pixel gris n'en a pas du tout. Les deux
/// ensemble forment le fond de la plupart des pochettes : les laisser voter
/// revient à élire le fond.
///
/// # Pourquoi il n'y a pas de valeur *maximale*
///
/// Il y en avait une, à 0,96, pour écarter le blanc. Elle écartait surtout le
/// **rouge pur** : en TSV, la valeur est la plus grande des trois composantes,
/// et elle vaut 1 pour tout ce qui est vif. Le blanc et le rouge vif ont la
/// même valeur ; ce qui les sépare est la saturation, et le seuil ci-dessous
/// s'en charge déjà. Le test des pochettes rouges l'a dit tout de suite.
const VALEUR_MIN: f32 = 0.12;
const SATURATION_MIN: f32 = 0.18;

/// Part minimale de pixels colorés pour qu'un vote fasse foi.
///
/// Sous 4 %, la pochette est en noir et blanc — une photo argentique, une
/// typographie sur fond noir — et lui inventer une couleur serait mentir sur
/// ce qu'elle est.
const PART_MIN: f32 = 0.04;

/// La teinte d'une pochette, à partir de ses pixels RVB.
///
/// `None` quand l'image n'a pas de couleur assumée : la page garde alors son
/// gris, qui est le bon choix pour une pochette en noir et blanc.
pub fn dominante(rgb: &[u8]) -> Option<[u8; 3]> {
    if rgb.len() < 3 {
        return None;
    }

    let total = rgb.len() / 3;
    let mut poids = [0f32; SECTEURS];
    let mut sommes = [[0f32; 3]; SECTEURS];
    let mut comptes = [0usize; SECTEURS];
    let mut colores = 0usize;

    for pixel in rgb.chunks_exact(3) {
        let (r, v, b) = (
            f32::from(pixel[0]) / 255.0,
            f32::from(pixel[1]) / 255.0,
            f32::from(pixel[2]) / 255.0,
        );
        let (teinte, saturation, valeur) = vers_tsv(r, v, b);

        if valeur < VALEUR_MIN || saturation < SATURATION_MIN {
            continue;
        }

        colores += 1;
        let secteur = (((teinte / 360.0) * SECTEURS as f32) as usize).min(SECTEURS - 1);

        // Un pixel vif et lumineux pèse plus qu'un pixel terne : c'est celui
        // qu'on retient d'une pochette quand on se la rappelle.
        poids[secteur] += saturation * valeur;
        comptes[secteur] += 1;
        sommes[secteur][0] += r;
        sommes[secteur][1] += v;
        sommes[secteur][2] += b;
    }

    if (colores as f32) < PART_MIN * total as f32 {
        return None;
    }

    let (gagnant, _) = poids
        .iter()
        .enumerate()
        .max_by(|gauche, droite| gauche.1.total_cmp(droite.1))?;

    let compte = comptes[gagnant];
    if compte == 0 {
        return None;
    }

    let moyenne = [
        sommes[gagnant][0] / compte as f32,
        sommes[gagnant][1] / compte as f32,
        sommes[gagnant][2] / compte as f32,
    ];

    let (teinte, saturation, _) = vers_tsv(moyenne[0], moyenne[1], moyenne[2]);
    let saturation = saturation.clamp(SATURATION.0, SATURATION.1);

    Some(depuis_tsl(teinte, saturation, LUMINOSITE))
}

/// La même, écrite en hexadécimal pour le CSS.
pub fn dominante_hex(rgb: &[u8]) -> Option<String> {
    dominante(rgb).map(|[r, v, b]| format!("#{r:02x}{v:02x}{b:02x}"))
}

/// Teinte (0–360), saturation et valeur (0–1).
fn vers_tsv(r: f32, v: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(v).max(b);
    let min = r.min(v).min(b);
    let delta = max - min;

    let teinte = if delta <= f32::EPSILON {
        0.0
    } else if max == r {
        60.0 * (((v - b) / delta) % 6.0)
    } else if max == v {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - v) / delta) + 4.0)
    };

    let teinte = if teinte < 0.0 { teinte + 360.0 } else { teinte };
    let saturation = if max <= f32::EPSILON { 0.0 } else { delta / max };

    (teinte, saturation, max)
}

/// Reconstruit une couleur depuis sa teinte, sa saturation et sa luminosité.
fn depuis_tsl(teinte: f32, saturation: f32, luminosite: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * luminosite - 1.0).abs()) * saturation;
    let x = c * (1.0 - (((teinte / 60.0) % 2.0) - 1.0).abs());
    let m = luminosite - c / 2.0;

    let (r, v, b) = match teinte as u32 / 60 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    [
        (((r + m) * 255.0).round()).clamp(0.0, 255.0) as u8,
        (((v + m) * 255.0).round()).clamp(0.0, 255.0) as u8,
        (((b + m) * 255.0).round()).clamp(0.0, 255.0) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Construit une image d'une seule couleur.
    fn uni(r: u8, v: u8, b: u8, pixels: usize) -> Vec<u8> {
        (0..pixels).flat_map(|_| [r, v, b]).collect()
    }

    #[test]
    fn une_pochette_rouge_donne_un_voile_rouge() {
        let couleur = dominante(&uni(220, 30, 40, 100)).expect("teinte trouvée");
        assert!(couleur[0] > couleur[1] && couleur[0] > couleur[2], "{couleur:?}");
    }

    #[test]
    fn une_pochette_bleue_donne_un_voile_bleu() {
        let couleur = dominante(&uni(30, 60, 220, 100)).expect("teinte trouvée");
        assert!(couleur[2] > couleur[0] && couleur[2] > couleur[1], "{couleur:?}");
    }

    /// Le point de la normalisation : quelle que soit la pochette, le voile a
    /// la même valeur, donc le titre garde le même contraste.
    #[test]
    fn toutes_les_teintes_ont_la_meme_luminosite() {
        let vif = dominante(&uni(255, 0, 0, 100)).unwrap();
        let terne = dominante(&uni(90, 40, 40, 100)).unwrap();

        let clarte = |c: [u8; 3]| c.iter().map(|v| u32::from(*v)).sum::<u32>();
        let ecart = clarte(vif).abs_diff(clarte(terne));
        assert!(ecart < 40, "vif {vif:?} contre terne {terne:?}");
    }

    /// Une pochette en noir et blanc n'a pas de couleur à donner : lui en
    /// inventer une mentirait sur ce qu'elle est.
    #[test]
    fn une_pochette_grise_ne_teinte_rien() {
        assert_eq!(dominante(&uni(120, 120, 120, 100)), None);
        assert_eq!(dominante(&uni(20, 20, 22, 100)), None);
    }

    /// Le fond ne doit pas élire la couleur : ici quatre-vingt-dix pour cent de
    /// noir et dix pour cent de vert franc.
    #[test]
    fn le_fond_noir_ne_vote_pas() {
        let mut image = uni(4, 4, 6, 90);
        image.extend(uni(40, 200, 90, 10));

        let couleur = dominante(&image).expect("le vert l'emporte");
        assert!(couleur[1] > couleur[0] && couleur[1] > couleur[2], "{couleur:?}");
    }

    /// Trois pixels colorés sur cent, c'est du bruit, pas une identité.
    #[test]
    fn une_trace_de_couleur_ne_suffit_pas() {
        let mut image = uni(200, 200, 200, 97);
        image.extend(uni(255, 0, 0, 3));
        assert_eq!(dominante(&image), None);
    }

    #[test]
    fn l_hexadecimal_a_sept_caracteres() {
        let hex = dominante_hex(&uni(220, 30, 40, 100)).unwrap();
        assert_eq!(hex.len(), 7);
        assert!(hex.starts_with('#'));
    }

    #[test]
    fn une_image_vide_ne_dit_rien() {
        assert_eq!(dominante(&[]), None);
        assert_eq!(dominante(&[255]), None);
    }
}
