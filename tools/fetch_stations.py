#!/usr/bin/env python3
"""Relève la liste des stations de nombres publiée par Priyom.org.

L'agenda (`update_catalog.py`) ne connaît que les stations à horaires fixes.
Les stations en émission continue, comme S28 « Le Bourdon », et les stations
inactives n'y figurent pas : leurs fiches viennent des pages de Priyom.

Usage :
    python tools/fetch_stations.py [--out app/assets/stations.json] [--cache .cache]

Les pages sont lues une seule fois et mises en cache, pour ne pas solliciter
Priyom inutilement. Le contenu reste la propriété de Priyom.org
(CC BY-NC-SA 4.0), cité dans THIRD_PARTY.md.
"""

from __future__ import annotations

import argparse
import html
import json
import re
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

BASE = "https://priyom.org"
INDEX = f"{BASE}/number-stations"
MILITARY = f"{BASE}/military-stations"
USER_AGENT = "wavecraft-catalog/1.0 (+https://nthill.be/wavecraft/)"
FAMILIES = ["english", "german", "slavic", "morse", "digital", "other"]
DELAY = 0.3  # seconde entre deux pages, par courtoisie

ROW_RE = re.compile(r"(?s)<tr>\s*<td[^>]*>(.*?)</td>\s*<td[^>]*>(.*?)</td>")
FREQ_RE = re.compile(r"(\d{3,6})\s*kHz", re.I)


def text(fragment: str) -> str:
    """Réduit un fragment HTML à son texte."""
    fragment = re.sub(r"(?s)<(script|style)\b.*?</\1>", "", fragment)
    fragment = re.sub(r"(?s)<[^>]+>", " ", fragment)
    return html.unescape(re.sub(r"\s+", " ", fragment)).strip()


class Fetcher:
    def __init__(self, cache: Path) -> None:
        self.cache = cache
        self.cache.mkdir(parents=True, exist_ok=True)

    def get(self, url: str) -> str:
        key = self.cache / (re.sub(r"[^\w]+", "_", url).strip("_") + ".html")
        if key.exists():
            return key.read_text(encoding="utf-8")
        request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
        with urllib.request.urlopen(request, timeout=45) as response:
            page = response.read().decode("utf-8", "replace")
        key.write_text(page, encoding="utf-8")
        time.sleep(DELAY)
        return page


def station_urls(fetcher: Fetcher) -> dict[str, str]:
    """Adresses des fiches de station, par famille."""
    found: dict[str, str] = {}
    for family in FAMILIES:
        try:
            page = fetcher.get(f"{INDEX}/{family}")
        except urllib.error.URLError as err:
            print(f"  famille {family} injoignable : {err}", file=sys.stderr)
            continue
        for path in re.findall(rf'href="(/number-stations/{family}/[^"#?]+)"', page):
            # On ne garde que les fiches de station, pas leurs sous-pages
            # (schedule, history, logs…).
            parts = path.strip("/").split("/")
            if len(parts) == 3:
                found[path] = f"{BASE}{path}"
    return found


def military_urls(fetcher: Fetcher) -> dict[str, str]:
    """Adresses des stations militaires : Le Bourdon, Le Pip, La Roue grinçante…

    Elles n'ont pas d'horaire — elles émettent en continu — et n'apparaissent
    donc jamais à l'agenda, alors que ce sont les plus faciles à entendre.
    """
    found: dict[str, str] = {}
    try:
        index = fetcher.get(MILITARY)
    except urllib.error.URLError as err:
        print(f"  stations militaires injoignables : {err}", file=sys.stderr)
        return found

    countries = sorted(set(re.findall(r'href="(/military-stations/[^"#?]+)"', index)))
    for country in countries:
        try:
            page = fetcher.get(f"{BASE}{country}")
        except urllib.error.URLError:
            continue
        for path in re.findall(r'href="(/military-stations/[^"#?]+)"', page):
            if path.strip("/").count("/") >= 2:  # pays/station
                found[path] = f"{BASE}{path}"
    return found


def parse_station(page: str, url: str) -> dict | None:
    fields = {text(label): text(value) for label, value in ROW_RE.findall(page)}
    station_id = fields.get("Enigma ID", "").strip()
    if station_id in ("None", "N/A", "-", "?"):
        station_id = ""  # fiche sans identifiant Enigma
    if not station_id or len(station_id) > 12:
        # Les stations militaires n'ont pas toutes d'identifiant Enigma : on les
        # garde si leur fiche annonce une fréquence et un statut.
        if not FREQ_RE.search(fields.get("Frequencies", "")):
            return None
        title = re.search(r"<h2>(.*?)</h2>", page)
        station_id = text(title.group(1))[:40] if title else ""
        if station_id in ("", "None"):
            # Certaines fiches militaires n'ont ni identifiant Enigma ni titre :
            # on les nomme d'après leur adresse (« the-air-horn » → « The Air Horn »).
            station_id = url.rstrip("/").rsplit("/", 1)[-1].replace("-", " ").title()

    frequencies = sorted(
        {int(f) for f in FREQ_RE.findall(fields.get("Frequencies", ""))}
    )
    status = fields.get("Status", "").strip().lower() or "unknown"

    # Première phrase de la description, pour la fiche affichée dans Wavecraft.
    article = re.search(r"(?s)<main>.*?</main>", page)
    summary = ""
    if article:
        body = re.sub(r"(?s)<table>.*?</table>", " ", article.group(0))
        paragraphs = [text(p) for p in re.findall(r"(?s)<p[^>]*>(.*?)</p>", body)]
        summary = next((p for p in paragraphs if len(p) > 60), "")

    return {
        "id": station_id,
        "name": fields.get("Name", "").strip(),
        "family": url.strip("/").split("/")[-2],
        "status": status,
        "voice": fields.get("Voice", "").strip(),
        "mode": fields.get("Emission mode", "").strip(),
        "location": fields.get("Location", "").strip(),
        "frequencies_khz": frequencies,
        "counterparts": [
            c.strip()
            for c in fields.get("Known counterpart stations", "").split(",")
            if c.strip()
        ],
        "summary": summary[:600],
        "url": url,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    root = Path(__file__).resolve().parents[1]
    parser.add_argument("--out", type=Path, default=root / "app" / "assets" / "stations.json")
    parser.add_argument("--cache", type=Path, default=root / ".cache" / "priyom")
    args = parser.parse_args()

    fetcher = Fetcher(args.cache)
    urls = station_urls(fetcher)
    urls.update(military_urls(fetcher))
    print(f"{len(urls)} fiches de station repérées")

    stations = []
    for path, url in sorted(urls.items()):
        try:
            station = parse_station(fetcher.get(url), url)
        except urllib.error.URLError as err:
            print(f"  {path} : {err}", file=sys.stderr)
            continue
        if station:
            stations.append(station)

    if len(stations) < 50:
        print(f"Relevé trop maigre ({len(stations)} fiches) : fichier inchangé.", file=sys.stderr)
        return 1

    actives = sum(1 for s in stations if s["status"] == "active")
    print(f"{len(stations)} stations, dont {actives} actives")

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(
        json.dumps(
            {
                "source": INDEX,
                "licence": "Priyom.org, CC BY-NC-SA 4.0",
                "stations": sorted(stations, key=lambda s: s["id"]),
            },
            ensure_ascii=False,
            indent=1,
        )
        + "\n",
        encoding="utf-8",
    )
    print(f"Écrit : {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
