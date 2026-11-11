//! État commun à la fenêtre et au serveur web.
//!
//! Tout ce qui n'est pas de l'affichage vit ici : le catalogue, le planning, le
//! journal, le moteur et la session en cours. La fenêtre et le navigateur n'en
//! sont que deux vues — c'est ce qui garantit qu'ils se comportent pareil.
//!
//! L'écoute se fait en un seul geste : [`AppState::listen`] applique le
//! préréglage, démarre la clé si besoin, active le Morse pour les stations en
//! CW et ouvre une entrée au journal.

use std::sync::mpsc::{Receiver, TryRecvError, channel};

use anyhow::{Context, Result};
use chrono::{DateTime, TimeDelta, Utc};

use crate::catalog::{Catalog, Preset, Upcoming};
use crate::dsp::Mode;
use crate::engine::{Command, Engine, Status};
use crate::link::{Link, LogRow, Request, Snapshot};
use crate::logbook::{Entry, Logbook};
use crate::priyom::{Priyom, Transmission};
use crate::sdr::{Gain, Source, demo::Demo, rtlsdr::RtlSdr, rtltcp::RtlTcp};
use crate::settings::{self, Settings, SourceChoice};

/// Horizon du planning affiché.
const UPCOMING_HOURS: i64 = 24;

/// Résultat d'une actualisation lancée en arrière-plan.
enum Refreshed {
    Planning(Result<Vec<Transmission>>),
    Catalog(Result<Option<Catalog>>),
}

/// Une station telle qu'elle apparaît dans la liste du catalogue.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: String,
    /// Nom seul, sans l'identifiant : « THE BUZZER ».
    pub name: String,
    pub frequency_khz: Option<u32>,
    pub mode: String,
    /// Prochaine émission annoncée, s'il y en a une.
    pub next: Option<DateTime<Utc>>,
    pub continuous: bool,
    pub bookmarked: bool,
}

/// Ce que l'utilisateur écoute en ce moment.
pub struct Session {
    pub station: String,
    pub entry: Entry,
}

pub struct AppState {
    pub settings: Settings,
    pub catalog: Catalog,
    pub logbook: Logbook,
    engine: Option<Engine>,
    pub session: Option<Session>,
    /// Émissions annoncées dans les prochaines heures.
    pub upcoming: Vec<Upcoming>,
    /// Relevé brut de l'agenda, pour le fil en direct.
    pub live: Vec<Transmission>,
    pub last_refresh: Option<DateTime<Utc>>,
    /// Stations repérées à l'agenda et absentes du catalogue.
    pub discovered: Vec<String>,
    pub message: String,
    pub demo: bool,
    inbox: Option<Receiver<Refreshed>>,
    refreshing: bool,
    /// File audio secondaire, pour le serveur web.
    tap: Option<crate::audio::AudioSink>,
    /// Incrémenté à chaque démarrage du moteur : la sortie audio sait ainsi
    /// qu'elle écoute une file périmée et doit être réouverte.
    generation: u64,
}

impl AppState {
    /// Charge réglages, catalogue et journal.
    pub fn new(demo: bool) -> Result<Self> {
        let settings = Settings::load().unwrap_or_default();
        let catalog = Catalog::load_best(&crate::paths::data_dir().join("catalog.json"));
        let logbook = Logbook::open().context("ouverture du journal")?;

        let mut state = Self {
            settings,
            catalog,
            logbook,
            engine: None,
            session: None,
            upcoming: Vec::new(),
            live: Vec::new(),
            last_refresh: None,
            discovered: Vec::new(),
            message: String::new(),
            demo,
            inbox: None,
            refreshing: false,
            tap: None,
            generation: 0,
        };
        state.recompute_upcoming();
        Ok(state)
    }

    /// Ouvre la source réglée : clé locale, clé distante ou démonstration.
    ///
    /// L'option `--demo` de la ligne de commande l'emporte sur le réglage.
    fn open_source(&self) -> Result<Box<dyn Source>> {
        if self.demo || self.settings.source == SourceChoice::Demo {
            return Ok(Box::new(Demo::new()));
        }
        if self.settings.source == SourceChoice::Remote {
            let adresse = format!("{}:{}", self.settings.remote_host, self.settings.remote_port);
            return Ok(Box::new(RtlTcp::connect(&adresse)?));
        }
        let mut sdr = RtlSdr::open(self.settings.device_index)?;
        sdr.set_ppm(self.settings.ppm).ok();
        sdr.set_gain(match self.settings.gain_tenths_db {
            Some(tenths) => Gain::Manual(tenths),
            None => Gain::Auto,
        })
        .ok();
        Ok(Box::new(sdr))
    }

    /// Démarre le moteur s'il ne tourne pas déjà.
    pub fn ensure_engine(&mut self) -> Result<()> {
        let running = self
            .engine
            .as_ref()
            .map(|e| e.status().running)
            .unwrap_or(false);
        if running {
            return Ok(());
        }
        let source = self.open_source()?;
        let engine = Engine::start(source)?;
        engine.audio().set_volume(self.settings.volume);
        self.tap = Some(engine.tap());
        self.engine = Some(engine);
        self.generation += 1;
        Ok(())
    }

    /// Numéro de la file audio en cours.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Écoute une station, d'un seul geste.
    pub fn listen(&mut self, station_id: &str) -> Result<()> {
        let (frequency_khz, mode, bandwidth, display) = {
            let station = self
                .catalog
                .station(station_id)
                .with_context(|| format!("station inconnue : {station_id}"))?;
            let preset: Preset = station
                .preset()
                .cloned()
                .with_context(|| format!("aucun préréglage connu pour {station_id}"))?;
            let mode = Mode::parse(&preset.mode).unwrap_or(Mode::Usb);
            (
                preset.frequency_khz,
                mode,
                preset.bandwidth(),
                station.display_name(),
            )
        };
        self.begin(station_id, &display, frequency_khz, mode, bandwidth)
    }

    /// Accorde, clôt la session précédente et ouvre une entrée au journal.
    fn begin(
        &mut self,
        station_id: &str,
        display: &str,
        frequency_khz: u32,
        mode: Mode,
        bandwidth_hz: u32,
    ) -> Result<()> {
        self.ensure_engine()?;
        self.close_session();

        let engine = self.engine.as_ref().expect("moteur démarré");
        engine.tune(frequency_khz * 1_000, mode, bandwidth_hz as f32);
        // Le Morse ne s'active de lui-même que sur les stations qui en émettent.
        engine.send(Command::SetMorse(
            mode == Mode::Cw && self.settings.morse_auto,
        ));

        let mut entry = Entry::new(station_id, frequency_khz, mode.label());
        entry.bandwidth_hz = bandwidth_hz;
        self.logbook.append(entry.clone())?;

        self.session = Some(Session {
            station: station_id.to_string(),
            entry,
        });
        self.settings.last_station = Some(station_id.to_string());
        let _ = self.settings.save();
        self.message = format!("{display} — {frequency_khz} kHz {}", mode.label());
        Ok(())
    }

    /// Accord libre, hors catalogue.
    pub fn tune_manual(&mut self, frequency_hz: u32, mode: Mode, bandwidth_hz: f32) -> Result<()> {
        self.ensure_engine()?;
        self.close_session();
        let engine = self.engine.as_ref().expect("moteur démarré");
        engine.tune(frequency_hz, mode, bandwidth_hz);
        engine.send(Command::SetMorse(mode == Mode::Cw && self.settings.morse_auto));
        self.message = format!("{:.3} kHz {}", frequency_hz as f32 / 1_000.0, mode.label());
        Ok(())
    }

    /// Clôt l'entrée de journal en cours, en y reportant ce qui a été entendu.
    pub fn close_session(&mut self) {
        let Some(mut session) = self.session.take() else {
            return;
        };
        if let Some(engine) = &self.engine {
            let status = engine.status();
            session.entry.level_dbfs = status.level_dbfs;
            if !status.morse_text.trim().is_empty() {
                session.entry.decoded = status.morse_text;
            }
            if let Some(path) = status.recording {
                session.entry.recording = Some(path);
            }
        }
        session.entry.close();
        if let Err(err) = self.logbook.update(session.entry) {
            self.message = format!("journal : {err:#}");
        }
    }

    pub fn stop(&mut self) {
        self.close_session();
        self.tap = None;
        if let Some(engine) = self.engine.take() {
            engine.stop();
        }
        self.message = "arrêté".into();
    }

    pub fn status(&self) -> Option<Status> {
        self.engine.as_ref().map(|e| e.status())
    }

    pub fn audio(&self) -> Option<crate::audio::AudioSink> {
        self.engine.as_ref().map(|e| e.audio())
    }

    pub fn send(&self, command: Command) {
        if let Some(engine) = &self.engine {
            engine.send(command);
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.settings.volume = volume.clamp(0.0, 1.0);
        if let Some(engine) = &self.engine {
            engine.audio().set_volume(self.settings.volume);
        }
    }

    /// Démarre ou arrête l'enregistrement.
    pub fn toggle_recording(&mut self) {
        let recording = self
            .status()
            .map(|s| s.recording.is_some())
            .unwrap_or(false);
        if recording {
            self.send(Command::StopRecording);
            self.message = "enregistrement terminé".into();
        } else {
            let station = self
                .session
                .as_ref()
                .map(|s| s.station.clone())
                .unwrap_or_else(|| "libre".to_string());
            self.send(Command::StartRecording {
                dir: self.settings.recordings(),
                station,
            });
            self.message = "enregistrement en cours".into();
        }
    }

    /// Faut-il relire le planning ?
    ///
    /// Oui s'il n'a jamais été lu, s'il date de plus que le délai réglé, ou
    /// s'il n'annonce plus rien dans les 24 heures qui viennent — ce dernier
    /// cas évite de rester sur un planning périmé après un long arrêt.
    pub fn needs_refresh(&self, now: DateTime<Utc>) -> bool {
        if !self.settings.auto_refresh {
            return false;
        }
        match self.last_refresh {
            None => true,
            Some(last) => {
                now - last > TimeDelta::hours(i64::from(self.settings.refresh_hours))
                    || self.upcoming.is_empty()
            }
        }
    }

    /// Lance une actualisation en arrière-plan. Sans effet si une est en cours.
    pub fn refresh(&mut self) {
        if self.refreshing {
            return;
        }
        self.refreshing = true;
        self.message = "actualisation du planning…".into();

        let (tx, rx) = channel();
        self.inbox = Some(rx);

        let planning_tx = tx.clone();
        std::thread::spawn(move || {
            let now = Utc::now();
            let client = Priyom::new();
            let result = client.fetch_range(now, now + TimeDelta::days(settings::PLANNING_DAYS));
            let _ = planning_tx.send(Refreshed::Planning(result));
        });

        if self.settings.catalog_updates {
            let url = self.settings.catalog_url.clone();
            let current = self.catalog.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Refreshed::Catalog(current.fetch_update(&url)));
            });
        }
    }

    /// À appeler régulièrement : récupère les résultats des actualisations.
    pub fn tick(&mut self) {
        // Les résultats sont d'abord ramassés, puis traités : la boucle de
        // réception emprunte la boîte aux lettres, et le traitement touche à
        // tout le reste de l'état.
        let mut arrivals = Vec::new();
        let mut closed = false;
        if let Some(inbox) = &self.inbox {
            loop {
                match inbox.try_recv() {
                    Ok(message) => arrivals.push(message),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        closed = true;
                        break;
                    }
                }
            }
        }
        if closed {
            self.inbox = None;
            self.refreshing = false;
        }

        for message in arrivals {
            match message {
                Refreshed::Planning(Ok(transmissions)) => {
                    self.discovered = self.catalog.merge_live(&transmissions);
                    self.live = transmissions;
                    self.last_refresh = Some(Utc::now());
                    self.refreshing = false;
                    self.recompute_upcoming();
                    self.message = format!(
                        "planning à jour : {} émissions annoncées{}",
                        self.live.len(),
                        if self.discovered.is_empty() {
                            String::new()
                        } else {
                            format!(", {} nouvelle(s) station(s)", self.discovered.len())
                        }
                    );
                }
                Refreshed::Planning(Err(err)) => {
                    self.refreshing = false;
                    // Hors connexion, le dernier planning reste utilisable.
                    self.message =
                        format!("agenda injoignable ({err:#}) — le dernier planning reste affiché");
                }
                Refreshed::Catalog(Ok(Some(catalog))) => {
                    let path = crate::paths::data_dir().join("catalog.json");
                    let version = catalog.version;
                    let count = catalog.stations.len();
                    self.catalog = catalog;
                    let _ = self.catalog.save_to(&path);
                    self.recompute_upcoming();
                    self.message =
                        format!("catalogue mis à jour : version {version}, {count} stations");
                }
                Refreshed::Catalog(Ok(None)) => {}
                Refreshed::Catalog(Err(err)) => {
                    self.message = format!("catalogue : {err:#}");
                }
            }
        }
    }

    pub fn is_refreshing(&self) -> bool {
        self.refreshing
    }

    /// Âge du planning, en texte lisible.
    pub fn planning_age(&self) -> String {
        match self.last_refresh {
            None => "jamais actualisé".to_string(),
            Some(last) => {
                let minutes = (Utc::now() - last).num_minutes().max(0);
                match minutes {
                    0 => "à l'instant".to_string(),
                    1..=59 => format!("il y a {minutes} min"),
                    _ => format!("il y a {} h", minutes / 60),
                }
            }
        }
    }

    fn recompute_upcoming(&mut self) {
        self.upcoming = self
            .catalog
            .upcoming(Utc::now(), TimeDelta::hours(UPCOMING_HOURS));
    }

    /// Une ligne du catalogue, telle qu'elle s'affiche à l'écran.
    ///
    /// Elle réunit ce qui vient de la fiche de station et ce qui vient du
    /// planning : l'interface n'a plus qu'à la mettre en forme.
    pub fn rows(&self, now: DateTime<Utc>) -> Vec<Row> {
        let mut rows: Vec<Row> = self
            .catalog
            .stations
            .iter()
            .filter(|s| s.is_listenable())
            .map(|station| {
                let next = station
                    .slots
                    .iter()
                    .filter_map(|slot| slot.next_after(now))
                    .min();
                let preset = station.preset();
                Row {
                    id: station.id.clone(),
                    name: station.name_only(),
                    frequency_khz: preset.map(|p| p.frequency_khz),
                    mode: preset.map(|p| p.mode.clone()).unwrap_or_default(),
                    next,
                    continuous: station.continuous,
                    bookmarked: self.settings.bookmarks.iter().any(|b| b == &station.id),
                }
            })
            .collect();

        // L'ordre de la maquette : ce qu'on peut écouter tout de suite, puis ce
        // qui vient bientôt, et enfin ce dont on ignore l'horaire.
        rows.sort_by_key(|row| {
            let rang = if row.continuous {
                0
            } else if row.next.is_some() {
                1
            } else {
                2
            };
            (
                !row.bookmarked,
                rang,
                row.next.unwrap_or(DateTime::<Utc>::MAX_UTC),
                row.id.clone(),
            )
        });
        rows
    }

    /// Publie l'état courant à l'intention du serveur web.
    ///
    /// Appelé à chaque tour de boucle par le fil qui possède l'état.
    pub fn publish(&self, link: &Link) {
        let status = self.status();
        let morse_text = status
            .as_ref()
            .map(|s| {
                if self.settings.cut_numbers {
                    crate::morse::cut_numbers(&s.morse_text)
                } else {
                    s.morse_text.clone()
                }
            })
            .unwrap_or_default();
        let station = self.session.as_ref().map(|s| s.station.clone());
        let station_name = station
            .as_deref()
            .and_then(|id| self.catalog.station(id))
            .map(|s| s.name_only())
            .unwrap_or_default();

        let log = self
            .logbook
            .entries()
            .take(20)
            .map(|e| LogRow {
                at: crate::format::stamp(e.started_at),
                station: e.station.clone(),
                frequency_khz: e.frequency_khz,
                rating: e.rating,
                tags: e.tags.join(" · "),
            })
            .collect();

        link.set_audio(self.tap.clone());
        link.publish(Snapshot {
            status,
            rows: self.rows(Utc::now()),
            station,
            station_name,
            planning_age: self.planning_age(),
            message: self.message.clone(),
            morse_text,
            log,
        });
    }

    /// Applique les demandes venues des navigateurs.
    pub fn apply_requests(&mut self, link: &Link) {
        for request in link.drain() {
            let résultat = match request {
                Request::Listen(station) => self.listen(&station),
                Request::Tune {
                    frequency_hz,
                    mode,
                    bandwidth_hz,
                } => self.tune_manual(frequency_hz, mode, bandwidth_hz),
                Request::SetMorse(on) => {
                    self.send(Command::SetMorse(on));
                    Ok(())
                }
                Request::ClearMorse => {
                    self.send(Command::ClearMorse);
                    Ok(())
                }
                Request::Recording(on) => {
                    let en_cours = self
                        .status()
                        .map(|s| s.recording.is_some())
                        .unwrap_or(false);
                    if on != en_cours {
                        self.toggle_recording();
                    }
                    Ok(())
                }
                Request::Volume(volume) => {
                    self.set_volume(volume);
                    Ok(())
                }
                Request::Refresh => {
                    self.refresh();
                    Ok(())
                }
                Request::Stop => {
                    self.stop();
                    Ok(())
                }
            };
            if let Err(err) = résultat {
                self.message = format!("{err:#}");
            }
        }
    }

    pub fn toggle_bookmark(&mut self, station: &str) {
        self.settings.toggle_bookmark(station);
        let _ = self.settings.save();
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        // Une session en cours doit se retrouver au journal, même si
        // l'application est fermée d'un coup.
        self.close_session();
        let _ = self.settings.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les essais d'état modifient une variable d'environnement commune : ils
    /// passent donc l'un après l'autre, même quand `cargo test` les lance en
    /// parallèle.
    static VERROU: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// État isolé : réglages et journal dans un dossier temporaire.
    fn etat(nom: &str) -> (AppState, std::sync::MutexGuard<'static, ()>) {
        // Un essai qui échoue empoisonne le verrou : les suivants doivent quand
        // même pouvoir s'exécuter.
        let verrou = VERROU.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("wavecraft-test-state-{nom}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: les tests d'état sont exécutés l'un après l'autre.
        unsafe { std::env::set_var("WAVECRAFT_DATA_DIR", &dir) };
        (AppState::new(true).unwrap(), verrou)
    }

    #[test]
    fn l_ecoute_en_un_geste_accorde_et_ouvre_le_journal() {
        let (mut state, _verrou) = etat("ecoute");
        state.listen("S28").unwrap();

        let session = state.session.as_ref().expect("session ouverte");
        assert_eq!(session.station, "S28");
        assert_eq!(session.entry.frequency_khz, 4625);
        assert_eq!(session.entry.mode, "USB");
        assert_eq!(state.logbook.len(), 1, "entrée de journal manquante");

        let status = state.status().expect("moteur démarré");
        assert!(status.running);
        state.stop();
    }

    #[test]
    fn une_station_sans_prereglage_est_refusee_proprement() {
        let (mut state, _verrou) = etat("sans-prereglage");
        let err = state.listen("station-qui-n-existe-pas").unwrap_err();
        assert!(format!("{err:#}").contains("inconnue"), "{err:#}");
        assert!(state.session.is_none());
    }

    #[test]
    fn le_morse_s_active_sur_une_station_en_cw() {
        let (mut state, _verrou) = etat("morse");
        state.listen("M12").unwrap();
        // M12 émet en Morse : le décodage doit s'activer tout seul.
        let mut status = state.status().unwrap();
        for _ in 0..100 {
            if status.morse_enabled {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            status = state.status().unwrap();
        }
        assert!(status.morse_enabled, "le Morse ne s'est pas activé sur M12");
        state.stop();
    }

    #[test]
    fn changer_de_station_clot_l_entree_precedente() {
        let (mut state, _verrou) = etat("changement");
        state.listen("S28").unwrap();
        state.listen("S30").unwrap();
        assert_eq!(state.logbook.len(), 2);
        let close = state
            .logbook
            .entries()
            .filter(|e| e.ended_at.is_some())
            .count();
        assert_eq!(close, 1, "la première session n'a pas été close");
        state.stop();
    }

    #[test]
    fn l_actualisation_est_demandee_au_premier_lancement() {
        let (state, _verrou) = etat("actualisation");
        assert!(state.needs_refresh(Utc::now()), "planning jamais lu");
    }

    #[test]
    fn l_actualisation_n_est_pas_redemandee_tout_de_suite() {
        let (mut state, _verrou) = etat("delai");
        state.last_refresh = Some(Utc::now());
        state.upcoming = state.catalog.upcoming(Utc::now(), TimeDelta::hours(24));
        assert!(
            !state.needs_refresh(Utc::now()),
            "actualisation redemandée trop tôt"
        );
    }

    #[test]
    fn un_planning_vide_force_l_actualisation() {
        // Après un long arrêt, le planning ne contient plus rien à venir :
        // il faut le relire même si le délai n'est pas écoulé.
        let (mut state, _verrou) = etat("planning-vide");
        state.last_refresh = Some(Utc::now());
        state.upcoming.clear();
        assert!(state.needs_refresh(Utc::now()));
    }

    #[test]
    fn les_signets_remontent_en_tete_de_liste() {
        let (mut state, _verrou) = etat("signets");
        let avant = state.rows(Utc::now())[0].id.clone();
        state.toggle_bookmark("M12");
        let apres = state.rows(Utc::now())[0].id.clone();
        assert_eq!(apres, "M12");
        assert_ne!(avant, apres);
    }
}
