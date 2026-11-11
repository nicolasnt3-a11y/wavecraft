//! Catalogue des stations : fiches, préréglages et planning hebdomadaire.
//!
//! Le catalogue livré avec l'application est produit par `tools/update_catalog.py`
//! à partir de deux sources Priyom.org, et de rien d'autre :
//!
//! - l'agenda, pour les stations à horaires fixes (E11, M12, F01…) ;
//! - les fiches du site, pour les stations en émission continue (S28 « Le
//!   Bourdon », S30 « Le Pip »…), qui n'apparaissent à aucun agenda.
//!
//! Il se met à jour tout seul : le planning est relu toutes les six heures
//! depuis l'agenda, et une version plus récente du catalogue est cherchée une
//! fois par jour sur le dépôt.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, NaiveTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use crate::priyom::Transmission;

/// Catalogue livré avec l'application.
const EMBEDDED: &str = include_str!("../assets/catalog.json");

/// Largeur de filtre habituelle pour un mode, en Hz.
pub fn default_bandwidth(mode: &str) -> u32 {
    match mode.to_ascii_uppercase().as_str() {
        "CW" => 500,
        "RTTY" | "FSK" | "PSK" => 1_000,
        "AM" => 6_000,
        "FM" | "NFM" => 12_500,
        _ => 2_700, // USB, LSB et modes numériques à bande latérale
    }
}

/// État d'activité d'une station, tel que publié par Priyom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// En activité.
    Active,
    /// Silencieuse, mais réapparaît lors des essais de l'opérateur.
    Hibernated,
    /// Ne émet plus.
    Inactive,
    #[serde(other)]
    Unknown,
}

impl Status {
}

/// Préréglage d'écoute : tout ce qu'il faut pour entendre une station.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub frequency_khz: u32,
    pub mode: String,
    #[serde(default)]
    pub bandwidth_hz: u32,
    /// Le squelch reste désactivé : une station de nombres passe le plus clair
    /// de son temps à émettre une porteuse silencieuse, qu'un squelch couperait.
    #[serde(default)]
    pub squelch: bool,
    /// Nombre de fois où le couple fréquence/mode a été relevé à l'agenda.
    /// Zéro pour une fréquence lue sur la fiche d'une station continue.
    #[serde(default)]
    pub seen: u32,
}

impl Preset {
    pub fn bandwidth(&self) -> u32 {
        if self.bandwidth_hz > 0 {
            self.bandwidth_hz
        } else {
            default_bandwidth(&self.mode)
        }
    }
}

/// Créneau hebdomadaire relevé à l'agenda.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    /// Jour de la semaine, 0 = lundi (comme `weekday()` en Python).
    pub weekday: u8,
    /// Heure UTC, au format `HH:MM`.
    pub utc: String,
    pub frequency_khz: u32,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub count: u32,
}

impl Slot {
    fn time(&self) -> Option<NaiveTime> {
        NaiveTime::parse_from_str(&self.utc, "%H:%M").ok()
    }

    /// Prochaine occurrence du créneau à partir de `from`.
    pub fn next_after(&self, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let time = self.time()?;
        // `weekday` compte le lundi comme 0, comme `num_days_from_monday`.
        let target = u32::from(self.weekday);
        let today = from.weekday().num_days_from_monday();
        let mut delta = (target + 7 - today) % 7;
        let candidate = from
            .date_naive()
            .and_time(time)
            .and_utc()
            .checked_add_signed(TimeDelta::days(i64::from(delta)))?;
        if candidate <= from {
            delta += 7;
            return from
                .date_naive()
                .and_time(time)
                .and_utc()
                .checked_add_signed(TimeDelta::days(i64::from(delta)));
        }
        Some(candidate)
    }
}

/// Fiche d'une station.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Station {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub family: String,
    #[serde(default = "status_active")]
    pub status: Status,
    #[serde(default)]
    pub voice: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub url: String,
    /// Variantes rattachées : `F03j`, `F03l` pour `F03`.
    #[serde(default)]
    pub variants: Vec<String>,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub counterparts: Vec<String>,
    #[serde(default)]
    pub presets: Vec<Preset>,
    #[serde(default)]
    pub slots: Vec<Slot>,
    /// Émission continue : pas d'horaire, écoutable à tout moment.
    #[serde(default)]
    pub continuous: bool,
    /// Station aperçue à l'agenda sans fiche au catalogue : affichée avec un
    /// badge « Nouvelle » jusqu'à la prochaine publication du catalogue.
    #[serde(default)]
    pub discovered: bool,
    #[serde(default)]
    pub first_seen: String,
    #[serde(default)]
    pub last_seen: String,
    #[serde(default)]
    pub notes: String,
}

fn status_active() -> Status {
    Status::Active
}

impl Station {
    /// Préréglage conseillé : le couple fréquence/mode le plus souvent relevé.
    pub fn preset(&self) -> Option<&Preset> {
        self.presets.first()
    }

    /// Nom affiché : « E11 — Oblique », ou simplement l'identifiant.
    pub fn display_name(&self) -> String {
        let name = self.name.trim();
        if name.is_empty() || name == self.id {
            self.id.clone()
        } else {
            // Priyom accumule parfois les alias dans le nom ; on garde le premier.
            let first = name.split(',').next().unwrap_or(name).trim();
            format!("{} — {first}", self.id)
        }
    }

    /// Nom seul, en capitales, sans l'identifiant : « THE BUZZER ».
    ///
    /// Vide si la fiche ne donne pas d'autre nom que l'identifiant.
    pub fn name_only(&self) -> String {
        let name = self.name.trim();
        if name.is_empty() || name == self.id {
            return String::new();
        }
        let first = name.split(',').next().unwrap_or(name).trim();
        first.to_uppercase()
    }

    pub fn is_listenable(&self) -> bool {
        self.status == Status::Active && !self.presets.is_empty()
    }
}

/// Une émission à venir, construite depuis le planning.
#[derive(Debug, Clone, PartialEq)]
pub struct Upcoming {
    pub station: String,
    pub at: DateTime<Utc>,
    pub frequency_khz: u32,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub window_days: i64,
    #[serde(default)]
    pub events_seen: usize,
    pub stations: Vec<Station>,
}

impl Catalog {
    /// Catalogue livré avec l'application.
    pub fn embedded() -> Result<Self> {
        serde_json::from_str(EMBEDDED).context("catalogue intégré illisible")
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("lecture du catalogue ({})", path.display()))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("catalogue illisible ({})", path.display()))
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        crate::paths::ensure_parent(path)?;
        std::fs::write(path, serde_json::to_string(self)?)
            .with_context(|| format!("écriture du catalogue ({})", path.display()))
    }

    /// Catalogue utilisé au démarrage : celui du disque s'il est plus récent
    /// que celui livré, sinon celui livré.
    pub fn load_best(path: &Path) -> Self {
        let embedded = Self::embedded().unwrap_or_else(|_| Self {
            version: 0,
            generated_at: String::new(),
            source: String::new(),
            window_days: 0,
            events_seen: 0,
            stations: Vec::new(),
        });
        match Self::load_from(path) {
            Ok(disk) if disk.version > embedded.version => disk,
            _ => embedded,
        }
    }

    pub fn station(&self, id: &str) -> Option<&Station> {
        self.stations.iter().find(|s| s.id == id).or_else(|| {
            // Une variante renvoie à sa station mère : `F03l` → `F03`.
            self.stations
                .iter()
                .find(|s| s.variants.iter().any(|v| v == id))
        })
    }

    /// Stations écoutables tout de suite, les continues d'abord.
    pub fn listenable(&self) -> Vec<&Station> {
        let mut out: Vec<&Station> = self.stations.iter().filter(|s| s.is_listenable()).collect();
        out.sort_by_key(|s| (!s.continuous, s.id.clone()));
        out
    }

    /// Prochaines émissions annoncées, triées par heure.
    pub fn upcoming(&self, from: DateTime<Utc>, horizon: TimeDelta) -> Vec<Upcoming> {
        let limit = from + horizon;
        let mut out = Vec::new();
        for station in &self.stations {
            if station.status == Status::Inactive {
                continue;
            }
            for slot in &station.slots {
                if let Some(at) = slot.next_after(from)
                    && at <= limit
                {
                    out.push(Upcoming {
                        station: station.id.clone(),
                        at,
                        frequency_khz: slot.frequency_khz,
                        mode: slot.mode.clone(),
                    });
                }
            }
        }
        out.sort_by_key(|u| (u.at, u.station.clone(), u.frequency_khz));
        out
    }

    /// Intègre un relevé d'agenda : les stations inconnues sont ajoutées,
    /// déjà préréglées, et signalées comme nouvelles.
    ///
    /// Renvoie les identifiants ajoutés.
    pub fn merge_live(&mut self, transmissions: &[Transmission]) -> Vec<String> {
        let mut added = Vec::new();
        let mut presets_by_station: HashMap<String, Vec<&Transmission>> = HashMap::new();

        for tx in transmissions {
            let parent = parent_station(&tx.station);
            if self.station(&parent).is_some() || self.station(&tx.station).is_some() {
                continue;
            }
            presets_by_station.entry(parent).or_default().push(tx);
        }

        for (id, found) in presets_by_station {
            let mut presets: Vec<Preset> = Vec::new();
            for tx in &found {
                let Some(khz) = tx.frequency_khz else { continue };
                let mode = tx.mode.clone().unwrap_or_else(|| "USB".to_string());
                if presets
                    .iter()
                    .any(|p| p.frequency_khz == khz && p.mode == mode)
                {
                    continue;
                }
                presets.push(Preset {
                    frequency_khz: khz,
                    bandwidth_hz: default_bandwidth(&mode),
                    mode,
                    squelch: false,
                    seen: 1,
                });
            }
            let variants: Vec<String> = {
                let mut v: Vec<String> = found
                    .iter()
                    .map(|t| t.station.clone())
                    .filter(|s| *s != id)
                    .collect();
                v.sort();
                v.dedup();
                v
            };
            added.push(id.clone());
            self.stations.push(Station {
                id: id.clone(),
                name: id.clone(),
                family: String::new(),
                status: Status::Active,
                voice: String::new(),
                location: String::new(),
                summary: String::new(),
                url: String::new(),
                variants,
                targets: Vec::new(),
                counterparts: Vec::new(),
                presets,
                slots: Vec::new(),
                continuous: false,
                discovered: true,
                first_seen: Utc::now().format("%Y-%m-%d").to_string(),
                last_seen: Utc::now().format("%Y-%m-%d").to_string(),
                notes: String::new(),
            });
        }

        added.sort();
        self.stations.sort_by(|a, b| a.id.cmp(&b.id));
        added
    }

    /// Âge du catalogue, d'après sa date de génération.
    pub fn age(&self, now: DateTime<Utc>) -> Option<Duration> {
        let generated = DateTime::parse_from_rfc3339(&self.generated_at).ok()?;
        (now - generated.with_timezone(&Utc)).to_std().ok()
    }

    /// Télécharge le catalogue publié et le retient s'il est valide, plus
    /// récent et au moins aussi complet que celui en place.
    pub fn fetch_update(&self, url: &str) -> Result<Option<Catalog>> {
        let body = ureq::get(url)
            .call()
            .with_context(|| format!("téléchargement du catalogue ({url})"))?
            .body_mut()
            .read_to_string()
            .context("lecture du catalogue téléchargé")?;

        let candidate: Catalog =
            serde_json::from_str(&body).context("catalogue téléchargé illisible")?;

        if candidate.version <= self.version {
            return Ok(None);
        }
        if candidate.stations.len() * 2 < self.stations.len() {
            bail!(
                "catalogue téléchargé trop pauvre ({} stations contre {}) : ignoré",
                candidate.stations.len(),
                self.stations.len()
            );
        }
        Ok(Some(candidate))
    }
}

/// Rattache une variante à sa station mère : `F03j` → `F03`, `P03k` → `P03`.
pub fn parent_station(id: &str) -> String {
    let bytes = id.as_bytes();
    if bytes.len() >= 3
        && bytes[bytes.len() - 1].is_ascii_lowercase()
        && bytes[bytes.len() - 2].is_ascii_digit()
    {
        return id[..id.len() - 1].to_string();
    }
    id.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn catalogue() -> Catalog {
        Catalog::embedded().expect("le catalogue livré doit être lisible")
    }

    #[test]
    fn le_catalogue_livre_est_lisible_et_fourni() {
        let c = catalogue();
        assert!(c.stations.len() > 100, "{} stations", c.stations.len());
        assert!(c.events_seen > 500, "relevé d'agenda trop maigre");
    }

    #[test]
    fn les_stations_continues_sont_prereglees() {
        let c = catalogue();
        // S28 « Le Bourdon » émet en continu sur 4 625 kHz en USB : c'est la
        // station la plus simple à entendre, elle doit être écoutable d'un clic.
        let buzzer = c.station("S28").expect("S28 au catalogue");
        let preset = buzzer.preset().expect("S28 préréglée");
        assert_eq!(preset.frequency_khz, 4625);
        assert_eq!(preset.mode, "USB");
        assert_eq!(preset.frequency_khz, 4_625);
        assert!(!preset.squelch, "le squelch couperait la porteuse");
        assert!(buzzer.continuous);
    }

    #[test]
    fn le_morse_recoit_un_filtre_etroit() {
        assert_eq!(default_bandwidth("CW"), 500);
        assert_eq!(default_bandwidth("USB"), 2700);
        assert_eq!(default_bandwidth("AM"), 6000);
        let c = catalogue();
        let m12 = c.station("M12").expect("M12 au catalogue");
        let preset = m12.preset().unwrap();
        assert_eq!(preset.mode, "CW");
        assert_eq!(preset.bandwidth(), 500);
    }

    #[test]
    fn une_variante_renvoie_a_sa_station_mere() {
        assert_eq!(parent_station("F03l"), "F03");
        assert_eq!(parent_station("P03k"), "P03");
        assert_eq!(parent_station("E11"), "E11");
        assert_eq!(parent_station("XPA2"), "XPA2");
        assert_eq!(parent_station("S28"), "S28");
        let c = catalogue();
        assert_eq!(c.station("F03l").map(|s| s.id.as_str()), Some("F03"));
    }

    #[test]
    fn le_prochain_creneau_tombe_apres_l_instant_donne() {
        // Mercredi 11 novembre 2026, 10 h 00 UTC.
        let now = Utc.with_ymd_and_hms(2026, 11, 11, 10, 0, 0).unwrap();
        let slot = Slot {
            weekday: 2, // mercredi
            utc: "10:30".into(),
            frequency_khz: 6804,
            mode: Some("USB".into()),
            count: 2,
        };
        let next = slot.next_after(now).unwrap();
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 11, 11, 10, 30, 0).unwrap());

        // Un créneau déjà passé dans la journée bascule à la semaine suivante.
        let passe = Slot {
            utc: "09:00".into(),
            ..slot
        };
        assert_eq!(
            passe.next_after(now).unwrap(),
            Utc.with_ymd_and_hms(2026, 11, 18, 9, 0, 0).unwrap()
        );
    }

    #[test]
    fn le_planning_annonce_des_emissions_a_venir() {
        let c = catalogue();
        let now = Utc::now();
        let next = c.upcoming(now, TimeDelta::hours(24));
        assert!(!next.is_empty(), "aucune émission dans les 24 h");
        assert!(next.windows(2).all(|w| w[0].at <= w[1].at), "mal trié");
        assert!(next.iter().all(|u| u.at > now && u.at <= now + TimeDelta::hours(24)));
    }

    #[test]
    fn une_station_inconnue_est_ajoutee_et_signalee() {
        let mut c = catalogue();
        let avant = c.stations.len();
        let tx = Transmission {
            station: "Z99a".into(),
            frequency_khz: Some(7_000),
            mode: Some("USB".into()),
            target: None,
            start: Utc::now(),
            summary: "Z99a 7000kHz USB".into(),
        };
        let added = c.merge_live(&[tx]);
        assert_eq!(added, vec!["Z99".to_string()]);
        assert_eq!(c.stations.len(), avant + 1);
        let station = c.station("Z99").unwrap();
        assert!(station.discovered, "doit porter le badge « Nouvelle »");
        assert_eq!(station.preset().unwrap().frequency_khz, 7_000);
        assert_eq!(station.variants, vec!["Z99a".to_string()]);
    }

    #[test]
    fn une_station_connue_n_est_pas_dupliquee() {
        let mut c = catalogue();
        let avant = c.stations.len();
        let tx = Transmission {
            station: "E11".into(),
            frequency_khz: Some(6_804),
            mode: Some("USB".into()),
            target: None,
            start: Utc::now(),
            summary: "E11 6804kHz USB".into(),
        };
        assert!(c.merge_live(&[tx]).is_empty());
        assert_eq!(c.stations.len(), avant);
    }

    #[test]
    fn le_catalogue_livre_a_une_date_de_generation() {
        let c = catalogue();
        assert!(
            c.age(Utc::now()).is_some(),
            "date de génération illisible : {}",
            c.generated_at
        );
    }
}
