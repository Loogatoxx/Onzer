#!/bin/sh
# ═══════════════════════════════════════════════════════════════════════════
#  Garantit que le volume de cache de compilation est monté.
#
#  Le cache Rust (plusieurs gigaoctets) vit sur le SSD Lexar, dans une image
#  disque APFS — voir .cargo/config.toml pour le raisonnement complet.
#  Ce script est appelé automatiquement avant chaque `npm run app`.
#
#  Idempotent : ne fait rien si le volume est déjà monté.
# ═══════════════════════════════════════════════════════════════════════════
set -e

VOLUME="/Volumes/OnzerBuild"
BUNDLE="/Volumes/Lexar/Perso/Projet/.onzer-build-cache.sparsebundle"

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
    echo "✖ Image de cache introuvable : $BUNDLE" >&2
    echo "  Le SSD Lexar est-il branché ?" >&2
    echo "  Pour la recréer :" >&2
    echo "  hdiutil create -type SPARSEBUNDLE -fs APFS -size 60g -volname OnzerBuild '$BUNDLE'" >&2
    exit 1
fi

hdiutil attach "$BUNDLE" -mountpoint "$VOLUME" -nobrowse -quiet
echo "✓ Cache de compilation monté sur $VOLUME"
