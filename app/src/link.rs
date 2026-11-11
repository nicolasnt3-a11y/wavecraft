//! Lien entre l'état de l'application et le serveur web.
//!
//! L'état ([`crate::state::AppState`]) appartient à un seul fil : celui de la
//! fenêtre, ou celui de la boucle du mode serveur. Le serveur web, lui, répond
//! à plusieurs navigateurs en même temps. Plutôt que de partager l'état sous un
//! verrou — avec le risque de bloquer l'audio pendant qu'un navigateur lit le
//! catalogue —, les deux communiquent par deux canaux à sens unique :
//!
//! * un **instantané** publié à chaque tour de boucle, que les navigateurs
//!   lisent sans jamais attendre ;
//! * une **file de demandes**, que le fil propriétaire dépile et applique.
//!
//! Le son ne passe pas par là : [`crate::audio::AudioSink`] est déjà partageable
//! et se lit directement.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, RwLock};

use crate::audio::AudioSink;
use crate::dsp::Mode;
use crate::engine::Status;
use crate::state::Row;

/// Ce qu'un navigateur peut demander.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Écouter une station du catalogue.
    Listen(String),
    /// Accord libre.
    Tune {
        frequency_hz: u32,
        mode: Mode,
        bandwidth_hz: f32,
    },
    SetMorse(bool),
    ClearMorse,
    Recording(bool),
    Volume(f32),
    Refresh,
    Stop,
}

/// Une ligne de journal, telle que le navigateur l'affiche.
#[derive(Debug, Clone, PartialEq)]
pub struct LogRow {
    pub at: String,
    pub station: String,
    pub frequency_khz: u32,
    pub rating: u8,
    pub tags: String,
}

/// Ce que les navigateurs affichent.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub status: Option<Status>,
    /// Catalogue classé par heure d'écoute.
    pub rows: Vec<Row>,
    /// Station en cours, s'il y en a une.
    pub station: Option<String>,
    /// Nom affiché de la station en cours.
    pub station_name: String,
    pub planning_age: String,
    pub message: String,
    /// Texte décodé, déjà converti en chiffres pleins si l'option est active.
    pub morse_text: String,
    /// Dernières sessions du journal.
    pub log: Vec<LogRow>,
}

/// Poignée partagée par le fil propriétaire et le serveur.
#[derive(Clone)]
pub struct Link {
    snapshot: Arc<RwLock<Snapshot>>,
    requests: Sender<Request>,
    inbox: Arc<Mutex<Receiver<Request>>>,
    audio: Arc<RwLock<Option<AudioSink>>>,
    /// Nombre de navigateurs connectés.
    listeners: Arc<Mutex<usize>>,
}

impl Default for Link {
    fn default() -> Self {
        Self::new()
    }
}

impl Link {
    pub fn new() -> Self {
        let (requests, inbox) = channel();
        Self {
            snapshot: Arc::new(RwLock::new(Snapshot::default())),
            requests,
            inbox: Arc::new(Mutex::new(inbox)),
            audio: Arc::new(RwLock::new(None)),
            listeners: Arc::new(Mutex::new(0)),
        }
    }

    /// Publie un nouvel instantané. Appelé par le fil propriétaire.
    pub fn publish(&self, snapshot: Snapshot) {
        if let Ok(mut écrit) = self.snapshot.write() {
            *écrit = snapshot;
        }
    }

    /// Dernier instantané publié.
    pub fn snapshot(&self) -> Snapshot {
        self.snapshot
            .read()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    /// Dépose une demande. Appelé par le serveur.
    pub fn request(&self, request: Request) {
        // Si le fil propriétaire s'est arrêté, la demande est simplement perdue :
        // le serveur n'a pas à s'en inquiéter, il n'y a plus rien à piloter.
        let _ = self.requests.send(request);
    }

    /// Dépile les demandes en attente. Appelé par le fil propriétaire.
    pub fn drain(&self) -> Vec<Request> {
        let mut out = Vec::new();
        let Ok(inbox) = self.inbox.lock() else {
            return out;
        };
        // Vide ou fermée, la file ne donne plus rien : on s'arrête de même.
        while let Ok(request) = inbox.try_recv() {
            out.push(request);
        }
        out
    }

    /// Publie la file audio en cours, ou la retire quand le moteur s'arrête.
    pub fn set_audio(&self, sink: Option<AudioSink>) {
        if let Ok(mut écrit) = self.audio.write() {
            *écrit = sink;
        }
    }

    pub fn audio(&self) -> Option<AudioSink> {
        self.audio.read().ok().and_then(|a| a.clone())
    }

    /// Un navigateur de plus écoute.
    pub fn joined(&self) {
        if let Ok(mut n) = self.listeners.lock() {
            *n += 1;
        }
    }

    pub fn left(&self) {
        if let Ok(mut n) = self.listeners.lock() {
            *n = n.saturating_sub(1);
        }
    }

    pub fn listeners(&self) -> usize {
        self.listeners.lock().map(|n| *n).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_demande_traverse_le_lien() {
        let lien = Link::new();
        let copie = lien.clone();
        copie.request(Request::Listen("S28".into()));
        copie.request(Request::SetMorse(true));
        assert_eq!(
            lien.drain(),
            vec![Request::Listen("S28".into()), Request::SetMorse(true)]
        );
        assert!(lien.drain().is_empty(), "la file doit être vidée");
    }

    #[test]
    fn l_instantane_est_lisible_sans_attendre() {
        let lien = Link::new();
        assert!(lien.snapshot().rows.is_empty());
        lien.publish(Snapshot {
            message: "à l'antenne".into(),
            ..Default::default()
        });
        let copie = lien.clone();
        assert_eq!(copie.snapshot().message, "à l'antenne");
    }

    #[test]
    fn les_auditeurs_se_comptent() {
        let lien = Link::new();
        lien.joined();
        lien.joined();
        assert_eq!(lien.listeners(), 2);
        lien.left();
        assert_eq!(lien.listeners(), 1);
        // Un départ de trop ne repasse pas sous zéro.
        lien.left();
        lien.left();
        assert_eq!(lien.listeners(), 0);
    }
}
