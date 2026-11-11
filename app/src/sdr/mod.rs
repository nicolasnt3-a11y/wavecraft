//! Sources de signal : clé RTL-SDR, serveur `rtl_tcp` ou démonstration.
//!
//! Toutes les sources livrent le même flux : des échantillons I/Q entrelacés,
//! codés sur 8 bits non signés, centrés sur 127,5 — le format brut du RTL2832U.
//! Le reste de l'application ignore donc d'où vient le signal.

pub mod demo;
pub mod pilote;
pub mod rtlsdr;
pub mod rtltcp;
pub mod usb;

use anyhow::Result;

/// Réglage du gain du tuner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum Gain {
    /// Gain laissé au tuner.
    #[default]
    Auto,
    /// Gain imposé, en dixièmes de dB.
    Manual(i32),
}


/// Une source d'échantillons I/Q.
pub trait Source: Send {
    /// Nom lisible, affiché dans l'interface.
    fn name(&self) -> String;

    /// Fréquence centrale, en Hz.
    fn set_center_freq(&mut self, hz: u32) -> Result<()>;
    fn center_freq(&self) -> u32;

    /// Fréquence d'échantillonnage, en échantillons par seconde.
    fn set_sample_rate(&mut self, hz: u32) -> Result<()>;
    fn sample_rate(&self) -> u32;

    fn set_gain(&mut self, gain: Gain) -> Result<()>;

    /// Gains possibles du tuner, en dixièmes de dB. Vide si le gain n'est pas
    /// réglable.
    fn gains(&self) -> Vec<i32> {
        Vec::new()
    }

    /// Correction d'oscillateur, en parties par million.
    fn set_ppm(&mut self, _ppm: i32) -> Result<()> {
        Ok(())
    }

    /// Alimentation d'antenne (bias-tee), pour un préamplificateur.
    fn set_bias_tee(&mut self, _on: bool) -> Result<()> {
        Ok(())
    }

    /// Remplit `out` d'échantillons I/Q et renvoie le nombre d'octets écrits.
    /// Appel bloquant.
    fn read(&mut self, out: &mut [u8]) -> Result<usize>;
}

/// Convertit un échantillon brut du RTL2832U en valeur centrée sur zéro.
#[inline]
pub fn to_float(sample: u8) -> f32 {
    (f32::from(sample) - 127.5) / 127.5
}

/// Convertit un bloc d'octets I/Q en paires (I, Q) flottantes.
pub fn to_iq(raw: &[u8], out: &mut Vec<(f32, f32)>) {
    out.clear();
    out.reserve(raw.len() / 2);
    for pair in raw.chunks_exact(2) {
        out.push((to_float(pair[0]), to_float(pair[1])));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_centre_de_l_echelle_vaut_zero() {
        // Le RTL2832U livre des octets non signés centrés sur 127,5.
        assert!(to_float(127).abs() < 0.01);
        assert!(to_float(128).abs() < 0.01);
        assert!((to_float(255) - 1.0).abs() < 0.01);
        assert!((to_float(0) + 1.0).abs() < 0.01);
    }

    #[test]
    fn les_octets_se_regroupent_en_paires_iq() {
        let mut out = Vec::new();
        to_iq(&[255, 0, 128, 127], &mut out);
        assert_eq!(out.len(), 2);
        assert!((out[0].0 - 1.0).abs() < 0.01);
        assert!((out[0].1 + 1.0).abs() < 0.01);
        assert!(out[1].0.abs() < 0.01);
    }

    #[test]
    fn un_octet_orphelin_est_ignore() {
        let mut out = Vec::new();
        to_iq(&[10, 20, 30], &mut out);
        assert_eq!(out.len(), 1);
    }
}
