#!/bin/sh
# Signe l'APK de sortie avec la clé de publication.
#
# # Pourquoi un script plutôt qu'une ligne de commande retenue de tête
#
# Une application Android ne peut être mise à jour que par une **mise à jour
# signée de la même clé**. Signer une fois avec la clé de débogage et une fois
# avec la vraie oblige l'utilisateur à désinstaller — il perd sa base, ses
# favoris, son historique. La procédure doit donc être écrite, et la même à
# chaque fois.
#
# # Pourquoi la clé n'est pas ici
#
# Elle n'entre pas dans le dépôt, jamais : qui la possède peut publier une
# application qui se fait passer pour Onzer. Elle vit à côté du projet, sur le
# disque, et le script la demande par variables d'environnement.
#
#   ONZER_KEYSTORE=/chemin/onzer-release.jks \
#   ONZER_KEYSTORE_PASS=… \
#   sh tools/android/signer.sh [destination.apk]
set -eu

ANDROID_HOME="${ANDROID_HOME:-/opt/homebrew/share/android-commandlinetools}"
OUTILS="$(ls -d "$ANDROID_HOME"/build-tools/* | tail -1)"

ENTREE="src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk"
SORTIE="${1:-Onzer.apk}"

if [ ! -f "$ENTREE" ]; then
  echo "APK introuvable : lance d'abord « npm run android:build »." >&2
  exit 1
fi

if [ -z "${ONZER_KEYSTORE:-}" ] || [ -z "${ONZER_KEYSTORE_PASS:-}" ]; then
  echo "ONZER_KEYSTORE et ONZER_KEYSTORE_PASS doivent être définis." >&2
  exit 1
fi

# L'alignement précède la signature : zipaligner après signerait un fichier
# déjà scellé, et Android refuserait l'installation.
ALIGNE="$(mktemp -t onzer-aligne).apk"
"$OUTILS/zipalign" -f -p 4 "$ENTREE" "$ALIGNE"

"$OUTILS/apksigner" sign \
  --ks "$ONZER_KEYSTORE" \
  --ks-pass "env:ONZER_KEYSTORE_PASS" \
  --key-pass "env:ONZER_KEYSTORE_PASS" \
  --ks-key-alias "${ONZER_KEYSTORE_ALIAS:-onzer}" \
  --out "$SORTIE" "$ALIGNE"

rm -f "$ALIGNE"
"$OUTILS/apksigner" verify --print-certs "$SORTIE" | head -4
echo "signé : $SORTIE"
