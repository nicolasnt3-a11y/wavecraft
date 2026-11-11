//! Client de l'agenda Priyom.org.
//!
//! L'API renvoie les émissions annoncées sous forme d'événements de calendrier :
//!
//! ```text
//! GET https://calendar2.priyom.org/events?timeMin=<ISO Z>&timeMax=<ISO Z>
//! {"items":[{"summary":"F01 10671kHz RTTY","start":{"dateTime":"2026-11-11T10:25:00.000Z"}}]}
//! ```
//!
//! Attention : **le serveur plafonne sa réponse à 250 événements et ne pagine
//! pas**. Une requête sur 7 jours renvoie donc 250 événements au lieu des ~850
//! réels, silencieusement tronqués. Toutes les lectures passent par
//! [`fetch_range`], qui découpe la période en tranches, redécoupe toute tranche
//! qui atteint le plafond et dédoublonne les jonctions.

use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, TimeDelta, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Adresse de l'agenda public.
pub const CALENDAR_URL: &str = "https://calendar2.priyom.org/events";

/// Nombre maximal d'événements renvoyés par requête. Au-delà, la réponse est
/// tronquée sans avertissement.
pub const MAX_RESULTS: usize = 250;

/// Durée d'une tranche de relevé. Une journée reste bien sous le plafond
/// (environ 120 émissions par 24 h en 2026).
pub const CHUNK: TimeDelta = TimeDelta::hours(24);

/// Tranche minimale avant d'abandonner le redécoupage.
const MIN_CHUNK: TimeDelta = TimeDelta::minutes(30);

const USER_AGENT: &str = concat!(
    "wavecraft/",
    env!("CARGO_PKG_VERSION"),
    " (+https://nthill.be/wavecraft/)"
);

/// Une émission annoncée au calendrier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transmission {
    /// Identifiant de la station, tel qu'annoncé : `F01`, `E11`, `M12`, `S28`…
    pub station: String,
    /// Fréquence en kHz, absente pour les émissions « Search ».
    pub frequency_khz: Option<u32>,
    /// Mode annoncé : `USB`, `AM`, `CW`, `RTTY`…
    pub mode: Option<String>,
    /// Zone visée, quand elle est précisée.
    pub target: Option<String>,
    /// Début de l'émission, en UTC. Priyom ne publie pas l'heure de fin.
    pub start: DateTime<Utc>,
    /// Libellé brut, conservé pour l'affichage et le dépannage.
    pub summary: String,
}

impl Transmission {
    /// Clé de dédoublonnage aux jonctions de tranches.
    fn key(&self) -> (String, i64) {
        (self.summary.clone(), self.start.timestamp())
    }
}

#[derive(Debug, Deserialize)]
struct EventsResponse {
    #[serde(default)]
    items: Vec<Event>,
}

#[derive(Debug, Deserialize)]
struct Event {
    #[serde(default)]
    summary: String,
    start: EventStart,
}

#[derive(Debug, Deserialize)]
struct EventStart {
    #[serde(rename = "dateTime")]
    date_time: DateTime<Utc>,
}

/// Expression de lecture des libellés, reprise de celle du site Priyom.
///
/// Exemples acceptés :
/// - `F01 10671kHz RTTY`
/// - `E11 6804 kHz USB [Target: Eastern Europe]`
/// - `S28 Search`
fn summary_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?x)
            ^([\w\ /-]+?)\                      # identifiant de station
            (?:Search|(\d+)\ ?[kK][hH][zZ]      # ou fréquence en kHz
              (?:.*?[kK][hH][zZ])??             # seconde fréquence éventuelle
              (?:\ ([A-Z][A-Z/]+))?             # mode
            )
            (?:.*?\[Target:\ ([\w\ -]+)\])?     # zone visée
            ",
        )
        .expect("expression régulière de libellé valide")
    })
}

/// Lit un libellé d'événement. Renvoie `None` si la station n'est pas
/// identifiable.
pub fn parse_summary(summary: &str, start: DateTime<Utc>) -> Option<Transmission> {
    let caps = summary_re().captures(summary.trim())?;
    let station = caps.get(1)?.as_str().trim().to_string();
    if station.is_empty() {
        return None;
    }
    Some(Transmission {
        station,
        frequency_khz: caps.get(2).and_then(|m| m.as_str().parse().ok()),
        mode: caps.get(3).map(|m| m.as_str().to_string()),
        target: caps.get(4).map(|m| m.as_str().trim().to_string()),
        start,
        summary: summary.trim().to_string(),
    })
}

/// Client HTTP de l'agenda.
pub struct Priyom {
    agent: ureq::Agent,
    base_url: String,
}

impl Default for Priyom {
    fn default() -> Self {
        Self::new()
    }
}

impl Priyom {
    pub fn new() -> Self {
        Self::with_url(CALENDAR_URL)
    }

    /// Même client, sur une autre adresse : sert aux tests avec un serveur local.
    pub fn with_url(base_url: impl Into<String>) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .user_agent(USER_AGENT)
            .build();
        Self {
            agent: config.new_agent(),
            base_url: base_url.into(),
        }
    }

    /// Une requête, une tranche. Ne corrige pas la troncature : voir
    /// [`Priyom::fetch_range`].
    pub fn fetch_window(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<Transmission>> {
        let body: String = self
            .agent
            .get(&self.base_url)
            .query("timeMin", iso(from))
            .query("timeMax", iso(to))
            .call()
            .with_context(|| format!("appel de l'agenda Priyom ({from} → {to})"))?
            .body_mut()
            .read_to_string()
            .context("lecture de la réponse de l'agenda Priyom")?;

        let parsed: EventsResponse =
            serde_json::from_str(&body).context("réponse de l'agenda Priyom illisible")?;

        Ok(parsed
            .items
            .into_iter()
            .filter_map(|e| parse_summary(&e.summary, e.start.date_time))
            .collect())
    }

    /// Relevé complet d'une période, par tranches successives.
    ///
    /// Une tranche qui atteint [`MAX_RESULTS`] est coupée en deux et relue, car
    /// sa réponse est probablement tronquée. Les doublons des jonctions sont
    /// supprimés et le résultat est trié par heure de début.
    pub fn fetch_range(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<Transmission>> {
        let mut out: Vec<Transmission> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut cursor = from;

        while cursor < to {
            let end = (cursor + CHUNK).min(to);
            for tx in self.fetch_chunk(cursor, end)? {
                if seen.insert(tx.key()) {
                    out.push(tx);
                }
            }
            cursor = end;
        }

        out.sort_by_key(|t| (t.start, t.summary.clone()));
        Ok(out)
    }

    /// Lit une tranche, en la redécoupant tant que la réponse touche le plafond.
    fn fetch_chunk(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<Transmission>> {
        let items = self.fetch_window(from, to)?;
        if items.len() < MAX_RESULTS || to - from <= MIN_CHUNK {
            return Ok(items);
        }
        // Réponse vraisemblablement tronquée : on coupe la tranche en deux.
        let middle = from + (to - from) / 2;
        let mut first = self.fetch_chunk(from, middle)?;
        first.extend(self.fetch_chunk(middle, to)?);
        Ok(first)
    }
}

/// Format d'horodatage attendu par l'agenda : ISO 8601 en UTC, suffixe `Z`.
fn iso(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 11, 10, 25, 0).unwrap()
    }

    #[test]
    fn lit_un_libelle_simple() {
        let tx = parse_summary("F01 10671kHz RTTY", t0()).unwrap();
        assert_eq!(tx.station, "F01");
        assert_eq!(tx.frequency_khz, Some(10671));
        assert_eq!(tx.mode.as_deref(), Some("RTTY"));
        assert_eq!(tx.target, None);
    }

    #[test]
    fn lit_un_espace_avant_khz() {
        let tx = parse_summary("E11 6804 kHz USB", t0()).unwrap();
        assert_eq!(tx.station, "E11");
        assert_eq!(tx.frequency_khz, Some(6804));
        assert_eq!(tx.mode.as_deref(), Some("USB"));
    }

    #[test]
    fn lit_la_zone_visee() {
        let tx = parse_summary("M12 6917kHz CW [Target: Eastern Europe]", t0()).unwrap();
        assert_eq!(tx.station, "M12");
        assert_eq!(tx.mode.as_deref(), Some("CW"));
        assert_eq!(tx.target.as_deref(), Some("Eastern Europe"));
    }

    #[test]
    fn lit_une_variante_de_station() {
        let tx = parse_summary("F03l 16005kHz USB", t0()).unwrap();
        assert_eq!(tx.station, "F03l");
        assert_eq!(tx.frequency_khz, Some(16005));
    }

    #[test]
    fn accepte_une_emission_sans_frequence() {
        let tx = parse_summary("S28 Search", t0()).unwrap();
        assert_eq!(tx.station, "S28");
        assert_eq!(tx.frequency_khz, None);
        assert_eq!(tx.mode, None);
    }

    #[test]
    fn refuse_un_libelle_vide() {
        assert!(parse_summary("", t0()).is_none());
    }

    #[test]
    fn horodatage_au_format_de_l_agenda() {
        assert_eq!(iso(t0()), "2026-11-11T10:25:00.000Z");
    }

    #[test]
    fn la_tranche_reste_sous_le_plafond() {
        // Garde-fou : une tranche de 24 h doit rester bien en dessous des 250
        // événements du plafond, sinon le relevé serait tronqué.
        assert!(CHUNK <= TimeDelta::hours(24));
        assert_eq!(MAX_RESULTS, 250);
    }
}
