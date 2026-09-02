#!/bin/sh
# ═══════════════════════════════════════════════════════════════════════════
#  Garantit que le volume de cache de compilation est monté.
#
#  Le cache Rust (plusieurs gigaoctets) vit sur le SSD Lexar, dans une image
#  disque APFS — voir .cargo/config.toml pour le raisonnement complet.
#  Ce script est appelé automatiquement avant chaque `npm run app`.
#
#  Idempotent : ne fait rien si le volume est déjà monté.
#
#  ── Pourquoi il sait se réparer ──────────────────────────────────────────
#
#  Débrancher le SSD pendant que l'image est montée corrompt son superbloc
#  APFS, définitivement : `fsck_apfs` échoue à le lire. C'est arrivé deux
#  fois, et les deux fois le symptôme était le même — `npm run app` sortait
#  en silence, sans un mot d'explication.
#
#  Or ce volume ne contient **que du cache de compilation** : rien qui ne se
#  reconstruise tout seul. Le perdre coûte quelques minutes de recompilation,
#  pas une donnée. Le script le recrée donc lui-même plutôt que de laisser
#  l'utilisateur devant une panne qu'il ne peut pas diagnostiquer.
# ═══════════════════════════════════════════════════════════════════════════
set -e

VOLUME="/Volumes/OnzerBuild"
BUNDLE="/Volumes/Lexar/Perso/Projet/.onzer-build-cache.sparsebundle"
SIZE="60g"

creer() {
    hdiutil create -type SPARSEBUNDLE -fs APFS -size "$SIZE" -volname OnzerBuild \
        "$BUNDLE" -quiet
}

# Déjà monté : rien à faire.
if mount | grep -q " on $VOLUME "; then
    exit 0
fi

# Cargo a pu créer un dossier vide à l'emplacement du point de montage lors
# d'une compilation faite volume démonté. Il empêcherait le montage.
if [ -d "$VOLUME" ] && [ -z "$(ls -A "$VOLUME" 2>/dev/null)" ]; then
    rmdir "$VOLUME" 2>/dev/null || true
fi

if [ ! -d "$BUNDLE" ]; then
    if [ ! -d "$(dirname "$BUNDLE")" ]; then
        echo "✖ Le SSD Lexar n'est pas branché : $(dirname "$BUNDLE") est introuvable." >&2
        exit 1
    fi

    echo "→ Cache de compilation absent, création…"
    creer
fi

if hdiutil attach "$BUNDLE" -mountpoint "$VOLUME" -nobrowse -quiet 2>/dev/null; then
    echo "✓ Cache de compilation monté sur $VOLUME"
    exit 0
fi

# L'image existe mais refuse de se monter : superbloc corrompu, presque
# toujours après un débranchement à chaud. On la remplace.
echo "⚠ Cache de compilation illisible (débranchement à chaud ?), reconstruction…" >&2
rm -rf "$BUNDLE"
creer

if hdiutil attach "$BUNDLE" -mountpoint "$VOLUME" -nobrowse -quiet; then
    echo "✓ Cache de compilation recréé sur $VOLUME — la prochaine compilation sera complète."
    exit 0
fi

echo "✖ Impossible de monter le cache de compilation, même recréé." >&2
exit 1
