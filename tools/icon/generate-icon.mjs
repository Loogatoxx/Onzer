/**
 * Génère l'icône source d'Onzer (PNG 1024×1024), sans aucune dépendance.
 *
 * Le fichier produit sert d'entrée à `npm run tauri icon`, qui en dérive
 * l'ensemble des tailles et le `.icns` macOS.
 *
 * Motif : carré aux angles arrondis, dégradé violet → cyan repris des jetons de
 * design (`src/styles/globals.css`), sur lequel se détache une note de musique
 * dans le noir de l'application.
 *
 *   node tools/icon/generate-icon.mjs
 */

import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SIZE = 1024;
const SAMPLES = 3; // suréchantillonnage par axe → 9 échantillons par pixel

// ── Gabarit d'icône macOS ───────────────────────────────────────────────────
//
// macOS n'affiche PAS une icône bord à bord : Apple impose une zone de sécurité
// transparente autour du motif. Sur une toile de 1024 px, le corps de l'icône
// mesure 824 px, centré, soit 100 px de marge de chaque côté.
//
// Sans cette marge, l'icône paraît nettement plus grosse que ses voisines dans
// le Dock et vient mordre sur l'indicateur d'application ouverte.
const BODY_SIZE = 824;
const BODY_OFFSET = (SIZE - BODY_SIZE) / 2;

// Jetons repris de globals.css, pour que l'icône et l'interface parlent la
// même langue visuelle.
const ACCENT = [0x8b, 0x5c, 0xf6]; // --color-accent
const ACCENT_ALT = [0x22, 0xd3, 0xee]; // --color-accent-alt
const BASE = [0x0a, 0x0a, 0x0c]; // --color-base

// ── Géométrie ───────────────────────────────────────────────────────────────

// Rayon d'arrondi du gabarit Apple, proportionnel au corps et non à la toile.
const CORNER_RADIUS = BODY_SIZE * 0.225;

/** Le glyphe est décrit dans un repère 24×24, puis mis à l'échelle. */
const GLYPH_BOX = 24;
const GLYPH_SCALE = (BODY_SIZE * 0.52) / GLYPH_BOX;

/** Distance signée à un rectangle aux angles arrondis. Négative à l'intérieur. */
function distanceToRoundedRect(x, y, width, height, radius) {
  const dx = Math.abs(x - width / 2) - (width / 2 - radius);
  const dy = Math.abs(y - height / 2) - (height / 2 - radius);
  const outside = Math.hypot(Math.max(dx, 0), Math.max(dy, 0));
  return outside + Math.min(Math.max(dx, dy), 0) - radius;
}

/** Le point (gx, gy), exprimé dans le repère 24×24, est-il dans la note ? */
function isInsideGlyph(gx, gy) {
  const head = (gx - 10) ** 2 + (gy - 17.4) ** 2 <= 4.2 ** 2;
  const stem = gx >= 13.1 && gx <= 15.1 && gy >= 3 && gy <= 17.6;
  const flag = gx >= 13.1 && gx <= 20.4 && gy >= 3 && gy <= 6.6;
  return head || stem || flag;
}

/** Dégradé diagonal entre les deux accents, calé sur le corps de l'icône. */
function gradientAt(x, y) {
  const u = (x - BODY_OFFSET) / BODY_SIZE;
  const v = (y - BODY_OFFSET) / BODY_SIZE;
  const t = Math.min(1, Math.max(0, u * 0.5 + v * 0.5));
  return [
    Math.round(ACCENT[0] + (ACCENT_ALT[0] - ACCENT[0]) * t),
    Math.round(ACCENT[1] + (ACCENT_ALT[1] - ACCENT[1]) * t),
    Math.round(ACCENT[2] + (ACCENT_ALT[2] - ACCENT[2]) * t),
  ];
}

// ── Rendu ───────────────────────────────────────────────────────────────────

/** Couleur RGBA d'un pixel, moyennée sur SAMPLES² sous-échantillons. */
function renderPixel(px, py) {
  let r = 0;
  let g = 0;
  let b = 0;
  let a = 0;

  for (let sy = 0; sy < SAMPLES; sy++) {
    for (let sx = 0; sx < SAMPLES; sx++) {
      const x = px + (sx + 0.5) / SAMPLES;
      const y = py + (sy + 0.5) / SAMPLES;

      // Hors du corps de l'icône : entièrement transparent. C'est cette zone
      // qui donne à l'icône sa taille apparente correcte dans le Dock.
      const bodyX = x - BODY_OFFSET;
      const bodyY = y - BODY_OFFSET;
      if (
        bodyX < 0 ||
        bodyY < 0 ||
        bodyX > BODY_SIZE ||
        bodyY > BODY_SIZE ||
        distanceToRoundedRect(bodyX, bodyY, BODY_SIZE, BODY_SIZE, CORNER_RADIUS) > 0
      ) {
        continue;
      }

      const gx = (x - SIZE / 2) / GLYPH_SCALE + GLYPH_BOX / 2;
      const gy = (y - SIZE / 2) / GLYPH_SCALE + GLYPH_BOX / 2;

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

function encodePng(pixels) {
  const signature = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);

  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(SIZE, 0);
  ihdr.writeUInt32BE(SIZE, 4);
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

// ── Exécution ───────────────────────────────────────────────────────────────

// Chaque ligne est préfixée d'un octet de filtre (0 = aucun).
const raster = Buffer.alloc(SIZE * (SIZE * 4 + 1));
let offset = 0;

for (let y = 0; y < SIZE; y++) {
  raster[offset++] = 0;
  for (let x = 0; x < SIZE; x++) {
    const [r, g, b, a] = renderPixel(x, y);
    raster[offset++] = r;
    raster[offset++] = g;
    raster[offset++] = b;
    raster[offset++] = a;
  }
}

const output = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "../../src-tauri/icons/source.png",
);

mkdirSync(dirname(output), { recursive: true });
writeFileSync(output, encodePng(raster));

console.log(`✓ Icône source générée : ${output} (${SIZE}×${SIZE})`);
