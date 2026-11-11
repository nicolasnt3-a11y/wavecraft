//! Enregistrement de l'audio démodulé, au format WAV.
//!
//! Une station de nombres ne se réécoute pas : le message passe une fois. Tout
//! ce qui est écouté peut donc être enregistré, pour être repris au calme —
//! c'est le geste de base de l'écoute d'ondes courtes.
//!
//! Le fichier est écrit au fil de l'eau : une coupure de courant laisse un WAV
//! tronqué mais lisible, jamais un fichier perdu.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

/// Nom de fichier d'un enregistrement : station, fréquence et horodatage, de
/// quoi s'y retrouver des mois plus tard.
pub fn file_name(station: &str, frequency_khz: u32, at: DateTime<Utc>) -> String {
    let station: String = station
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!(
        "{}_{station}_{frequency_khz}kHz.wav",
        at.format("%Y%m%d-%H%M%S")
    )
}

/// Enregistreur en cours d'écriture.
pub struct Recorder {
    writer: hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    path: PathBuf,
    samples: u64,
    sample_rate: u32,
}

impl Recorder {
    /// Ouvre un enregistrement dans `dir`.
    pub fn start(
        dir: &Path,
        station: &str,
        frequency_khz: u32,
        sample_rate: u32,
    ) -> Result<Self> {
        let started_at = Utc::now();
        let path = dir.join(file_name(station, frequency_khz, started_at));
        std::fs::create_dir_all(dir)
            .with_context(|| format!("création du dossier d'enregistrement ({})", dir.display()))?;

        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let writer = hound::WavWriter::create(&path, spec)
            .with_context(|| format!("création du fichier ({})", path.display()))?;

        Ok(Self {
            writer,
            path,
            samples: 0,
            sample_rate,
        })
    }

    /// Ajoute un bloc d'audio.
    pub fn write(&mut self, audio: &[f32]) -> Result<()> {
        for &sample in audio {
            // Saturation franche plutôt que repliement : un dépassement doit
            // s'entendre comme une saturation, pas comme un craquement.
            let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
            self.writer.write_sample(value)?;
        }
        self.samples += audio.len() as u64;
        Ok(())
    }

    /// Durée enregistrée jusqu'ici.
    pub fn duration_secs(&self) -> f64 {
        self.samples as f64 / f64::from(self.sample_rate)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Referme le fichier et renvoie son chemin et sa durée.
    pub fn finish(self) -> Result<(PathBuf, f64)> {
        let duration = self.duration_secs();
        let path = self.path.clone();
        self.writer
            .finalize()
            .with_context(|| format!("fermeture du fichier ({})", path.display()))?;
        Ok((path, duration))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dossier(nom: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wavecraft-test-rec-{nom}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn le_nom_de_fichier_porte_station_frequence_et_date() {
        let at = Utc.with_ymd_and_hms(2026, 11, 11, 14, 5, 9).unwrap();
        assert_eq!(
            file_name("S28", 4625, at),
            "20261111-140509_S28_4625kHz.wav"
        );
    }

    #[test]
    fn un_nom_de_station_exotique_donne_un_fichier_valide() {
        let at = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
        let nom = file_name("The Pip / S30", 5448, at);
        assert!(!nom.contains('/'), "{nom}");
        assert!(nom.ends_with(".wav"));
    }

    #[test]
    fn l_audio_enregistre_se_relit() {
        let dir = dossier("relecture");
        let mut rec = Recorder::start(&dir, "S28", 4625, 48_000).unwrap();
        let bloc: Vec<f32> = (0..4_800).map(|n| (n as f32 / 100.0).sin() * 0.5).collect();
        rec.write(&bloc).unwrap();
        assert!((rec.duration_secs() - 0.1).abs() < 1e-6);

        let (path, duree) = rec.finish().unwrap();
        assert!(path.exists());
        assert!((duree - 0.1).abs() < 1e-6);

        let lecteur = hound::WavReader::open(&path).unwrap();
        assert_eq!(lecteur.spec().sample_rate, 48_000);
        assert_eq!(lecteur.spec().channels, 1);
        assert_eq!(lecteur.len(), 4_800);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn un_depassement_sature_sans_se_replier() {
        let dir = dossier("saturation");
        let mut rec = Recorder::start(&dir, "test", 1, 48_000).unwrap();
        rec.write(&[2.0, -2.0]).unwrap();
        let (path, _) = rec.finish().unwrap();

        let mut lecteur = hound::WavReader::open(&path).unwrap();
        let echantillons: Vec<i16> = lecteur.samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(echantillons[0], i16::MAX);
        assert!(echantillons[1] <= -i16::MAX + 1, "{}", echantillons[1]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn le_dossier_manquant_est_cree() {
        let dir = dossier("cree").join("sous-dossier");
        let rec = Recorder::start(&dir, "S30", 5448, 48_000).unwrap();
        assert!(dir.exists());
        let (path, _) = rec.finish().unwrap();
        assert!(path.starts_with(&dir));
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }
}
