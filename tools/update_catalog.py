#!/usr/bin/env python3
"""Régénère le catalogue des stations de nombres de Wavecraft depuis Priyom.org.

Le catalogue est construit uniquement à partir des émissions réellement
annoncées à l'agenda Priyom : aucune fréquence, aucun mode n'est inventé.

Usage :
    python tools/update_catalog.py [--days 14] [--out app/assets/catalog.json]

Garde-fous :
  * les requêtes sont découpées en tranches de 24 h, car l'agenda plafonne
    silencieusement ses réponses à 250 événements et ne pagine pas ;
  * le script refuse d'écrire si le relevé est anormalement maigre, pour ne pas
    remplacer un bon catalogue par une panne d'API ;
  * les fiches rédigées à la main (`notes`, `name`, `status`) du catalogue
    existant sont conservées.

Lancé chaque lundi par .github/workflows/update-catalog.yml.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.error
import urllib.request
from collections import defaultdict
from datetime import datetime, timedelta, timezone
from pathlib import Path

CALENDAR_URL = "https://calendar2.priyom.org/events"
MAX_RESULTS = 250  # plafond par requête, sans pagination
USER_AGENT = "wavecraft-catalog/1.0 (+https://nthill.be/wavecraft/)"

# Même lecture des libellés que le site Priyom et que app/src/priyom.rs.
SUMMARY_RE = re.compile(
    r"^([\w /-]+?) (?:Search|(\d+) ?[kK][hH][zZ](?:.*?[kK][hH][zZ])?? ?([A-Z][A-Z/]+)?)"
    r"(?:.*?\[Target: ([\w -]+)\])?"
)

# Largeur de filtre habituelle par mode, en Hz.
BANDWIDTH = {
    "USB": 2700,
    "LSB": 2700,
    "CW": 500,
    "AM": 6000,
    "FM": 12500,
    "NFM": 12500,
    "RTTY": 1000,
    "FSK": 1000,
    "MFSK": 2700,
    "PSK": 1000,
    "DIGITAL": 2700,
}

# Une émission est retenue au planning si elle revient au moins deux fois sur la
# période observée, pour écarter les émissions ponctuelles.
MIN_OCCURRENCES = 2


def iso(t: datetime) -> str:
    return t.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.000Z")


def fetch_window(start: datetime, end: datetime) -> list[dict]:
    url = f"{CALENDAR_URL}?timeMin={iso(start)}&timeMax={iso(end)}"
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.loads(resp.read()).get("items", [])


def fetch_chunk(start: datetime, end: datetime) -> list[dict]:
    """Lit une tranche, en la coupant en deux si la réponse touche le plafond."""
    items = fetch_window(start, end)
    if len(items) < MAX_RESULTS or (end - start) <= timedelta(minutes=30):
        return items
    middle = start + (end - start) / 2
    return fetch_chunk(start, middle) + fetch_chunk(middle, end)


def fetch_range(start: datetime, end: datetime) -> list[dict]:
    """Relevé complet, par tranches de 24 h, doublons des jonctions retirés."""
    seen: set[tuple[str, str]] = set()
    out: list[dict] = []
    cursor = start
    while cursor < end:
        stop = min(cursor + timedelta(hours=24), end)
        for item in fetch_chunk(cursor, stop):
            key = (item.get("summary", ""), item["start"]["dateTime"])
            if key not in seen:
                seen.add(key)
                out.append(item)
        cursor = stop
    return out


def parent_station(station_id: str) -> str:
    """Rattache une variante à sa station mère : `F03j` → `F03`, `P03k` → `P03`.

    Les identifiants Priyom suivent le schéma lettre + deux chiffres, suivi
    éventuellement d'une lettre minuscule de variante.
    """
    match = re.match(r"^([A-Z]+\d+)[a-z]?$", station_id)
    return match.group(1) if match else station_id


def parse_event(item: dict) -> dict | None:
    summary = (item.get("summary") or "").strip()
    match = SUMMARY_RE.match(summary)
    if not match:
        return None
    station, freq, mode, target = match.groups()
    station = station.strip()
    if not station:
        return None
    start = datetime.fromisoformat(item["start"]["dateTime"].replace("Z", "+00:00"))
    return {
        "station": station,
        "parent": parent_station(station),
        "frequency_khz": int(freq) if freq else None,
        "mode": mode,
        "target": target.strip() if target else None,
        "start": start,
        "summary": summary,
    }


def mode_of(raw: str | None) -> str | None:
    """Ramène un mode annoncé à un mode de réception connu.

    Priyom écrit parfois « J3E (USB) », « USB; 14-tone MFSK » ou « USB, CW » :
    seul le premier mode reconnu nous intéresse pour le préréglage.
    """
    if not raw:
        return None
    for token in re.split(r"[;,/()\s]+", raw.upper()):
        if token in BANDWIDTH:
            return token
    return None


def merge_roster(stations: list[dict], roster_path: Path) -> list[dict]:
    """Ajoute les fiches de Priyom au planning relevé à l'agenda.

    L'agenda ne connaît que les stations à horaires fixes. Les stations en
    émission continue — S28 « Le Bourdon » et les autres stations militaires —
    n'y figurent jamais, alors que ce sont les plus faciles à entendre.
    """
    if not roster_path.exists():
        print(f"Fiches absentes ({roster_path}) : catalogue limité à l'agenda.", file=sys.stderr)
        return stations

    roster = json.loads(roster_path.read_text(encoding="utf-8")).get("stations", [])
    by_id = {s["id"]: s for s in stations}

    for fiche in roster:
        station = by_id.get(fiche["id"])
        if station is None:
            station = {
                "id": fiche["id"],
                "variants": [],
                "targets": [],
                "presets": [],
                "slots": [],
                "first_seen": "",
                "last_seen": "",
                "notes": "",
            }
            by_id[fiche["id"]] = station
            stations.append(station)

        station["name"] = fiche.get("name") or station.get("name") or fiche["id"]
        station["family"] = fiche.get("family", "")
        station["status"] = fiche.get("status", station.get("status", "active"))
        station["voice"] = fiche.get("voice", "")
        station["location"] = fiche.get("location", "")
        station["summary"] = fiche.get("summary", "")
        station["url"] = fiche.get("url", "")
        station["counterparts"] = fiche.get("counterparts", [])

        # Les fréquences annoncées sur la fiche complètent les préréglages :
        # ce sont les seules disponibles pour une station en émission continue.
        mode = mode_of(fiche.get("mode"))
        known = {(p["frequency_khz"], p["mode"]) for p in station["presets"]}
        for freq in fiche.get("frequencies_khz", []):
            if (freq, mode or "USB") in known:
                continue
            station["presets"].append(
                {
                    "frequency_khz": freq,
                    "mode": mode or "USB",
                    "bandwidth_hz": BANDWIDTH.get(mode or "USB", 2700),
                    "squelch": False,
                    "seen": 0,  # fréquence de fiche, pas de relevé d'agenda
                }
            )
        station["continuous"] = bool(fiche.get("frequencies_khz")) and not station["slots"]

    return stations


def build_catalog(events: list[dict], days: int, previous: dict, roster_path: Path) -> dict:
    kept = [e for e in (parse_event(i) for i in events) if e]

    old_stations = {s["id"]: s for s in previous.get("stations", [])}
    by_station: dict[str, list[dict]] = defaultdict(list)
    for event in kept:
        by_station[event["parent"]].append(event)

    stations = []
    for station_id, station_events in sorted(by_station.items()):
        # Créneaux récurrents : même jour de semaine, même heure UTC, même
        # fréquence et même mode, vus au moins deux fois.
        slots_count: dict[tuple, int] = defaultdict(int)
        for event in station_events:
            if event["frequency_khz"] is None:
                continue  # émission « Search », sans fréquence annoncée
            key = (
                event["start"].weekday(),
                event["start"].strftime("%H:%M"),
                event["frequency_khz"],
                event["mode"] or "",
            )
            slots_count[key] += 1

        slots = [
            {
                "weekday": weekday,
                "utc": utc,
                "frequency_khz": freq,
                "mode": mode or None,
                "count": count,
            }
            for (weekday, utc, freq, mode), count in sorted(slots_count.items())
            if count >= MIN_OCCURRENCES or days < 7
        ]

        # Préréglages : les couples fréquence/mode les plus fréquents.
        preset_count: dict[tuple, int] = defaultdict(int)
        for event in station_events:
            if event["frequency_khz"] is not None:
                preset_count[(event["frequency_khz"], event["mode"] or "USB")] += 1
        presets = [
            {
                "frequency_khz": freq,
                "mode": mode,
                "bandwidth_hz": BANDWIDTH.get(mode, 2700),
                "squelch": False,
                "seen": count,
            }
            for (freq, mode), count in sorted(
                preset_count.items(), key=lambda kv: (-kv[1], kv[0][0])
            )
        ]

        variants = sorted({e["station"] for e in station_events if e["station"] != station_id})
        targets = sorted({e["target"] for e in station_events if e["target"]})
        old = old_stations.get(station_id, {})

        stations.append(
            {
                "id": station_id,
                "name": old.get("name") or station_id,
                "variants": variants,
                "targets": targets,
                "presets": presets,
                "slots": slots,
                "status": old.get("status", "active"),
                "notes": old.get("notes", ""),
                "first_seen": old.get("first_seen")
                or min(e["start"] for e in station_events).strftime("%Y-%m-%d"),
                "last_seen": max(e["start"] for e in station_events).strftime("%Y-%m-%d"),
            }
        )

    # Les stations du catalogue précédent qui n'émettent plus sont conservées,
    # marquées inactives : l'historique reste consultable.
    current = {s["id"] for s in stations}
    for station_id, old in sorted(old_stations.items()):
        if station_id not in current:
            old = dict(old)
            old["status"] = old.get("status") if old.get("status") != "active" else "inactive"
            stations.append(old)

    stations = merge_roster(stations, roster_path)

    return {
        "version": previous.get("version", 0) + 1,
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "source": CALENDAR_URL,
        "window_days": days,
        "events_seen": len(kept),
        "stations": sorted(stations, key=lambda s: s["id"]),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--days", type=int, default=14, help="profondeur du relevé")
    parser.add_argument(
        "--out",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "app" / "assets" / "catalog.json",
    )
    parser.add_argument(
        "--roster",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "app" / "assets" / "stations.json",
        help="fiches de station relevées par fetch_stations.py",
    )
    parser.add_argument(
        "--min-events",
        type=int,
        default=400,
        help="refuse d'écrire en dessous de ce nombre d'émissions relevées",
    )
    args = parser.parse_args()

    start = datetime.now(timezone.utc)
    end = start + timedelta(days=args.days)
    try:
        events = fetch_range(start, end)
    except urllib.error.URLError as err:
        print(f"Agenda Priyom injoignable : {err}", file=sys.stderr)
        return 1

    print(f"{len(events)} émissions relevées sur {args.days} jours")
    if len(events) < args.min_events:
        print(
            f"Relevé trop maigre (< {args.min_events}) : catalogue inchangé.",
            file=sys.stderr,
        )
        return 1

    previous = {}
    if args.out.exists():
        previous = json.loads(args.out.read_text(encoding="utf-8"))

    catalog = build_catalog(events, args.days, previous, args.roster)
    slots = sum(len(s["slots"]) for s in catalog["stations"])
    print(f"{len(catalog['stations'])} stations, {slots} créneaux récurrents")

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(
        json.dumps(catalog, ensure_ascii=False, indent=1) + "\n", encoding="utf-8"
    )
    print(f"Écrit : {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
