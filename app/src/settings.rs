//! Réglages de Wavecraft, conservés entre deux sessions.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths;

/// Adresse par défaut du catalogue publié, vérifiée une fois par jour.
pub const CATALOG_URL: &str =
    "https://raw.githubusercontent.com/nicolasnt3-a11y/wavecraft/main/app/assets/catalog.json";

/// Délai par défaut entre deux actualisations du planning Priyom.
pub const REFRESH_HOURS: u32 = 6;

/// Profondeur du planning téléchargé, en jours.
pub const PLANNING_DAYS: i64 = 7;

/// D'où vient le signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceChoice {
    /// Clé branchée sur cette machine.
    Rtl,
    /// Clé d'une autre machine, servie par `rtl_tcp`.
    Remote,
    /// Signal de démonstration, sans matériel.
    Demo,
}

impl SourceChoice {
    pub fn label(self) -> &'static str {
        match self {
            SourceChoice::Rtl => "clé locale",
            SourceChoice::Remote => "clé distante (rtl_tcp)",
            SourceChoice::Demo => "démonstration",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Index de la clé RTL-SDR à ouvrir.
    pub device_index: u32,
    /// Correction d'oscillateur, en parties par million.
    pub ppm: i32,
    /// Gain en dixièmes de dB ; `None` pour le gain automatique.
    pub gain_tenths_db: Option<i32>,
    /// Volume de sortie, de 0,0 à 1,0.
    pub volume: f32,
    /// Dernière station écoutée, pour rouvrir au même endroit.
    pub last_station: Option<String>,
    /// Actualisation automatique du planning Priyom.
    pub auto_refresh: bool,
    /// Délai entre deux actualisations, en heures.
    pub refresh_hours: u32,
    /// Vérification quotidienne d'un catalogue plus récent.
    pub catalog_updates: bool,
    /// Adresse du catalogue publié.
    pub catalog_url: String,
    /// Dossier des enregistrements ; par défaut celui de [`paths::recordings_dir`].
    pub recordings_dir: Option<PathBuf>,
    /// Décodage Morse activé d'office sur les stations en CW.
    pub morse_auto: bool,
    /// Traduire les chiffres abrégés des opérateurs en chiffres pleins.
    pub cut_numbers: bool,
    /// Stations mises en signet, dans l'ordre choisi.
    pub bookmarks: Vec<String>,
    /// D'où vient le signal.
    pub source: SourceChoice,
    /// Machine servant la clé distante.
    pub remote_host: String,
    pub remote_port: u16,
    /// Mode Expert : gain, correction PPM, bias-tee et bandes sont accessibles.
    pub expert: bool,
    /// L'assistant de premier lancement n'a pas encore été mené à son terme.
    pub first_run: bool,
    /// Port du serveur web lancé depuis la fenêtre.
    pub server_port: u16,
    /// Mot de passe du serveur web ; vide, l'accès est libre.
    pub server_password: String,
    /// Position de l'auditeur, utilisée pour indiquer comment orienter l'antenne.
    pub position: Option<crate::antenna::Position>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device_index: 0,
            ppm: 0,
            gain_tenths_db: None,
            volume: 0.7,
            last_station: None,
            auto_refresh: true,
            refresh_hours: REFRESH_HOURS,
            catalog_updates: true,
            catalog_url: CATALOG_URL.to_string(),
            recordings_dir: None,
            morse_auto: true,
            cut_numbers: true,
            bookmarks: Vec::new(),
            source: SourceChoice::Rtl,
            remote_host: "127.0.0.1".to_string(),
            remote_port: 1234,
            expert: false,
            first_run: true,
            server_port: 8073,
            server_password: String::new(),
            position: None,
        }
    }
}

impl Settings {
    /// Fichier de réglages par défaut.
    pub fn path() -> PathBuf {
        paths::data_dir().join("settings.json")
    }

    /// Lit les réglages. Un fichier absent donne les réglages par défaut ; un
    /// fichier illisible est signalé plutôt qu'écrasé en silence.
    pub fn load() -> Result<Self> {
        Self::load_from(&Self::path())
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("lecture des réglages ({})", path.display()))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("réglages illisibles ({})", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::path())
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        paths::ensure_parent(path)?;
        let raw = serde_json::to_string_pretty(self)?;
        std::fs::write(path, raw)
            .with_context(|| format!("écriture des réglages ({})", path.display()))
    }

    /// Dossier d'enregistrement effectif.
    pub fn recordings(&self) -> PathBuf {
        self.recordings_dir
            .clone()
            .unwrap_or_else(paths::recordings_dir)
    }

    /// Ajoute ou retire un signet.
    pub fn toggle_bookmark(&mut self, station: &str) -> bool {
        if let Some(pos) = self.bookmarks.iter().position(|s| s == station) {
            self.bookmarks.remove(pos);
            false
        } else {
            self.bookmarks.push(station.to_string());
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("wavecraft-test-{name}.json"))
    }

    #[test]
    fn les_valeurs_par_defaut_sont_raisonnables() {
        let s = Settings::default();
        assert!(s.auto_refresh);
        assert!(s.catalog_updates);
        assert_eq!(s.refresh_hours, 6);
        assert!((0.0..=1.0).contains(&s.volume));
        assert!(s.gain_tenths_db.is_none(), "gain automatique par défaut");
    }

    #[test]
    fn un_fichier_absent_donne_les_valeurs_par_defaut() {
        let path = tmp("absent");
        let _ = std::fs::remove_file(&path);
        assert_eq!(Settings::load_from(&path).unwrap(), Settings::default());
    }

    #[test]
    fn aller_retour_sur_disque() {
        let path = tmp("aller-retour");
        let mut s = Settings::default();
        s.ppm = 12;
        s.last_station = Some("E11".into());
        s.bookmarks.push("S28".into());
        s.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path).unwrap(), s);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn un_reglage_inconnu_ne_casse_pas_la_lecture() {
        // Un fichier écrit par une version plus récente doit rester lisible.
        let path = tmp("champ-inconnu");
        std::fs::write(&path, r#"{"ppm": 3, "reglage_futur": true}"#).unwrap();
        let s = Settings::load_from(&path).unwrap();
        assert_eq!(s.ppm, 3);
        assert_eq!(s.refresh_hours, REFRESH_HOURS);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn les_signets_s_ajoutent_et_se_retirent() {
        let mut s = Settings::default();
        assert!(s.toggle_bookmark("M12"));
        assert_eq!(s.bookmarks, vec!["M12".to_string()]);
        assert!(!s.toggle_bookmark("M12"));
        assert!(s.bookmarks.is_empty());
    }
}
