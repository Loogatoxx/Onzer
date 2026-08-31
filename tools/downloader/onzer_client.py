#!/usr/bin/env python3
"""Client de l'API d'import d'Onzer — sans aucune dépendance.

À utiliser tel quel en ligne de commande, ou à importer comme module depuis ton
script de téléchargement :

    from onzer_client import Onzer

    onzer = Onzer()
    if not onzer.exists("Digital Love", artist="Daft Punk"):
        chemin = telecharger(...)
        onzer.import_file(chemin, title="Digital Love", artist="Daft Punk")

Contrat complet : docs/IMPORT_API.md
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

BASE_URL = "http://127.0.0.1:47811"

TOKEN_PATH = (
    Path.home()
    / "Library/Application Support/com.loogatoxx.onzer/onzer.token"
)


class OnzerError(RuntimeError):
    """Erreur renvoyée par Onzer, ou impossibilité de le joindre."""


class Onzer:
    def __init__(self, base_url: str = BASE_URL, token: str | None = None) -> None:
        self.base_url = base_url.rstrip("/")
        self.token = token or self._read_token()

    @staticmethod
    def _read_token() -> str:
        # Le jeton est créé au premier démarrage d'Onzer. Son absence signifie
        # presque toujours que l'application n'a jamais été lancée.
        token_path = Path(os.environ.get("ONZER_TOKEN_FILE", TOKEN_PATH))
        try:
            return token_path.read_text(encoding="utf-8").strip()
        except OSError as error:
            raise OnzerError(
                f"jeton introuvable ({token_path}). Onzer a-t-il déjà été lancé ?"
            ) from error

    def _request(self, method: str, path: str, body: dict | None = None) -> dict:
        request = urllib.request.Request(
            f"{self.base_url}{path}",
            method=method,
            data=json.dumps(body).encode("utf-8") if body is not None else None,
            headers={
                "Authorization": f"Bearer {self.token}",
                **({"Content-Type": "application/json"} if body is not None else {}),
            },
        )

        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                return json.loads(response.read())
        except urllib.error.HTTPError as error:
            # Onzer renvoie toujours {"error": "..."} : on remonte ce message
            # plutôt qu'un code HTTP nu.
            try:
                detail = json.loads(error.read()).get("error", "")
            except Exception:
                detail = ""
            raise OnzerError(f"HTTP {error.code} : {detail or error.reason}") from error
        except urllib.error.URLError as error:
            raise OnzerError(
                f"Onzer injoignable sur {self.base_url} — l'application est-elle lancée ?"
            ) from error

    # ── Routes ──────────────────────────────────────────────────────────

    def health(self) -> dict:
        return self._request("GET", "/api/v1/health")

    def exists(
        self,
        title: str,
        artist: str | None = None,
        duration_ms: int | None = None,
    ) -> bool:
        """Le morceau est-il déjà dans la bibliothèque ?

        À appeler AVANT de télécharger : c'est tout l'intérêt de la route.
        """
        params: dict[str, str] = {"title": title}
        if artist:
            params["artist"] = artist
        if duration_ms:
            params["durationMs"] = str(duration_ms)

        query = urllib.parse.urlencode(params)
        return bool(self._request("GET", f"/api/v1/exists?{query}")["exists"])

    def import_file(self, path: str | Path, **metadata) -> dict:
        """Fait entrer un fichier déjà téléchargé dans la bibliothèque.

        Les métadonnées sont des *indications* : les vrais tags du fichier
        priment, sauf si Onzer a dû les deviner depuis le nom de fichier.
        """
        clean = {key: value for key, value in metadata.items() if value is not None}

        return self._request(
            "POST",
            "/api/v1/import",
            {
                "path": str(Path(path).resolve()),
                "metadata": _to_camel_case(clean) or None,
            },
        )


def _to_camel_case(metadata: dict) -> dict:
    """`album_artist` → `albumArtist`, pour coller au contrat JSON."""
    return {
        "".join(
            part if index == 0 else part.capitalize()
            for index, part in enumerate(key.split("_"))
        ): value
        for key, value in metadata.items()
    }


# ════════════════════════════════════════════════════════════════════════════
#  Ligne de commande
# ════════════════════════════════════════════════════════════════════════════


def main() -> int:
    parser = argparse.ArgumentParser(description="Client de l'API d'import d'Onzer")
    sub = parser.add_subparsers(dest="command", required=True)

    sub.add_parser("health", help="Onzer tourne-t-il ?")

    exists = sub.add_parser("exists", help="Le morceau est-il déjà présent ?")
    exists.add_argument("title")
    exists.add_argument("--artist")
    exists.add_argument("--duration-ms", type=int)

    imp = sub.add_parser("import", help="Importer un fichier audio")
    imp.add_argument("path")
    imp.add_argument("--title")
    imp.add_argument("--artist")
    imp.add_argument("--album")
    imp.add_argument("--album-artist")
    imp.add_argument("--year", type=int)
    imp.add_argument("--track-no", type=int)
    imp.add_argument("--genre")

    args = parser.parse_args()

    try:
        onzer = Onzer()

        if args.command == "health":
            print(json.dumps(onzer.health(), indent=2, ensure_ascii=False))

        elif args.command == "exists":
            found = onzer.exists(args.title, args.artist, args.duration_ms)
            print("déjà présent" if found else "absent")
            # Code de sortie exploitable dans un script shell.
            return 0 if found else 1

        elif args.command == "import":
            result = onzer.import_file(
                args.path,
                title=args.title,
                artist=args.artist,
                album=args.album,
                album_artist=args.album_artist,
                year=args.year,
                track_no=args.track_no,
                genre=args.genre,
            )
            print(json.dumps(result, indent=2, ensure_ascii=False))

    except OnzerError as error:
        print(f"✖ {error}", file=sys.stderr)
        return 2

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
