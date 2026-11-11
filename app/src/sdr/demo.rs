//! Source de démonstration : une station de nombres fabriquée de toutes pièces.
//!
//! Elle permet d'essayer Wavecraft, de régler l'audio et de vérifier la cascade
//! sans clé branchée — et sert de signal d'essai reproductible aux tests.
//!
//! Le signal imite ce qu'on entend réellement : une porteuse qui tourne
//! longtemps à vide, puis un indicatif en Morse, puis des groupes de cinq
//! chiffres. Rien de tout cela n'est un enregistrement : ce sont des tonalités
//! synthétiques, aucun extrait de station réelle n'est livré avec le logiciel.

use std::f32::consts::TAU;

use anyhow::Result;

use super::{Gain, Source};

/// Décalage de la station par rapport au centre, en Hz.
const OFFSET_HZ: f32 = 15_000.0;

/// Vitesse du Morse, en points par seconde (environ 12 mots/minute).
const DOT_PER_SEC: f32 = 10.0;

/// Indicatif émis en boucle, en Morse : « VC » pour Wavecraft.
const CALL: &[&str] = &["...-", "-.-."];

pub struct Demo {
    sample_rate: u32,
    center_freq: u32,
    phase: f32,
    clock: u64,
    noise: u32,
}

impl Default for Demo {
    fn default() -> Self {
        Self::new()
    }
}

impl Demo {
    pub fn new() -> Self {
        Self {
            sample_rate: crate::dsp::DEFAULT_SAMPLE_RATE,
            center_freq: 4_625_000,
            phase: 0.0,
            clock: 0,
            noise: 0x1234_5678,
        }
    }

    /// Bruit blanc léger, par registre à décalage : rapide et reproductible.
    #[inline]
    fn noise(&mut self) -> f32 {
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 17;
        self.noise ^= self.noise << 5;
        // Assez de bruit pour que la cascade ait un fond, assez peu pour que la
        // porteuse s'y détache comme sur une bande réelle bien dégagée.
        (self.noise as f32 / u32::MAX as f32 - 0.5) * 0.02
    }

    /// Niveau de la porteuse à l'instant courant : silence, Morse ou voix.
    fn envelope(&self, seconds: f32) -> f32 {
        let cycle = seconds % 30.0;
        match cycle {
            // 0 → 12 s : porteuse seule, comme une station au repos.
            c if c < 12.0 => 0.15,
            // 12 → 18 s : indicatif en Morse.
            c if c < 18.0 => {
                let t = (c - 12.0) * DOT_PER_SEC;
                if morse_on(CALL, t) { 1.0 } else { 0.15 }
            }
            // 18 → 30 s : groupes de chiffres, simulés par une modulation.
            c => {
                let group = ((c - 18.0) * 1.5) as u32;
                let inside = ((c - 18.0) * 1.5).fract();
                if inside < 0.75 {
                    0.4 + 0.5 * (TAU * (3.0 + (group % 5) as f32) * inside).sin().abs()
                } else {
                    0.15
                }
            }
        }
    }
}

/// Vrai si le manipulateur Morse est abaissé à l'instant `t`, compté en points.
fn morse_on(letters: &[&str], t: f32) -> bool {
    let mut position = 0.0;
    for (index, letter) in letters.iter().enumerate() {
        for symbol in letter.chars() {
            let length = if symbol == '-' { 3.0 } else { 1.0 };
            if t >= position && t < position + length {
                return true;
            }
            position += length + 1.0; // silence d'un point entre symboles
        }
        position += 2.0; // silence de trois points entre lettres
        if index + 1 == letters.len() {
            position += 4.0; // silence plus long avant la reprise
        }
    }
    // Boucle sur la longueur totale de l'indicatif.
    if position > 0.0 {
        morse_on_wrapped(letters, t % position, position)
    } else {
        false
    }
}

fn morse_on_wrapped(letters: &[&str], t: f32, total: f32) -> bool {
    if t >= total {
        return false;
    }
    let mut position = 0.0;
    for letter in letters {
        for symbol in letter.chars() {
            let length = if symbol == '-' { 3.0 } else { 1.0 };
            if t >= position && t < position + length {
                return true;
            }
            position += length + 1.0;
        }
        position += 2.0;
    }
    false
}

impl Source for Demo {
    fn name(&self) -> String {
        "démonstration (sans clé)".to_string()
    }

    fn set_center_freq(&mut self, hz: u32) -> Result<()> {
        self.center_freq = hz;
        Ok(())
    }

    fn center_freq(&self) -> u32 {
        self.center_freq
    }

    fn set_sample_rate(&mut self, hz: u32) -> Result<()> {
        self.sample_rate = hz.max(1);
        Ok(())
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn set_gain(&mut self, _gain: Gain) -> Result<()> {
        Ok(())
    }

    fn read(&mut self, out: &mut [u8]) -> Result<usize> {
        let step = TAU * OFFSET_HZ / self.sample_rate as f32;
        for pair in out.chunks_exact_mut(2) {
            let seconds = self.clock as f32 / self.sample_rate as f32;
            let amplitude = self.envelope(seconds);
            let (sin, cos) = self.phase.sin_cos();
            self.phase = (self.phase + step) % TAU;

            let i = amplitude * cos + self.noise();
            let q = amplitude * sin + self.noise();
            // Retour au format de la clé : 8 bits non signés, centrés sur 127,5.
            pair[0] = ((i * 127.5) + 127.5).clamp(0.0, 255.0) as u8;
            pair[1] = ((q * 127.5) + 127.5).clamp(0.0, 255.0) as u8;
            self.clock += 1;
        }
        // Le débit est volontairement libre : le moteur régule la cadence.
        Ok(out.len() & !1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::{AUDIO_RATE, DEFAULT_SAMPLE_RATE, Demodulator, Mode};

    #[test]
    fn la_source_livre_des_octets_par_paires() {
        let mut demo = Demo::new();
        let mut buffer = vec![0u8; 1_024];
        assert_eq!(demo.read(&mut buffer).unwrap(), 1_024);
        assert!(buffer.iter().any(|&b| b != 0));
    }

    #[test]
    fn le_signal_est_centre_sur_l_echelle() {
        let mut demo = Demo::new();
        let mut buffer = vec![0u8; 200_000];
        demo.read(&mut buffer).unwrap();
        let moyenne: f64 = buffer.iter().map(|&b| f64::from(b)).sum::<f64>() / buffer.len() as f64;
        assert!(
            (moyenne - 127.5).abs() < 6.0,
            "signal décentré : moyenne {moyenne:.1}"
        );
    }

    #[test]
    fn la_demonstration_s_entend_apres_demodulation() {
        // Bout en bout : la station de démonstration doit produire un audio
        // audible une fois accordée, décalage compris.
        let mut demo = Demo::new();
        let mut raw = vec![0u8; (DEFAULT_SAMPLE_RATE as usize / 2) * 2];
        demo.read(&mut raw).unwrap();

        let mut iq = Vec::new();
        super::super::to_iq(&raw, &mut iq);

        let mut demod = Demodulator::new(Mode::Usb, 2_700.0, DEFAULT_SAMPLE_RATE);
        demod.set_offset(OFFSET_HZ);
        let mut audio = Vec::new();
        demod.process(&iq, &mut audio);

        assert!(audio.len() > AUDIO_RATE as usize / 4);
        let energie: f32 = audio.iter().map(|s| s * s).sum::<f32>() / audio.len() as f32;
        assert!(energie > 1e-4, "audio muet : énergie {energie:e}");
    }

    #[test]
    fn le_manipulateur_morse_s_abaisse_et_se_releve() {
        // Le premier symbole de « V » (...-) est un point : manipulateur
        // abaissé au début, relevé juste après.
        assert!(morse_on(CALL, 0.5));
        assert!(!morse_on(CALL, 1.5));
    }
}
