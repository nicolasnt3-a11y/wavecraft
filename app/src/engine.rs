//! Moteur de réception : un fil, une source, une chaîne de traitement.
//!
//! Le fil de réception ne s'arrête jamais sur autre chose que la lecture de la
//! clé : les réglages arrivent par messages, et l'état consultable est recopié
//! dans une structure partagée. L'interface — fenêtre ou navigateur — ne fait
//! donc que lire cet état et envoyer des ordres, sans jamais bloquer l'audio.
//!
//! ```text
//!  source ──► mélangeur ──► démodulation ──┬─► file audio ──► carte son / navigateur
//!     │                                    ├─► décodeur Morse
//!     │                                    └─► enregistreur WAV
//!     └──────► transformée de Fourier ─────► cascade
//! ```

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use anyhow::{Context, Result};
use rustfft::{FftPlanner, num_complex::Complex32};

use crate::audio::AudioSink;
use crate::dsp::{AUDIO_RATE, DEFAULT_SAMPLE_RATE, Demodulator, Mode};
use crate::morse::MorseDecoder;
use crate::recorder::Recorder;
use crate::sdr::{Gain, Source, to_iq};

/// Taille d'un bloc lu sur la clé. 128 Ki-échantillons I/Q font environ 68 ms à
/// 960 kHz : assez court pour que l'écoute réagisse, assez long pour que l'USB
/// travaille par gros paquets.
const BLOCK_BYTES: usize = 256 * 1024;

/// Nombre de points de la transformée de Fourier de la cascade.
pub const SPECTRUM_BINS: usize = 2048;

/// Écart entre la fréquence écoutée et le centre de la clé.
///
/// Le RTL2832U laisse un pic parasite au centre de son spectre — le défaut de
/// son convertisseur. On accorde donc la clé à côté, et le mélangeur ramène la
/// station au centre par le calcul.
pub const TUNING_OFFSET_HZ: i64 = 250_000;

/// Ordre envoyé au fil de réception.
#[derive(Debug, Clone)]
pub enum Command {
    /// Écouter une station : fréquence, mode et largeur de filtre.
    Tune {
        frequency_hz: u32,
        mode: Mode,
        bandwidth_hz: f32,
    },
    /// Change de mode sans réaccorder la clé ni couper le son.
    SetMode(Mode),
    /// Change la largeur du filtre sans réaccorder la clé ni effacer le Morse.
    SetBandwidth(f32),
    SetGain(Gain),
    SetPpm(i32),
    SetBiasTee(bool),
    SetMorse(bool),
    StartRecording {
        dir: PathBuf,
        station: String,
    },
    StopRecording,
    /// Efface le texte décodé.
    ClearMorse,
    Stop,
}

/// État consultable du moteur.
#[derive(Debug, Clone)]
pub struct Status {
    pub source: String,
    pub running: bool,
    /// Fréquence écoutée, en Hz.
    pub frequency_hz: u32,
    /// Fréquence réellement affichée par la clé, en Hz.
    pub center_freq_hz: u32,
    pub sample_rate: u32,
    pub mode: Mode,
    pub bandwidth_hz: f32,
    pub level_dbfs: f32,
    pub agc_gain: f32,
    /// Cascade : puissance par canal, en dB, du plus bas au plus haut.
    pub spectrum: Vec<f32>,
    pub morse_enabled: bool,
    pub morse_text: String,
    /// Vitesse mesurée du Morse, en mots par minute.
    pub morse_wpm: f32,
    /// Tonalité suivie par le décodeur, en hertz.
    pub morse_pitch_hz: f32,
    pub recording: Option<PathBuf>,
    pub recorded_secs: f64,
    pub bias_tee: bool,
    /// Dernière erreur rencontrée par le fil de réception.
    pub error: Option<String>,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            source: String::new(),
            running: false,
            frequency_hz: 0,
            center_freq_hz: 0,
            sample_rate: DEFAULT_SAMPLE_RATE,
            mode: Mode::Usb,
            bandwidth_hz: 2_700.0,
            level_dbfs: -120.0,
            agc_gain: 1.0,
            spectrum: vec![-120.0; SPECTRUM_BINS / 2],
            morse_enabled: false,
            morse_text: String::new(),
            morse_wpm: 0.0,
            morse_pitch_hz: crate::dsp::CW_PITCH_HZ,
            recording: None,
            recorded_secs: 0.0,
            bias_tee: false,
            error: None,
        }
    }
}

/// Poignée sur le fil de réception.
pub struct Engine {
    commands: Sender<Command>,
    status: Arc<Mutex<Status>>,
    audio: AudioSink,
    /// Files secondaires : le serveur web reçoit le même son que la carte son,
    /// sans le lui prendre.
    taps: Arc<Mutex<Vec<AudioSink>>>,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    /// Démarre la réception sur `source`.
    pub fn start(source: Box<dyn Source>) -> Result<Self> {
        let audio = AudioSink::new(AUDIO_RATE);
        let status = Arc::new(Mutex::new(Status {
            source: source.name(),
            running: true,
            sample_rate: source.sample_rate(),
            ..Status::default()
        }));
        let (commands, inbox) = channel();

        let taps: Arc<Mutex<Vec<AudioSink>>> = Arc::new(Mutex::new(Vec::new()));
        let worker_status = Arc::clone(&status);
        let worker_audio = audio.clone();
        let worker_taps = Arc::clone(&taps);
        let thread = std::thread::Builder::new()
            .name("wavecraft-rx".into())
            .spawn(move || {
                let mut worker = Worker::new(source, worker_audio, worker_status);
                worker.taps = worker_taps;
                worker.run(&inbox);
            })
            .context("démarrage du fil de réception")?;

        Ok(Self {
            commands,
            status,
            audio,
            taps,
            thread: Some(thread),
        })
    }

    pub fn send(&self, command: Command) {
        // Le fil peut s'être arrêté sur une erreur : l'ordre est alors perdu,
        // ce qui est sans conséquence puisque l'état le signale déjà.
        let _ = self.commands.send(command);
    }

    /// Écoute une station en un seul geste.
    pub fn tune(&self, frequency_hz: u32, mode: Mode, bandwidth_hz: f32) {
        self.send(Command::Tune {
            frequency_hz,
            mode,
            bandwidth_hz,
        });
    }

    pub fn status(&self) -> Status {
        self.status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }

    pub fn audio(&self) -> AudioSink {
        self.audio.clone()
    }

    /// Ouvre une file audio secondaire, alimentée en parallèle de la première.
    ///
    /// Sert au serveur web : il lit le son sans le retirer à la carte son.
    pub fn tap(&self) -> AudioSink {
        let sink = AudioSink::new(AUDIO_RATE);
        sink.set_volume(1.0);
        if let Ok(mut taps) = self.taps.lock() {
            taps.push(sink.clone());
        }
        sink
    }

    /// Arrête le fil et attend sa fin.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Ce qui tourne dans le fil de réception.
struct Worker {
    source: Box<dyn Source>,
    audio: AudioSink,
    taps: Arc<Mutex<Vec<AudioSink>>>,
    status: Arc<Mutex<Status>>,
    demodulator: Demodulator,
    morse: MorseDecoder,
    morse_enabled: bool,
    recorder: Option<Recorder>,
    raw: Vec<u8>,
    iq: Vec<(f32, f32)>,
    samples: Vec<f32>,
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    window: Vec<f32>,
    frequency_hz: u32,
}

impl Worker {
    fn new(source: Box<dyn Source>, audio: AudioSink, status: Arc<Mutex<Status>>) -> Self {
        let sample_rate = source.sample_rate().max(1);
        let mut planner = FftPlanner::new();
        // Fenêtre de Hann : les raies de la cascade restent fines et les
        // fuites entre canaux voisins sont faibles.
        let window = (0..SPECTRUM_BINS)
            .map(|n| {
                0.5 - 0.5
                    * (std::f32::consts::TAU * n as f32 / (SPECTRUM_BINS - 1) as f32).cos()
            })
            .collect();
        Self {
            source,
            audio,
            taps: Arc::new(Mutex::new(Vec::new())),
            status,
            demodulator: Demodulator::new(Mode::Usb, 2_700.0, sample_rate),
            morse: MorseDecoder::default(),
            morse_enabled: false,
            recorder: None,
            raw: vec![0; BLOCK_BYTES],
            iq: Vec::with_capacity(BLOCK_BYTES / 2),
            samples: Vec::with_capacity(AUDIO_RATE as usize),
            fft: planner.plan_fft_forward(SPECTRUM_BINS),
            window,
            frequency_hz: 0,
        }
    }

    fn run(&mut self, inbox: &Receiver<Command>) {
        loop {
            match self.pump(inbox) {
                Ok(true) => {}
                Ok(false) => break,
                Err(err) => {
                    self.with_status(|s| {
                        s.error = Some(format!("{err:#}"));
                        s.running = false;
                    });
                    break;
                }
            }
        }
        // Un enregistrement en cours est refermé proprement.
        if let Some(recorder) = self.recorder.take() {
            let _ = recorder.finish();
        }
        self.with_status(|s| s.running = false);
    }

    /// Un tour : ordres en attente, puis un bloc de signal.
    fn pump(&mut self, inbox: &Receiver<Command>) -> Result<bool> {
        loop {
            match inbox.try_recv() {
                Ok(Command::Stop) => return Ok(false),
                Ok(command) => self.apply(command)?,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(false),
            }
        }

        let read = self.source.read(&mut self.raw)?;
        if read < 2 {
            return Ok(true);
        }
        let raw = &self.raw[..read];
        to_iq(raw, &mut self.iq);

        self.samples.clear();
        self.demodulator.process(&self.iq, &mut self.samples);
        self.audio.push(&self.samples);
        if let Ok(taps) = self.taps.lock() {
            for tap in taps.iter() {
                tap.push(&self.samples);
            }
        }

        let decoded = if self.morse_enabled {
            self.morse.process(&self.samples)
        } else {
            String::new()
        };

        let mut recorded_secs = 0.0;
        if let Some(recorder) = self.recorder.as_mut() {
            recorder.write(&self.samples)?;
            recorded_secs = recorder.duration_secs();
        }

        let spectrum = self.spectrum();
        let level = self.demodulator.level_dbfs();
        let gain = self.demodulator.agc_gain();
        let center = self.source.center_freq();
        let morse_text = self.morse.text().to_string();
        let morse_wpm = self.morse.wpm();
        let morse_pitch = self.morse.pitch_hz();
        let has_recorder = self.recorder.is_some();

        self.with_status(move |s| {
            s.level_dbfs = level;
            s.agc_gain = gain;
            s.center_freq_hz = center;
            s.spectrum = spectrum;
            s.recorded_secs = if has_recorder { recorded_secs } else { 0.0 };
            if !decoded.is_empty() {
                s.morse_text = morse_text;
            }
            s.morse_wpm = morse_wpm;
            s.morse_pitch_hz = morse_pitch;
        });
        Ok(true)
    }

    /// Puissance par canal, en dB, centrée sur la fréquence écoutée.
    fn spectrum(&mut self) -> Vec<f32> {
        if self.iq.len() < SPECTRUM_BINS {
            return vec![-120.0; SPECTRUM_BINS / 2];
        }
        let mut buffer: Vec<Complex32> = self.iq[..SPECTRUM_BINS]
            .iter()
            .zip(&self.window)
            .map(|(&(i, q), &w)| Complex32::new(i * w, q * w))
            .collect();
        self.fft.process(&mut buffer);

        // Le spectre d'un signal complexe est rangé par rustfft de 0 à +Fe/2
        // puis de -Fe/2 à 0. On découpe une fenêtre d'une demi-bande centrée
        // sur la station écoutée (et non sur le centre de la clé, qui porte le
        // pic DC) : le repère central de la cascade tombe ainsi pile dessus.
        let n = SPECTRUM_BINS as isize;
        let half = SPECTRUM_BINS / 2;
        let sample_rate = self.source.sample_rate().max(1) as f64;
        let offset_hz = if self.frequency_hz > 0 {
            i64::from(self.frequency_hz) - i64::from(self.source.center_freq())
        } else {
            0
        };
        let center_bin = ((offset_hz as f64 / sample_rate) * SPECTRUM_BINS as f64).round() as isize;
        let start_bin = center_bin - (half as isize) / 2;

        let scale = 1.0 / SPECTRUM_BINS as f32;
        let mut out = Vec::with_capacity(half);
        for index in 0..half {
            let source = (start_bin + index as isize).rem_euclid(n) as usize;
            let value = buffer[source] * scale;
            let power = value.norm_sqr().max(1e-20);
            out.push(10.0 * power.log10());
        }
        out
    }

    fn apply(&mut self, command: Command) -> Result<()> {
        match command {
            Command::Tune {
                frequency_hz,
                mode,
                bandwidth_hz,
            } => self.tune(frequency_hz, mode, bandwidth_hz)?,
            Command::SetMode(mode) => {
                self.demodulator.set_mode(mode);
                let bandwidth = self.demodulator.bandwidth();
                self.with_status(move |s| {
                    s.mode = mode;
                    s.bandwidth_hz = bandwidth;
                });
            }
            Command::SetBandwidth(bandwidth_hz) => {
                self.demodulator.set_bandwidth(bandwidth_hz);
                let bandwidth = self.demodulator.bandwidth();
                self.with_status(move |s| {
                    s.bandwidth_hz = bandwidth;
                });
            }
            Command::SetGain(gain) => self.source.set_gain(gain)?,
            Command::SetPpm(ppm) => self.source.set_ppm(ppm)?,
            Command::SetBiasTee(on) => {
                self.source.set_bias_tee(on)?;
                self.with_status(move |s| s.bias_tee = on);
            }
            Command::SetMorse(on) => {
                self.morse_enabled = on;
                self.with_status(|s| s.morse_enabled = on);
            }
            Command::ClearMorse => {
                self.morse.clear();
                self.with_status(|s| s.morse_text.clear());
            }
            Command::StartRecording { dir, station } => {
                let recorder = Recorder::start(
                    &dir,
                    &station,
                    (self.frequency_hz / 1_000).max(1),
                    AUDIO_RATE,
                )?;
                let path = recorder.path().to_path_buf();
                self.recorder = Some(recorder);
                self.with_status(move |s| s.recording = Some(path));
            }
            Command::StopRecording => {
                if let Some(recorder) = self.recorder.take() {
                    recorder.finish()?;
                }
                self.with_status(|s| {
                    s.recording = None;
                    s.recorded_secs = 0.0;
                });
            }
            Command::Stop => {}
        }
        Ok(())
    }

    fn tune(&mut self, frequency_hz: u32, mode: Mode, bandwidth_hz: f32) -> Result<()> {
        // La clé est accordée à côté de la station, pour éviter son pic central.
        let center = i64::from(frequency_hz) - TUNING_OFFSET_HZ;
        let center = center.max(i64::from(frequency_hz) / 2).max(1) as u32;

        self.source.set_center_freq(center)?;
        let actual = self.source.center_freq();
        let offset = i64::from(frequency_hz) - i64::from(actual);

        self.demodulator.set_mode(mode);
        self.demodulator.set_bandwidth(bandwidth_hz);
        self.demodulator.set_offset(offset as f32);
        self.morse.clear();
        self.audio.clear();
        self.frequency_hz = frequency_hz;

        let sample_rate = self.source.sample_rate();
        let bandwidth = self.demodulator.bandwidth();
        self.with_status(move |s| {
            s.frequency_hz = frequency_hz;
            s.center_freq_hz = actual;
            s.sample_rate = sample_rate;
            s.mode = mode;
            s.bandwidth_hz = bandwidth;
            s.morse_text.clear();
        });
        Ok(())
    }

    fn with_status(&self, edit: impl FnOnce(&mut Status)) {
        match self.status.lock() {
            Ok(mut status) => edit(&mut status),
            Err(poisoned) => edit(&mut poisoned.into_inner()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdr::demo::Demo;
    use std::time::{Duration, Instant};

    /// Attend qu'une condition sur l'état devienne vraie.
    fn attendre(engine: &Engine, limite: Duration, condition: impl Fn(&Status) -> bool) -> Status {
        let debut = Instant::now();
        loop {
            let status = engine.status();
            if condition(&status) || debut.elapsed() > limite {
                return status;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn le_moteur_produit_de_l_audio() {
        let engine = Engine::start(Box::new(Demo::new())).unwrap();
        engine.tune(4_625_000, Mode::Usb, 2_700.0);
        let audio = engine.audio();

        let debut = Instant::now();
        while audio.pending() == 0 && debut.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(audio.pending() > 0, "aucun audio produit");
        engine.stop();
    }

    #[test]
    fn l_accord_se_reflete_dans_l_etat() {
        let engine = Engine::start(Box::new(Demo::new())).unwrap();
        engine.tune(6_917_000, Mode::Cw, 500.0);
        let status = attendre(&engine, Duration::from_secs(5), |s| s.frequency_hz == 6_917_000);

        assert_eq!(status.frequency_hz, 6_917_000);
        assert_eq!(status.mode, Mode::Cw);
        assert_eq!(status.bandwidth_hz, 500.0);
        // La clé doit être accordée à côté, jamais pile sur la station.
        assert_ne!(status.center_freq_hz, 6_917_000);
        assert_eq!(
            i64::from(status.frequency_hz) - i64::from(status.center_freq_hz),
            TUNING_OFFSET_HZ
        );
        engine.stop();
    }

    #[test]
    fn la_cascade_se_remplit() {
        let engine = Engine::start(Box::new(Demo::new())).unwrap();
        engine.tune(4_625_000, Mode::Usb, 2_700.0);
        let status = attendre(&engine, Duration::from_secs(5), |s| {
            s.spectrum.iter().any(|&v| v > -119.0)
        });
        assert_eq!(status.spectrum.len(), SPECTRUM_BINS / 2);
        assert!(
            status.spectrum.iter().any(|&v| v > -119.0),
            "cascade restée vide"
        );
        engine.stop();
    }

    #[test]
    fn un_enregistrement_produit_un_fichier_lisible() {
        let dir = std::env::temp_dir().join("wavecraft-test-engine-rec");
        let _ = std::fs::remove_dir_all(&dir);

        let engine = Engine::start(Box::new(Demo::new())).unwrap();
        engine.tune(4_625_000, Mode::Usb, 2_700.0);
        engine.send(Command::StartRecording {
            dir: dir.clone(),
            station: "S28".into(),
        });

        let status = attendre(&engine, Duration::from_secs(5), |s| s.recorded_secs > 0.2);
        let chemin = status.recording.clone().expect("enregistrement non signalé");
        engine.send(Command::StopRecording);
        attendre(&engine, Duration::from_secs(5), |s| s.recording.is_none());
        engine.stop();

        let lecteur = hound::WavReader::open(&chemin).expect("fichier WAV illisible");
        assert_eq!(lecteur.spec().sample_rate, AUDIO_RATE);
        assert!(lecteur.len() > 0, "enregistrement vide");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn le_decodage_morse_s_active_a_la_demande() {
        let engine = Engine::start(Box::new(Demo::new())).unwrap();
        engine.send(Command::SetMorse(true));
        engine.tune(4_625_000, Mode::Cw, 500.0);
        let status = attendre(&engine, Duration::from_secs(3), |s| s.morse_enabled);
        assert!(status.morse_enabled);
        engine.stop();
    }

    #[test]
    fn la_cascade_est_centree_sur_la_station_ecoutee() {
        let engine = Engine::start(Box::new(Demo::new())).unwrap();
        engine.tune(4_625_000, Mode::Usb, 2_700.0);
        let status = attendre(&engine, Duration::from_secs(5), |s| {
            s.frequency_hz == 4_625_000 && s.spectrum.iter().any(|&v| v > -60.0)
        });
        let (pic, _) = status
            .spectrum
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .expect("spectre vide");
        let centre = status.spectrum.len() / 2;
        assert!(
            (pic as isize - centre as isize).abs() <= 4,
            "la porteuse de démonstration est au canal {pic} au lieu du centre {centre}"
        );
        engine.stop();
    }

    #[test]
    fn changer_de_filtre_ou_de_mode_ne_reaccorde_pas() {
        let engine = Engine::start(Box::new(Demo::new())).unwrap();
        engine.tune(4_625_000, Mode::Usb, 2_700.0);
        attendre(&engine, Duration::from_secs(5), |s| s.frequency_hz == 4_625_000);

        engine.send(Command::SetBandwidth(1_800.0));
        let status = attendre(&engine, Duration::from_secs(3), |s| {
            (s.bandwidth_hz - 1_800.0).abs() < 1.0
        });
        assert_eq!(status.mode, Mode::Usb);
        assert_eq!(status.frequency_hz, 4_625_000);

        engine.send(Command::SetMode(Mode::Cw));
        let status = attendre(&engine, Duration::from_secs(3), |s| s.mode == Mode::Cw);
        assert_eq!(status.bandwidth_hz, Mode::Cw.default_bandwidth());
        assert_eq!(status.frequency_hz, 4_625_000);

        engine.send(Command::SetBiasTee(true));
        let status = attendre(&engine, Duration::from_secs(3), |s| s.bias_tee);
        assert!(status.bias_tee);
        engine.stop();
    }

    #[test]
    fn l_arret_termine_le_fil() {
        let engine = Engine::start(Box::new(Demo::new())).unwrap();
        attendre(&engine, Duration::from_secs(2), |s| s.running);
        engine.send(Command::Stop);
        std::thread::sleep(Duration::from_millis(300));
        // `stop` attend la fin du fil : s'il ne se terminait pas, le test
        // resterait bloqué ici.
        drop(engine);
    }
}
