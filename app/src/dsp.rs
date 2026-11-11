//! Traitement du signal : mélangeur, filtres, démodulation et cascade.
//!
//! La chaîne est la même pour tous les modes :
//!
//! ```text
//! I/Q 960 kHz → mélangeur (met la station à 0 Hz) → filtre décimateur
//!   → 48 kHz → filtre de mode → démodulation → correction de niveau → audio
//! ```
//!
//! Les stations de nombres émettent presque toujours en bande latérale unique,
//! souvent une porteuse silencieuse pendant des heures : le niveau audio est
//! donc corrigé lentement, et le squelch reste désactivé.

use std::f32::consts::{PI, TAU};

/// Fréquence d'échantillonnage de la clé. 960 kHz se divise exactement par 20
/// pour donner les 48 kHz de l'audio, ce qui évite tout rééchantillonnage.
pub const DEFAULT_SAMPLE_RATE: u32 = 960_000;

/// Fréquence de l'audio produit.
pub const AUDIO_RATE: u32 = 48_000;

/// Décalage de la note Morse : une porteuse accordée pile produit un sifflement
/// à cette fréquence, bien audible sans être fatigante.
pub const CW_PITCH_HZ: f32 = 700.0;

/// Mode de démodulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Bande latérale supérieure : le mode de presque toutes les stations vocales.
    Usb,
    /// Bande latérale inférieure.
    Lsb,
    /// Modulation d'amplitude.
    Am,
    /// Morse : bande latérale supérieure étroite, ramenée à une note audible.
    Cw,
    /// Modulation de fréquence à bande étroite.
    Nfm,
}

impl Mode {
    /// Lit un mode annoncé par le catalogue ou l'agenda Priyom.
    pub fn parse(raw: &str) -> Option<Self> {
        // Le catalogue note parfois plusieurs modes pour une même station —
        // « USB/AM », « USB, CW » : c'est le premier qui sert de préréglage.
        let premier = raw
            .split(['/', ',', ' '])
            .map(str::trim)
            .find(|m| !m.is_empty())
            .unwrap_or("");
        match premier.to_ascii_uppercase().as_str() {
            "USB" | "J3E" | "MFSK" | "PSK" | "DIGITAL" => Some(Mode::Usb),
            "LSB" => Some(Mode::Lsb),
            "AM" => Some(Mode::Am),
            "CW" | "MORSE" | "A1A" => Some(Mode::Cw),
            "FM" | "NFM" | "RTTY" | "FSK" => Some(Mode::Nfm),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Usb => "USB",
            Mode::Lsb => "LSB",
            Mode::Am => "AM",
            Mode::Cw => "CW",
            Mode::Nfm => "NFM",
        }
    }

    /// Largeur de filtre habituelle, en Hz.
    pub fn default_bandwidth(self) -> f32 {
        match self {
            Mode::Usb | Mode::Lsb => 2_700.0,
            Mode::Am => 6_000.0,
            Mode::Cw => 500.0,
            Mode::Nfm => 12_500.0,
        }
    }
}

/// Oscillateur local : ramène la station écoutée au centre du spectre.
#[derive(Debug, Clone)]
pub struct Nco {
    phase: f32,
    step: f32,
}

impl Nco {
    pub fn new(freq_hz: f32, sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            step: TAU * freq_hz / sample_rate,
        }
    }

    pub fn set_freq(&mut self, freq_hz: f32, sample_rate: f32) {
        self.step = TAU * freq_hz / sample_rate;
    }

    /// Multiplie l'échantillon par l'oscillateur et avance d'un pas.
    #[inline]
    pub fn mix(&mut self, sample: (f32, f32)) -> (f32, f32) {
        let (sin, cos) = self.phase.sin_cos();
        self.phase += self.step;
        if self.phase > PI {
            self.phase -= TAU;
        } else if self.phase < -PI {
            self.phase += TAU;
        }
        (
            sample.0 * cos - sample.1 * sin,
            sample.0 * sin + sample.1 * cos,
        )
    }
}

/// Coefficients d'un passe-bas, calculés par la méthode de la fenêtre.
///
/// La fenêtre de Blackman donne une réjection d'environ 74 dB hors bande, ce
/// qui suffit à écarter une station voisine forte.
pub fn lowpass_taps(cutoff_hz: f32, sample_rate: f32, len: usize) -> Vec<f32> {
    let len = len.max(3) | 1; // longueur impaire : la phase reste linéaire
    let fc = (cutoff_hz / sample_rate).clamp(0.000_01, 0.499);
    let middle = (len - 1) as f32 / 2.0;
    let mut taps = Vec::with_capacity(len);
    let mut sum = 0.0;
    for n in 0..len {
        let x = n as f32 - middle;
        let sinc = if x.abs() < 1e-6 {
            2.0 * fc
        } else {
            (TAU * fc * x).sin() / (PI * x)
        };
        let w = 0.42 - 0.5 * (TAU * n as f32 / (len - 1) as f32).cos()
            + 0.08 * (2.0 * TAU * n as f32 / (len - 1) as f32).cos();
        let tap = sinc * w;
        sum += tap;
        taps.push(tap);
    }
    for tap in &mut taps {
        *tap /= sum; // gain unité en continu
    }
    taps
}

/// Filtre à réponse impulsionnelle finie, à coefficients complexes, qui décime.
///
/// Les coefficients complexes permettent un filtre dissymétrique, nécessaire
/// pour ne garder qu'une seule bande latérale.
#[derive(Debug, Clone)]
pub struct Fir {
    taps: Vec<(f32, f32)>,
    history: Vec<(f32, f32)>,
    position: usize,
    decimation: usize,
    counter: usize,
}

impl Fir {
    /// Filtre passe-bas réel.
    pub fn lowpass(cutoff_hz: f32, sample_rate: f32, len: usize, decimation: usize) -> Self {
        let taps = lowpass_taps(cutoff_hz, sample_rate, len)
            .into_iter()
            .map(|t| (t, 0.0))
            .collect();
        Self::from_taps(taps, decimation)
    }

    /// Filtre passe-bande complexe, centré sur `center_hz`.
    ///
    /// C'est ce qui isole une bande latérale : le filtre laisse passer
    /// `[center - largeur/2, center + largeur/2]` et rejette les fréquences
    /// négatives, donc l'autre bande latérale.
    pub fn bandpass(center_hz: f32, width_hz: f32, sample_rate: f32, len: usize) -> Self {
        let proto = lowpass_taps(width_hz / 2.0, sample_rate, len);
        let middle = (proto.len() - 1) as f32 / 2.0;
        let w = TAU * center_hz / sample_rate;
        let taps = proto
            .iter()
            .enumerate()
            .map(|(n, tap)| {
                let phase = w * (n as f32 - middle);
                (tap * phase.cos(), tap * phase.sin())
            })
            .collect();
        Self::from_taps(taps, 1)
    }

    fn from_taps(taps: Vec<(f32, f32)>, decimation: usize) -> Self {
        let len = taps.len();
        Self {
            taps,
            history: vec![(0.0, 0.0); len],
            position: 0,
            decimation: decimation.max(1),
            counter: 0,
        }
    }

    /// Ajoute un échantillon ; renvoie une sortie une fois sur `decimation`.
    #[inline]
    pub fn push(&mut self, sample: (f32, f32)) -> Option<(f32, f32)> {
        self.history[self.position] = sample;
        self.position = (self.position + 1) % self.history.len();

        self.counter += 1;
        if self.counter < self.decimation {
            return None;
        }
        self.counter = 0;

        let len = self.history.len();
        let mut acc = (0.0, 0.0);
        for (k, tap) in self.taps.iter().enumerate() {
            // L'échantillon le plus récent se trouve juste avant `position`.
            let index = (self.position + len - 1 - k) % len;
            let x = self.history[index];
            acc.0 += x.0 * tap.0 - x.1 * tap.1;
            acc.1 += x.0 * tap.1 + x.1 * tap.0;
        }
        Some(acc)
    }
}

/// Correction de niveau à attaque rapide et retour lent.
///
/// Une station de nombres alterne de longs silences et des passages de voix :
/// un retour lent évite que le souffle ne remonte entre deux groupes.
#[derive(Debug, Clone)]
pub struct Agc {
    gain: f32,
    target: f32,
    attack: f32,
    release: f32,
    max_gain: f32,
}

impl Agc {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            gain: 1.0,
            target: 0.3,
            // ~10 ms d'attaque, ~2 s de retour.
            attack: 1.0 - (-1.0 / (0.010 * sample_rate)).exp(),
            release: 1.0 - (-1.0 / (2.0 * sample_rate)).exp(),
            max_gain: 400.0,
        }
    }

    #[inline]
    pub fn process(&mut self, sample: f32) -> f32 {
        let level = (sample * self.gain).abs();
        let coefficient = if level > self.target {
            self.attack
        } else {
            self.release
        };
        let wanted = if level > 1e-9 {
            self.gain * self.target / level
        } else {
            self.max_gain
        };
        self.gain += (wanted - self.gain) * coefficient;
        self.gain = self.gain.clamp(0.001, self.max_gain);
        (sample * self.gain).clamp(-1.0, 1.0)
    }

    pub fn gain(&self) -> f32 {
        self.gain
    }
}

/// Chaîne complète, d'un bloc I/Q brut à l'audio.
pub struct Demodulator {
    mode: Mode,
    bandwidth_hz: f32,
    sample_rate: f32,
    audio_rate: f32,
    nco: Nco,
    decimator: Fir,
    mode_filter: Fir,
    agc: Agc,
    last_phase: f32,
    dc: f32,
    /// Niveau du signal reçu, en dB relatifs à la pleine échelle.
    level_dbfs: f32,
}

impl Demodulator {
    pub fn new(mode: Mode, bandwidth_hz: f32, sample_rate: u32) -> Self {
        let sample_rate = sample_rate as f32;
        let audio_rate = AUDIO_RATE as f32;
        let decimation = (sample_rate / audio_rate).round().max(1.0) as usize;
        // Le filtre décimateur coupe à la moitié de la bande audio conservée,
        // ce qui évite tout repliement lors de la décimation.
        let decimator = Fir::lowpass(audio_rate / 2.2, sample_rate, 127, decimation);
        let mut demod = Self {
            mode,
            bandwidth_hz,
            sample_rate,
            audio_rate,
            nco: Nco::new(0.0, sample_rate),
            decimator,
            mode_filter: Fir::lowpass(bandwidth_hz / 2.0, audio_rate, 201, 1),
            agc: Agc::new(audio_rate),
            last_phase: 0.0,
            dc: 0.0,
            level_dbfs: -120.0,
        };
        demod.rebuild_mode_filter();
        demod
    }

    /// Écart entre la station écoutée et le centre de la clé, en Hz.
    ///
    /// La clé n'est jamais accordée pile sur la station : son oscillateur
    /// laisse un pic parasite au centre du spectre, qui gâcherait l'écoute.
    pub fn set_offset(&mut self, offset_hz: f32) {
        self.nco.set_freq(-offset_hz, self.sample_rate);
    }

    pub fn set_mode(&mut self, mode: Mode) {
        if self.mode != mode {
            self.mode = mode;
            self.bandwidth_hz = mode.default_bandwidth();
            self.rebuild_mode_filter();
        }
    }

    pub fn set_bandwidth(&mut self, bandwidth_hz: f32) {
        let clamped = bandwidth_hz.clamp(100.0, self.audio_rate / 2.0 - 500.0);
        if (clamped - self.bandwidth_hz).abs() > 1.0 {
            self.bandwidth_hz = clamped;
            self.rebuild_mode_filter();
        }
    }

    pub fn bandwidth(&self) -> f32 {
        self.bandwidth_hz
    }

    /// Niveau du signal reçu, en dB pleine échelle.
    pub fn level_dbfs(&self) -> f32 {
        self.level_dbfs
    }

    fn rebuild_mode_filter(&mut self) {
        let bw = self.bandwidth_hz;
        self.mode_filter = match self.mode {
            // Une seule bande latérale : filtre complexe décalé d'une demi-largeur.
            Mode::Usb => Fir::bandpass(bw / 2.0, bw, self.audio_rate, 201),
            Mode::Lsb => Fir::bandpass(-bw / 2.0, bw, self.audio_rate, 201),
            // Morse : bande latérale supérieure étroite, centrée sur la note.
            Mode::Cw => Fir::bandpass(CW_PITCH_HZ, bw, self.audio_rate, 301),
            // Amplitude et fréquence gardent les deux côtés du spectre.
            Mode::Am | Mode::Nfm => Fir::lowpass(bw / 2.0, self.audio_rate, 201, 1),
        };
    }

    /// Démodule un bloc d'échantillons I/Q et ajoute l'audio à `out`.
    pub fn process(&mut self, iq: &[(f32, f32)], out: &mut Vec<f32>) {
        let mut peak = 0.0f32;
        for &sample in iq {
            let mixed = self.nco.mix(sample);
            let Some(baseband) = self.decimator.push(mixed) else {
                continue;
            };
            let Some(filtered) = self.mode_filter.push(baseband) else {
                continue;
            };

            let magnitude = (filtered.0 * filtered.0 + filtered.1 * filtered.1).sqrt();
            peak = peak.max(magnitude);

            let audio = match self.mode {
                // Une bande latérale isolée est un signal analytique : sa partie
                // réelle est l'audio recherché.
                Mode::Usb | Mode::Lsb | Mode::Cw => filtered.0,
                Mode::Am => {
                    // Retrait lent de la composante continue, c'est-à-dire de la
                    // porteuse.
                    self.dc += (magnitude - self.dc) * 0.0005;
                    magnitude - self.dc
                }
                Mode::Nfm => {
                    let phase = filtered.1.atan2(filtered.0);
                    let mut delta = phase - self.last_phase;
                    self.last_phase = phase;
                    if delta > PI {
                        delta -= TAU;
                    } else if delta < -PI {
                        delta += TAU;
                    }
                    delta / PI
                }
            };
            out.push(self.agc.process(audio));
        }

        if peak > 0.0 {
            let instant = 20.0 * peak.max(1e-9).log10();
            // Lissage, pour un indicateur de niveau lisible.
            self.level_dbfs += (instant - self.level_dbfs) * 0.2;
        }
    }

    pub fn agc_gain(&self) -> f32 {
        self.agc.gain()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Signal d'essai : une porteuse à `freq_hz` du centre, d'amplitude donnée.
    fn carrier(freq_hz: f32, sample_rate: f32, len: usize, amplitude: f32) -> Vec<(f32, f32)> {
        (0..len)
            .map(|n| {
                let phase = TAU * freq_hz * n as f32 / sample_rate;
                (amplitude * phase.cos(), amplitude * phase.sin())
            })
            .collect()
    }

    /// Estime la fréquence dominante d'un signal audio, par comptage des
    /// passages par zéro sur la seconde moitié (la première contient le
    /// transitoire des filtres).
    fn dominant_freq(audio: &[f32], sample_rate: f32) -> f32 {
        let tail = &audio[audio.len() / 2..];
        let crossings = tail
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        crossings as f32 * sample_rate / (2.0 * tail.len() as f32)
    }

    #[test]
    fn le_passe_bas_a_un_gain_unite_en_continu() {
        let taps = lowpass_taps(3_000.0, 48_000.0, 101);
        let sum: f32 = taps.iter().sum();
        assert!((sum - 1.0).abs() < 1e-4, "somme des coefficients = {sum}");
        assert_eq!(taps.len(), 101);
    }

    #[test]
    fn la_longueur_de_filtre_est_toujours_impaire() {
        assert_eq!(lowpass_taps(1_000.0, 48_000.0, 100).len(), 101);
    }

    #[test]
    fn l_oscillateur_ramene_la_station_au_centre() {
        let sample_rate = 48_000.0;
        let input = carrier(1_000.0, sample_rate, 4_096, 1.0);
        let mut nco = Nco::new(-1_000.0, sample_rate);
        let mixed: Vec<(f32, f32)> = input.iter().map(|&s| nco.mix(s)).collect();
        // Une porteuse ramenée à 0 Hz ne tourne plus : sa phase reste constante.
        let first = mixed[100];
        let last = mixed[4_000];
        let ecart = (first.0 - last.0).abs() + (first.1 - last.1).abs();
        assert!(ecart < 0.05, "la porteuse tourne encore : écart {ecart}");
    }

    #[test]
    fn une_porteuse_dans_la_bande_laterale_superieure_donne_une_note() {
        // Station accordée à 1 500 Hz au-dessus de la fréquence affichée :
        // en USB, on doit entendre une note à 1 500 Hz.
        let sample_rate = DEFAULT_SAMPLE_RATE;
        let input = carrier(1_500.0, sample_rate as f32, 200_000, 0.5);
        let mut demod = Demodulator::new(Mode::Usb, 2_700.0, sample_rate);
        let mut audio = Vec::new();
        demod.process(&input, &mut audio);

        assert!(audio.len() > 2_000, "audio trop court : {}", audio.len());
        let freq = dominant_freq(&audio, AUDIO_RATE as f32);
        assert!(
            (freq - 1_500.0).abs() < 60.0,
            "note attendue à 1 500 Hz, obtenue à {freq:.0} Hz"
        );
    }

    #[test]
    fn la_bande_laterale_inferieure_est_rejetee_en_usb() {
        let sample_rate = DEFAULT_SAMPLE_RATE;
        // Porteuse 1 500 Hz *en dessous* : en USB elle doit être étouffée.
        let input = carrier(-1_500.0, sample_rate as f32, 200_000, 0.5);
        let mut demod = Demodulator::new(Mode::Usb, 2_700.0, sample_rate);
        let mut audio = Vec::new();
        demod.process(&input, &mut audio);

        // La correction de niveau pousse le gain à fond sur un signal étouffé :
        // c'est le niveau mesuré avant correction qui compte.
        assert!(
            demod.level_dbfs() < -25.0,
            "la bande rejetée reste à {:.1} dBFS",
            demod.level_dbfs()
        );
    }

    #[test]
    fn le_morse_produit_une_note_audible() {
        let sample_rate = DEFAULT_SAMPLE_RATE;
        // En Morse, la station est accordée pile : le décalage de note doit
        // produire un sifflement à CW_PITCH_HZ.
        let input = carrier(CW_PITCH_HZ, sample_rate as f32, 200_000, 0.5);
        let mut demod = Demodulator::new(Mode::Cw, 500.0, sample_rate);
        let mut audio = Vec::new();
        demod.process(&input, &mut audio);
        let freq = dominant_freq(&audio, AUDIO_RATE as f32);
        assert!(
            (freq - CW_PITCH_HZ).abs() < 60.0,
            "note Morse attendue à {CW_PITCH_HZ} Hz, obtenue à {freq:.0} Hz"
        );
    }

    #[test]
    fn la_decimation_donne_le_debit_audio_attendu() {
        let sample_rate = DEFAULT_SAMPLE_RATE;
        let input = carrier(1_000.0, sample_rate as f32, sample_rate as usize, 0.5);
        let mut demod = Demodulator::new(Mode::Usb, 2_700.0, sample_rate);
        let mut audio = Vec::new();
        demod.process(&input, &mut audio);
        // Une seconde de signal doit donner une seconde d'audio.
        let ecart = (audio.len() as i64 - i64::from(AUDIO_RATE)).abs();
        assert!(ecart < 10, "{} échantillons audio produits", audio.len());
    }

    #[test]
    fn la_correction_de_niveau_remonte_un_signal_faible() {
        let mut agc = Agc::new(AUDIO_RATE as f32);
        let mut last = 0.0;
        for n in 0..AUDIO_RATE * 3 {
            let x = 0.001 * (TAU * 800.0 * n as f32 / AUDIO_RATE as f32).sin();
            last = agc.process(x);
        }
        assert!(agc.gain() > 10.0, "gain resté à {}", agc.gain());
        assert!(last.abs() <= 1.0, "sortie saturée : {last}");
    }

    #[test]
    fn la_correction_de_niveau_ne_sature_pas_un_signal_fort() {
        let mut agc = Agc::new(AUDIO_RATE as f32);
        let mut peak: f32 = 0.0;
        for n in 0..AUDIO_RATE * 2 {
            let x = 0.9 * (TAU * 800.0 * n as f32 / AUDIO_RATE as f32).sin();
            peak = peak.max(agc.process(x).abs());
        }
        assert!(peak <= 1.0, "sortie saturée : {peak}");
    }

    #[test]
    fn les_modes_annonces_par_priyom_sont_reconnus() {
        assert_eq!(Mode::parse("USB"), Some(Mode::Usb));
        assert_eq!(Mode::parse("usb"), Some(Mode::Usb));
        assert_eq!(Mode::parse("CW"), Some(Mode::Cw));
        assert_eq!(Mode::parse("AM"), Some(Mode::Am));
        assert_eq!(Mode::parse("RTTY"), Some(Mode::Nfm));
        assert_eq!(Mode::parse("J3E"), Some(Mode::Usb));
        assert_eq!(Mode::parse("brouillard"), None);
        // Plusieurs modes annoncés : le premier l'emporte.
        assert_eq!(Mode::parse("USB/AM"), Some(Mode::Usb));
        assert_eq!(Mode::parse("CW, USB"), Some(Mode::Cw));
        assert_eq!(Mode::parse(" AM "), Some(Mode::Am));
        assert_eq!(Mode::parse(""), None);
    }

    #[test]
    fn chaque_mode_a_une_largeur_de_filtre_coherente() {
        assert_eq!(Mode::Cw.default_bandwidth(), 500.0);
        assert_eq!(Mode::Usb.default_bandwidth(), 2_700.0);
        assert_eq!(Mode::Am.default_bandwidth(), 6_000.0);
    }

    #[test]
    fn la_largeur_de_filtre_reste_dans_les_limites() {
        let mut demod = Demodulator::new(Mode::Usb, 2_700.0, DEFAULT_SAMPLE_RATE);
        demod.set_bandwidth(1_000_000.0);
        assert!(demod.bandwidth() < AUDIO_RATE as f32 / 2.0);
        demod.set_bandwidth(1.0);
        assert!(demod.bandwidth() >= 100.0);
    }
}
