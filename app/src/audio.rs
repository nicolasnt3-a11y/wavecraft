//! Sortie audio et fil d'attente des échantillons.
//!
//! La carte son réclame des échantillons à son propre rythme, dans un fil que
//! le système lui impose ; le moteur, lui, produit par blocs. Entre les deux,
//! une file simple à verrou : la carte son ne s'y bloque jamais longtemps et,
//! si le moteur prend du retard, elle joue du silence plutôt que de hoqueter.
//!
//! En mode serveur, il n'y a pas de carte son : seule la file est utilisée, et
//! le serveur y puise pour alimenter les navigateurs connectés.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Durée de l'audio conservé d'avance. Au-delà, les échantillons les plus
/// anciens sont abandonnés : mieux vaut un saut qu'un décalage qui s'accumule.
const MAX_BUFFERED_SECONDS: f32 = 1.5;

/// File d'attente partagée entre le moteur et la sortie audio.
#[derive(Clone)]
pub struct AudioSink {
    inner: Arc<Mutex<Inner>>,
    sample_rate: u32,
}

struct Inner {
    samples: VecDeque<f32>,
    capacity: usize,
    /// Nombre d'échantillons abandonnés faute de place.
    dropped: u64,
    /// Nombre d'échantillons de silence fournis faute de signal.
    starved: u64,
    volume: f32,
}

impl AudioSink {
    pub fn new(sample_rate: u32) -> Self {
        let capacity = (sample_rate as f32 * MAX_BUFFERED_SECONDS) as usize;
        Self {
            inner: Arc::new(Mutex::new(Inner {
                samples: VecDeque::with_capacity(capacity),
                capacity,
                dropped: 0,
                starved: 0,
                volume: 0.7,
            })),
            sample_rate,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn set_volume(&self, volume: f32) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.volume = volume.clamp(0.0, 1.0);
        }
    }

    #[cfg(test)]
    pub fn volume(&self) -> f32 {
        self.inner.lock().map(|i| i.volume).unwrap_or(0.0)
    }

    /// Dépose un bloc d'audio démodulé.
    pub fn push(&self, samples: &[f32]) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        for &sample in samples {
            if inner.samples.len() >= inner.capacity {
                inner.samples.pop_front();
                inner.dropped += 1;
            }
            inner.samples.push_back(sample);
        }
    }

    /// Remplit `out` pour la carte son. Le silence comble les manques.
    #[cfg(test)]
    pub fn fill(&self, out: &mut [f32]) {
        self.fill_interleaved(out, 1);
    }

    /// Remplit `out` en recopiant chaque échantillon mono sur `channels` voies.
    /// Évite toute allocation dans le fil audio temps-réel.
    pub fn fill_interleaved(&self, out: &mut [f32], channels: usize) {
        let channels = channels.max(1);
        let Ok(mut inner) = self.inner.lock() else {
            out.fill(0.0);
            return;
        };
        let volume = inner.volume;
        for frame in out.chunks_mut(channels) {
            let sample = match inner.samples.pop_front() {
                Some(sample) => sample * volume,
                None => {
                    inner.starved += 1;
                    0.0
                }
            };
            frame.fill(sample);
        }
    }

    /// Retire jusqu'à `max` échantillons, sans silence de remplissage.
    /// Sert au serveur web, qui préfère attendre plutôt qu'envoyer du vide.
    pub fn take(&self, max: usize) -> Vec<f32> {
        let Ok(mut inner) = self.inner.lock() else {
            return Vec::new();
        };
        let count = max.min(inner.samples.len());
        inner.samples.drain(..count).collect()
    }

    /// Nombre d'échantillons en attente.
    #[cfg(test)]
    pub fn pending(&self) -> usize {
        self.inner.lock().map(|i| i.samples.len()).unwrap_or(0)
    }

    /// Échantillons abandonnés et silences comblés, pour le diagnostic.
    #[cfg(test)]
    pub fn stats(&self) -> (u64, u64) {
        self.inner
            .lock()
            .map(|i| (i.dropped, i.starved))
            .unwrap_or((0, 0))
    }

    pub fn clear(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.samples.clear();
        }
    }
}

/// Carte son locale.
#[cfg(feature = "local-audio")]
pub mod output {
    use anyhow::{Context, Result, bail};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    use super::AudioSink;

    /// Flux ouvert vers la carte son. Le son s'arrête quand il est abandonné.
    pub struct AudioOutput {
        _stream: cpal::Stream,
        device_name: String,
        sample_rate: u32,
    }

    impl AudioOutput {
        /// Ouvre la sortie par défaut et l'alimente depuis `sink`.
        pub fn open(sink: AudioSink) -> Result<Self> {
            let host = cpal::default_host();
            let device = host
                .default_output_device()
                .context("aucune sortie audio disponible")?;
            let device_name = device
                .description()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|_| "sortie audio".into());

            let wanted: cpal::SampleRate = sink.sample_rate();
            let config = device
                .supported_output_configs()
                .context("configurations audio illisibles")?
                .find(|c| {
                    c.channels() >= 1
                        && c.min_sample_rate() <= wanted
                        && c.max_sample_rate() >= wanted
                        && c.sample_format() == cpal::SampleFormat::F32
                })
                .map(|c| c.with_sample_rate(wanted));

            let Some(config) = config else {
                bail!(
                    "la sortie « {device_name} » ne gère pas {} Hz en virgule flottante",
                    sink.sample_rate()
                );
            };

            let channels = config.channels() as usize;
            let sample_rate = config.sample_rate();
            let stream = device
                .build_output_stream(
                    config.config(),
                    move |data: &mut [f32], _| {
                        // L'audio est monophonique : le même échantillon part
                        // sur toutes les voies, sans allocation dans le fil audio.
                        sink.fill_interleaved(data, channels);
                    },
                    move |err| eprintln!("sortie audio : {err}"),
                    None,
                )
                .context("ouverture de la sortie audio")?;
            stream.play().context("démarrage de la sortie audio")?;

            Ok(Self {
                _stream: stream,
                device_name,
                sample_rate,
            })
        }

        pub fn device_name(&self) -> &str {
            &self.device_name
        }

        pub fn sample_rate(&self) -> u32 {
            self.sample_rate
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::AUDIO_RATE;

    #[test]
    fn ce_qui_entre_ressort_dans_l_ordre() {
        let sink = AudioSink::new(AUDIO_RATE);
        sink.set_volume(1.0);
        sink.push(&[0.1, 0.2, 0.3]);
        let mut out = [0.0; 3];
        sink.fill(&mut out);
        assert!((out[0] - 0.1).abs() < 1e-6);
        assert!((out[2] - 0.3).abs() < 1e-6);
        assert_eq!(sink.pending(), 0);
    }

    #[test]
    fn le_volume_s_applique_a_la_sortie() {
        let sink = AudioSink::new(AUDIO_RATE);
        sink.set_volume(0.5);
        assert!((sink.volume() - 0.5).abs() < 1e-6);
        sink.push(&[1.0]);
        let mut out = [0.0; 1];
        sink.fill(&mut out);
        assert!((out[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn un_manque_de_signal_donne_du_silence_pas_un_hoquet() {
        let sink = AudioSink::new(AUDIO_RATE);
        let mut out = [1.0; 128];
        sink.fill(&mut out);
        assert!(out.iter().all(|&s| s == 0.0));
        assert_eq!(sink.stats().1, 128, "silences non comptés");
    }

    #[test]
    fn le_retard_accumule_est_abandonne() {
        // Si le moteur produit plus vite que la carte son ne consomme, la file
        // ne doit pas grandir sans fin : l'écoute resterait en retard.
        let sink = AudioSink::new(AUDIO_RATE);
        let bloc = vec![0.5; AUDIO_RATE as usize];
        sink.push(&bloc);
        sink.push(&bloc);
        sink.push(&bloc);
        assert!(
            sink.pending() <= (AUDIO_RATE as f32 * MAX_BUFFERED_SECONDS) as usize,
            "file de {} échantillons",
            sink.pending()
        );
        assert!(sink.stats().0 > 0, "aucun échantillon abandonné");
    }

    #[test]
    fn le_serveur_prend_ce_qui_est_disponible_sans_silence() {
        let sink = AudioSink::new(AUDIO_RATE);
        sink.push(&[0.1, 0.2]);
        let pris = sink.take(10);
        assert_eq!(pris.len(), 2, "pas de silence ajouté");
        assert!(sink.take(10).is_empty());
    }

    #[test]
    fn la_file_est_partagee_entre_fils() {
        let sink = AudioSink::new(AUDIO_RATE);
        let copie = sink.clone();
        let fil = std::thread::spawn(move || copie.push(&[0.25; 100]));
        fil.join().unwrap();
        assert_eq!(sink.pending(), 100);
    }

    #[test]
    fn le_remplissage_entrelace_copie_sur_toutes_les_voies() {
        let sink = AudioSink::new(AUDIO_RATE);
        sink.set_volume(1.0);
        sink.push(&[0.2, 0.8]);
        let mut stereo = [0.0; 6];
        sink.fill_interleaved(&mut stereo, 2);
        assert!((stereo[0] - 0.2).abs() < 1e-6);
        assert!((stereo[1] - 0.2).abs() < 1e-6);
        assert!((stereo[2] - 0.8).abs() < 1e-6);
        assert!((stereo[3] - 0.8).abs() < 1e-6);
        assert_eq!(stereo[4], 0.0);
        assert_eq!(stereo[5], 0.0);
    }
}
