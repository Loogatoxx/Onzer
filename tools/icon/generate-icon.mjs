/**
 * Génère les icônes d'Onzer — macOS et Android — sans aucune dépendance.
 *
 * Le motif est décrit **une seule fois** : carré aux angles arrondis, dégradé
 * violet → cyan repris des jetons de design (`src/styles/globals.css`), sur
 * lequel se détache une note de musique dans le noir de l'application. Les deux
 * plateformes en dérivent, chacune selon ses règles.
 *
 *   node tools/icon/generate-icon.mjs
 *
 * # Pourquoi Android ne peut pas simplement recevoir le PNG de macOS
 *
 * Android ne montre pas l'image qu'on lui donne : il la **découpe**. Le
 * lanceur choisit le masque — cercle, goutte, carré arrondi selon le
 * constructeur — et l'applique à un dessin de 108 dp dont seuls les 72 dp
 * centraux sont garantis visibles. Le carré arrondi de macOS, posé tel quel,
 * se retrouvait donc rétréci au centre d'un rond blanc, avec ses propres
 * angles arrondis à l'intérieur de ceux du masque.
 *
 * Une icône adaptative sépare le fond du motif : le dégradé occupe toute la
 * toile et se fait tailler par le masque, la note reste dans la zone sûre.
 * C'est la seule façon d'obtenir, sur n'importe quel lanceur, la même image
 * que dans le Dock.
 */

import { execFileSync } from "node:child_process";
import { deflateSync } from "node:zlib";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const RACINE = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const ANDROID = `${RACINE}/src-tauri/gen/android/app/src/main/res`;

const SAMPLES = 3; // suréchantillonnage par axe → 9 échantillons par pixel

// Jetons repris de globals.css, pour que l'icône et l'interface parlent la
// même langue visuelle.
const ACCENT = [0x8b, 0x5c, 0xf6]; // --color-accent
const ACCENT_ALT = [0x22, 0xd3, 0xee]; // --color-accent-alt
const BASE = [0x0a, 0x0a, 0x0c]; // --color-base

/** Part du corps occupée par le glyphe. Identique partout : c'est le motif. */
const GLYPH_RATIO = 0.52;

/** Rayon d'arrondi, proportionnel au corps et non à la toile. */
const CORNER_RATIO = 0.225;

/** Le glyphe est décrit dans un repère 24×24, puis mis à l'échelle. */
const GLYPH_BOX = 24;

// ── Géométrie du glyphe ─────────────────────────────────────────────────────

/**
 * Tête de la note : un disque.
 *
 * # Pourquoi son centre tombe exactement sous le bord droit de la hampe
 *
 * La hampe descendait plus bas que le centre du disque **et** dépassait de son
 * flanc droit : le coin qui en résultait laissait une encoche blanche entre les
 * deux, qu'on prenait pour une rayure sur l'icône. En calant le bord droit de
 * la hampe sur le point le plus à droite du disque, les deux contours se
 * rejoignent tangentiellement — et c'est d'ailleurs ainsi que la hampe se pose
 * sur la tête en gravure musicale.
 */
const TETE = { x: 10.9, y: 17.4, r: 4.2 };
/** Hampe et crochet : deux rectangles. */
const HAMPE = { x0: 13.1, x1: 15.1, y0: 3, y1: 17.4 };
const CROCHET = { x0: 13.1, x1: 20.4, y0: 3, y1: 6.6 };

/** Distance signée à un rectangle aux angles arrondis. Négative à l'intérieur. */
function distanceToRoundedRect(x, y, width, height, radius) {
  const dx = Math.abs(x - width / 2) - (width / 2 - radius);
  const dy = Math.abs(y - height / 2) - (height / 2 - radius);
  const outside = Math.hypot(Math.max(dx, 0), Math.max(dy, 0));
  return outside + Math.min(Math.max(dx, dy), 0) - radius;
}

/** Le point (gx, gy), exprimé dans le repère 24×24, est-il dans la note ? */
function isInsideGlyph(gx, gy) {
  const head = (gx - TETE.x) ** 2 + (gy - TETE.y) ** 2 <= TETE.r ** 2;
  const stem = gx >= HAMPE.x0 && gx <= HAMPE.x1 && gy >= HAMPE.y0 && gy <= HAMPE.y1;
  const flag =
    gx >= CROCHET.x0 && gx <= CROCHET.x1 && gy >= CROCHET.y0 && gy <= CROCHET.y1;
  return head || stem || flag;
}

// ── Rendu matriciel ─────────────────────────────────────────────────────────

/**
 * Dessine l'icône sur une toile carrée.
 *
 * @param size   côté de la toile, en pixels
 * @param body   côté du motif ; plus petit que la toile sur macOS, qui impose
 *               une zone de sécurité transparente autour de l'icône — sans
 *               elle, l'icône paraît plus grosse que ses voisines dans le Dock
 *               et mord sur l'indicateur d'application ouverte
 * @param forme  `"arrondi"` ou `"cercle"`
 */
function dessiner(size, body, forme) {
  const offset = (size - body) / 2;
  const radius = forme === "cercle" ? body / 2 : body * CORNER_RATIO;
  const echelle = (body * GLYPH_RATIO) / GLYPH_BOX;

  const gradientAt = (x, y) => {
    const u = (x - offset) / body;
    const v = (y - offset) / body;
    const t = Math.min(1, Math.max(0, u * 0.5 + v * 0.5));
    return [
      Math.round(ACCENT[0] + (ACCENT_ALT[0] - ACCENT[0]) * t),
      Math.round(ACCENT[1] + (ACCENT_ALT[1] - ACCENT[1]) * t),
      Math.round(ACCENT[2] + (ACCENT_ALT[2] - ACCENT[2]) * t),
    ];
  };

  const renderPixel = (px, py) => {
    let r = 0;
    let g = 0;
    let b = 0;
    let a = 0;

    for (let sy = 0; sy < SAMPLES; sy++) {
      for (let sx = 0; sx < SAMPLES; sx++) {
        const x = px + (sx + 0.5) / SAMPLES;
        const y = py + (sy + 0.5) / SAMPLES;

        const bodyX = x - offset;
        const bodyY = y - offset;
        if (
          bodyX < 0 ||
          bodyY < 0 ||
          bodyX > body ||
          bodyY > body ||
          distanceToRoundedRect(bodyX, bodyY, body, body, radius) > 0
        ) {
          continue;
        }

        const gx = (x - size / 2) / echelle + GLYPH_BOX / 2;
        const gy = (y - size / 2) / echelle + GLYPH_BOX / 2;

        const color = isInsideGlyph(gx, gy) ? BASE : gradientAt(x, y);
        r += color[0];
        g += color[1];
        b += color[2];
        a += 255;
      }
    }

    const total = SAMPLES * SAMPLES;
    if (a === 0) return [0, 0, 0, 0];

    // Les composantes sont moyennées sur les seuls échantillons opaques, afin
    // qu'un bord n'aille pas se mélanger avec du noir transparent.
    const opaque = a / 255;
    return [
      Math.round(r / opaque),
      Math.round(g / opaque),
      Math.round(b / opaque),
      Math.round(a / total),
    ];
  };

  // Chaque ligne est préfixée d'un octet de filtre (0 = aucun).
  const raster = Buffer.alloc(size * (size * 4 + 1));
  let offsetOctet = 0;

  for (let y = 0; y < size; y++) {
    raster[offsetOctet++] = 0;
    for (let x = 0; x < size; x++) {
      const [r, g, b, a] = renderPixel(x, y);
      raster[offsetOctet++] = r;
      raster[offsetOctet++] = g;
      raster[offsetOctet++] = b;
      raster[offsetOctet++] = a;
    }
  }

  return encodePng(raster, size);
}

// ── Encodage PNG ────────────────────────────────────────────────────────────

const CRC_TABLE = (() => {
  const table = new Int32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c;
  }
  return table;
})();

function crc32(buffer) {
  let c = -1;
  for (const byte of buffer) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
}

function pngChunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);

  const typed = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(typed));

  return Buffer.concat([length, typed, crc]);
}

function encodePng(pixels, size) {
  const signature = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);

  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; // 8 bits par composante
  ihdr[9] = 6; // RGBA
  ihdr[10] = 0; // compression deflate
  ihdr[11] = 0; // filtrage standard
  ihdr[12] = 0; // non entrelacé

  return Buffer.concat([
    signature,
    pngChunk("IHDR", ihdr),
    pngChunk("IDAT", deflateSync(pixels, { level: 9 })),
    pngChunk("IEND", Buffer.alloc(0)),
  ]);
}

function ecrire(chemin, contenu) {
  mkdirSync(dirname(chemin), { recursive: true });
  writeFileSync(chemin, contenu);
}

// ── macOS ───────────────────────────────────────────────────────────────────

const SOURCE = 1024;
// Gabarit d'Apple : sur une toile de 1024 px, le corps mesure 824 px.
const SOURCE_BODY = 824;

ecrire(`${RACINE}/src-tauri/icons/source.png`, dessiner(SOURCE, SOURCE_BODY, "arrondi"));
console.log(`✓ macOS : src-tauri/icons/source.png (${SOURCE}×${SOURCE})`);

// # Pourquoi les dérivés se génèrent ici et non à la main
//
// `tauri icon` produit le `.icns`, le `.ico` et les vignettes — mais il écrit
// **aussi** dans `res/`, où il remplace l'icône adaptative par un carré arrondi
// posé au centre d'un fond blanc. L'ordre n'est donc pas une commodité : la
// moitié Android doit passer après, sans quoi le travail est défait en
// silence. L'enchaîner ici est la seule façon de ne pas dépendre de la mémoire
// de celui qui lance la commande.
if (!process.argv.includes("--sans-derives")) {
  execFileSync("npx", ["tauri", "icon", "src-tauri/icons/source.png"], {
    cwd: RACINE,
    stdio: "inherit",
  });
}

// ── Android ─────────────────────────────────────────────────────────────────

/** Toile d'une icône adaptative, en dp. */
const TOILE = 108;
/** Zone garantie visible quel que soit le masque du lanceur, en dp. */
const ZONE_SURE = 72;

/**
 * Le glyphe, en coordonnées de la toile de 108.
 *
 * Il occupe dans la zone sûre la même part que dans le corps de l'icône macOS :
 * la note doit avoir l'air d'être la même, pas seulement d'être la même forme.
 */
const ECHELLE = (ZONE_SURE * GLYPH_RATIO) / GLYPH_BOX;
const vers = (valeur) => +(TOILE / 2 + (valeur - GLYPH_BOX / 2) * ECHELLE).toFixed(3);
const taille = (valeur) => +(valeur * ECHELLE).toFixed(3);

const rect = (r) => `M${vers(r.x0)},${vers(r.y0)} H${vers(r.x1)} V${vers(r.y1)} H${vers(r.x0)} Z`;
/**
 * Le disque, parcouru dans **le même sens** que les rectangles.
 *
 * Les trois formes ne font qu'un seul tracé, et un tracé se remplit selon la
 * règle du non-zéro : deux contours parcourus en sens contraire s'annulent là
 * où ils se recouvrent. Le disque tournant à l'envers, la hampe creusait un
 * coin blanc dans la tête au lieu de s'y fondre — un défaut qu'aucune relecture
 * du dessin ne révèle, puisqu'il ne naît qu'au moment du remplissage.
 */
const disque = (() => {
  const r = taille(TETE.r);
  const cx = vers(TETE.x);
  const cy = vers(TETE.y);
  return `M${+(cx - r).toFixed(3)},${cy} a${r},${r} 0 1,1 ${2 * r},0 a${r},${r} 0 1,1 ${-2 * r},0 Z`;
})();

const NOTE = `${disque} ${rect(HAMPE)} ${rect(CROCHET)}`;

const vecteur = (corps) => `<?xml version="1.0" encoding="utf-8"?>
<!-- Généré par tools/icon/generate-icon.mjs — ne pas modifier à la main. -->
<vector xmlns:android="http://schemas.android.com/apk/res/android"
    android:width="${TOILE}dp"
    android:height="${TOILE}dp"
    android:viewportWidth="${TOILE}"
    android:viewportHeight="${TOILE}">
${corps}
</vector>
`;

const hex = ([r, g, b]) =>
  `#FF${[r, g, b].map((c) => c.toString(16).padStart(2, "0")).join("")}`.toUpperCase();

// Le dégradé court d'un coin à l'autre de la **zone sûre**, et non de la toile :
// c'est la partie que le masque laisse voir, et c'est donc là que doit tenir
// toute la plage de couleurs. Au-delà, Android prolonge les teintes extrêmes.
const MARGE = (TOILE - ZONE_SURE) / 2;

ecrire(
  `${ANDROID}/drawable/ic_launcher_background.xml`,
  vecteur(`    <path android:pathData="M0,0 H${TOILE} V${TOILE} H0 Z">
        <aapt:attr xmlns:aapt="http://schemas.android.com/aapt" name="android:fillColor">
            <gradient
                android:type="linear"
                android:startX="${MARGE}" android:startY="${MARGE}"
                android:endX="${TOILE - MARGE}" android:endY="${TOILE - MARGE}"
                android:startColor="${hex(ACCENT)}"
                android:endColor="${hex(ACCENT_ALT)}" />
        </aapt:attr>
    </path>`),
);

ecrire(
  `${ANDROID}/drawable/ic_launcher_foreground.xml`,
  vecteur(`    <path android:fillColor="${hex(BASE)}" android:pathData="${NOTE}" />`),
);

// Icône thématique d'Android 13 : le système la teinte lui-même, la couleur
// déclarée ici ne sert qu'à définir la forme.
ecrire(
  `${ANDROID}/drawable/ic_launcher_monochrome.xml`,
  vecteur(`    <path android:fillColor="#FF000000" android:pathData="${NOTE}" />`),
);

ecrire(
  `${ANDROID}/mipmap-anydpi-v26/ic_launcher.xml`,
  `<?xml version="1.0" encoding="utf-8"?>
<!-- Généré par tools/icon/generate-icon.mjs — ne pas modifier à la main. -->
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
  <background android:drawable="@drawable/ic_launcher_background"/>
  <foreground android:drawable="@drawable/ic_launcher_foreground"/>
  <monochrome android:drawable="@drawable/ic_launcher_monochrome"/>
</adaptive-icon>
`,
);

// Les PNG ne servent qu'aux surfaces qui ignorent les icônes adaptatives.
// `minSdkVersion` valant 26, aucun lanceur ne les regarde — mais un raccourci
// ou un widget tiers peut encore les demander, et une image périmée est pire
// qu'une image absente.
const DENSITES = {
  "mipmap-mdpi": 48,
  "mipmap-hdpi": 72,
  "mipmap-xhdpi": 96,
  "mipmap-xxhdpi": 144,
  "mipmap-xxxhdpi": 192,
};

for (const [dossier, taillePx] of Object.entries(DENSITES)) {
  ecrire(`${ANDROID}/${dossier}/ic_launcher.png`, dessiner(taillePx, taillePx, "arrondi"));
  ecrire(`${ANDROID}/${dossier}/ic_launcher_round.png`, dessiner(taillePx, taillePx, "cercle"));

  // Le motif complet vivait dans la couche de premier plan, où le masque le
  // rognait. Il est désormais vectoriel et séparé du fond.
  rmSync(`${ANDROID}/${dossier}/ic_launcher_foreground.png`, { force: true });
}

// Le fond était une couleur — du blanc, qui débordait de tous les côtés du
// motif. Il est maintenant le dégradé lui-même.
rmSync(`${ANDROID}/values/ic_launcher_background.xml`, { force: true });

console.log(`✓ Android : icône adaptative (fond dégradé, note, thématique) + ${Object.keys(DENSITES).length} densités`);
