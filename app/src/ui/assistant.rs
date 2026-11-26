//! Assistant de premier lancement, en quatre écrans.
//!
//! Il mène de la clé dans le port USB à la première voix entendue. C'est le
//! seul endroit de l'application qui parle d'installation : une fois terminé,
//! il ne revient plus, sauf s'il est redemandé depuis les réglages.

use std::time::{Duration, Instant};

use egui::{RichText, Sense, vec2};

use super::{style, waterfall};
use crate::settings::SourceChoice;
use crate::theme;

/// Les quatre étapes, dans l'ordre.
const ETAPES: [&str; 4] = [
    "Brancher la clé",
    "Pilote",
    "Antenne",
    "Test d'écoute",
];

pub struct Assistant {
    pub etape: usize,
    /// Clés vues sur le bus USB, relevées à intervalles réguliers.
    cles: Vec<crate::sdr::usb::Found>,
    /// La bibliothèque arrive-t-elle à ouvrir la clé ?
    pilote_ok: bool,
    /// La clé est utilisée par le moteur : ne pas tenter de l'ouvrir.
    en_service: bool,
    dernier_examen: Option<Instant>,
    /// Ouverture d'essai en cours dans un fil secondaire : elle charge la
    /// bibliothèque et initialise le tuner, ce qui prend une demi-seconde — trop
    /// pour le fil de la fenêtre.
    essai: Option<std::sync::mpsc::Receiver<bool>>,
    /// Les clés présentes lors de la dernière ouverture d'essai : on ne
    /// recommence que si la liste change.
    cles_essayees: Vec<String>,
}

impl Default for Assistant {
    fn default() -> Self {
        Self::new()
    }
}

impl Assistant {
    pub fn new() -> Self {
        let mut assistant = Self {
            etape: 0,
            cles: Vec::new(),
            pilote_ok: false,
            en_service: false,
            dernier_examen: None,
            essai: None,
            cles_essayees: Vec::new(),
        };
        assistant.examiner();
        assistant
    }

    /// Relève ce qui est branché et lance une ouverture d'essai de la clé.
    pub fn examiner(&mut self) {
        self.cles = crate::sdr::usb::devices().unwrap_or_default();
        self.dernier_examen = Some(Instant::now());
        self.lancer_essai();
    }

    /// Ouvrir vraiment la clé est le seul test probant ; on ne le fait ni
    /// pendant qu'elle est en service, ni deux fois à la fois.
    fn lancer_essai(&mut self) {
        if self.en_service || self.essai.is_some() {
            return;
        }
        self.cles_essayees = self.cles.iter().map(|c| c.label()).collect();
        if self.cles.is_empty() {
            self.pilote_ok = false;
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(crate::sdr::rtlsdr::can_open(0).is_ok());
        });
        self.essai = Some(rx);
    }

    /// Le bus USB est relu toutes les deux secondes tant que le pilote est en
    /// question (étapes 1 et 2) : l'assistant réagit tout seul quand on branche
    /// la clé ou que le pilote change, sans jamais retenir la fenêtre.
    fn examiner_si_besoin(&mut self) {
        if let Some(rx) = &self.essai {
            match rx.try_recv() {
                Ok(ok) => {
                    self.pilote_ok = ok;
                    self.essai = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.essai = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.etape > 1 {
            return;
        }
        let vieux = self
            .dernier_examen
            .map(|t| t.elapsed() > Duration::from_secs(2))
            .unwrap_or(true);
        if !vieux {
            return;
        }
        self.cles = crate::sdr::usb::devices().unwrap_or_default();
        self.dernier_examen = Some(Instant::now());
        let actuelles: Vec<String> = self.cles.iter().map(|c| c.label()).collect();
        if actuelles != self.cles_essayees || !self.pilote_ok {
            self.lancer_essai();
        }
    }
}

pub fn show(app: &mut super::App, ui: &mut egui::Ui) {
    let Some(mut assistant) = app.assistant.take() else {
        return;
    };
    // Pendant une installation, c'est le processus élevé qui ouvre la clé :
    // la fenêtre ne doit pas la lui disputer.
    assistant.en_service =
        app.state.status().map(|s| s.running).unwrap_or(false) || app.pilote_en_cours();
    assistant.examiner_si_besoin();

    style::caption(
        ui,
        &format!(
            "Premier lancement · étape {} sur {}",
            assistant.etape + 1,
            ETAPES.len()
        ),
    );
    ui.add_space(14.0);
    progression(ui, assistant.etape);
    ui.add_space(30.0);

    match assistant.etape {
        0 => brancher(app, ui, &mut assistant),
        1 => pilote(app, ui, &mut assistant),
        2 => antenne(app, ui, &mut assistant),
        _ => test(app, ui, &mut assistant),
    }

    // L'assistant est reposé tel quel, sauf s'il vient d'être fermé.
    if app.assistant.is_none() && !assistant.fini() {
        app.assistant = Some(assistant);
    }
}

impl Assistant {
    fn fini(&self) -> bool {
        self.etape >= ETAPES.len()
    }
}

/// Les quatre segments de progression, en haut de l'écran.
fn progression(ui: &mut egui::Ui, courante: usize) {
    let largeur = ui.available_width();
    let pas = largeur / ETAPES.len() as f32;
    let (rect, _) = ui.allocate_exact_size(vec2(largeur, 46.0), Sense::hover());
    for (index, titre) in ETAPES.iter().enumerate() {
        let x = rect.left() + index as f32 * pas;
        let fait = index <= courante;
        ui.painter().rect_filled(
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), vec2(pas - 14.0, 2.0)),
            0.0,
            if fait {
                theme::ACCENT.egui()
            } else {
                theme::BORDER.egui()
            },
        );
        style::painted_caps(
            ui,
            egui::pos2(x, rect.top() + 16.0),
            &format!("{:02} {titre}", index + 1),
            11.0,
            if fait { theme::TEXT } else { theme::MUTED },
        );
    }
}

/// Titre d'étape, en très gros, comme sur la maquette.
/// Le grand titre de la maquette : condensée grasse, en capitales.
fn titre(ui: &mut egui::Ui, texte: &str) {
    ui.label(
        RichText::new(texte.to_uppercase())
            .family(egui::FontFamily::Name(style::CONDENSED_BOLD.into()))
            .size(58.0)
            .color(theme::ACCENT.egui()),
    );
    ui.add_space(10.0);
}

/// Le texte courant de l'assistant est en condensée, comme sur le site.
fn paragraphe(ui: &mut egui::Ui, texte: &str) {
    ui.label(
        RichText::new(texte)
            .family(egui::FontFamily::Proportional)
            .size(19.0)
            .color(theme::TEXT_SOFT.egui())
            .line_height(Some(32.0)),
    );
}

/// Ligne d'état : ce que l'assistant voit du matériel.
fn constat(ui: &mut egui::Ui, bon: bool, texte: &str) {
    ui.add_space(22.0);
    let origine = egui::pos2(ui.cursor().left(), ui.cursor().top());
    let couleur = if bon { theme::TEXT } else { theme::MUTED };
    if bon {
        style::coche(ui.painter(), egui::pos2(origine.x + 5.0, origine.y + 7.0), 9.0, couleur);
    } else {
        ui.painter().circle_filled(egui::pos2(origine.x + 5.0, origine.y + 7.0), 1.5, couleur.egui());
    }
    style::painted_caps(ui, egui::pos2(origine.x + 20.0, origine.y), texte, 11.0, couleur);
    ui.add_space(20.0);
}

fn brancher(app: &mut super::App, ui: &mut egui::Ui, assistant: &mut Assistant) {
    titre(ui, "Brancher la clé");
    paragraphe(
        ui,
        "Branchez la clé RTL-SDR sur un port USB, de préférence directement sur \
         l'ordinateur plutôt que sur un concentrateur. Vissez l'antenne avant de brancher.",
    );

    match assistant.cles.first() {
        Some(clé) => constat(
            ui,
            true,
            &format!(
                "clé détectée · {} · {:04X}:{:04X}",
                clé.label(),
                clé.vendor_id,
                clé.product_id
            ),
        ),
        None => constat(ui, false, "aucune clé détectée pour l'instant"),
    }

    ui.horizontal(|ui| {
        if style::bouton(ui, "Continuer", true).clicked() {
            assistant.etape = 1;
        }
        if style::bouton(ui, "Mode démo", false).clicked() {
            demo(app, assistant);
        }
    });
}

fn pilote(app: &mut super::App, ui: &mut egui::Ui, assistant: &mut Assistant) {
    titre(ui, "Installer le pilote");

    if cfg!(target_os = "windows") {
        paragraphe(
            ui,
            "Windows doit utiliser le pilote WinUSB pour la clé RTL-SDR.\n\
             Wavecraft l'installe lui-même, avec Zadig fourni dans le paquet : \
             il suffit d'accepter l'autorisation que Windows demandera.",
        );
    } else {
        paragraphe(
            ui,
            "Sous Linux, une règle udev donne l'accès à la clé sans être \
             administrateur, et le pilote de télévision numérique doit être écarté.\n\
             Le script packaging/linux/install.sh pose les deux.",
        );
    }

    if app.pilote_en_cours() {
        constat(ui, false, "installation en cours — Zadig travaille, patientez");
    } else if assistant.pilote_ok {
        constat(ui, true, "la clé répond : le pilote est en place");
    } else if assistant.cles.is_empty() {
        constat(ui, false, "aucune clé branchée");
    } else {
        constat(ui, false, "clé branchée, mais le pilote ne répond pas encore");
    }

    ui.horizontal(|ui| {
        if cfg!(target_os = "windows")
            && !app.pilote_en_cours()
            && style::bouton(ui, "Installer le pilote", !assistant.pilote_ok).clicked()
        {
            app.installer_pilote();
        }
        if !cfg!(target_os = "windows") && style::bouton(ui, "Copier la règle udev", true).clicked() {
            ui.ctx().copy_text(REGLE_UDEV.to_string());
            app.state.message = "règle udev copiée".into();
        }
        if style::bouton(ui, "Revérifier", false).clicked() {
            assistant.examiner();
        }
        if style::bouton(ui, "Mode démo", false).clicked() {
            demo(app, assistant);
        }
        if assistant.pilote_ok && style::bouton(ui, "Continuer", assistant.pilote_ok).clicked() {
            assistant.etape = 2;
        }
    });

    if !cfg!(target_os = "windows") {
        ui.add_space(16.0);
        style::panel().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                RichText::new(REGLE_UDEV)
                    .size(12.0)
                    .color(theme::TEXT_SOFT.egui()),
            );
        });
    }
}

fn antenne(app: &mut super::App, ui: &mut egui::Ui, assistant: &mut Assistant) {
    titre(ui, "Installer l'antenne");
    paragraphe(
        ui,
        "Quelques mètres de fil suffisent sur les ondes courtes. Le plus important \
         est de s'éloigner des parasites :\n\
         • dehors si possible, ou le long d'une fenêtre ;\n\
         • loin des alimentations, chargeurs, ampoules à LED et écrans ;\n\
         • le fil tendu, pas enroulé.\n\
         Les fréquences basses passent surtout la nuit.",
    );
    ui.add_space(12.0);
    super::antenne::panneau(app, ui, Some("S28"));
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        if style::bouton(ui, "Continuer", true).clicked() {
            assistant.etape = 3;
        }
        if style::bouton(ui, "Revenir", false).clicked() {
            assistant.etape = 1;
        }
    });
}

fn test(app: &mut super::App, ui: &mut egui::Ui, assistant: &mut Assistant) {
    titre(ui, "Test d'écoute");
    paragraphe(
        ui,
        "Une station qui émet sans interruption sert de mire : si elle s'entend, \
         la chaîne complète fonctionne.",
    );

    let permanente = app
        .state
        .rows(chrono::Utc::now())
        .into_iter()
        .find(|row| row.continuous);

    ui.add_space(16.0);
    match app.state.status() {
        Some(status) if status.running => {
            waterfall::s_meter(ui, status.level_dbfs);
            ui.add_space(10.0);
            app.waterfall.show(ui, 180.0);
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if style::bouton(ui, "Je l'entends : terminer", true).clicked() {
                    terminer(app, assistant);
                }
                if style::bouton(ui, "Je n'entends rien", false).clicked() {
                    app.state.message = "Vérifiez l'antenne, puis essayez une autre station \
                                          ou revenez la nuit."
                        .into();
                }
            });
        }
        _ => {
            match &permanente {
                Some(row) => {
                    constat(
                        ui,
                        false,
                        &format!("station de test : {} {}", row.id, row.name),
                    );
                    ui.horizontal(|ui| {
                        if style::bouton(ui, "Lancer le test", true).clicked() {
                            let station = row.id.clone();
                            let résultat = app.state.listen(&station);
                            app.report(résultat);
                        }
                        if style::bouton(ui, "Passer", false).clicked() {
                            terminer(app, assistant);
                        }
                    });
                }
                None => {
                    constat(ui, false, "aucune station permanente au catalogue");
                    if style::bouton(ui, "Terminer", true).clicked() {
                        terminer(app, assistant);
                    }
                }
            };
        }
    }
}

/// Bascule en mode démonstration et ferme l'assistant.
fn demo(app: &mut super::App, assistant: &mut Assistant) {
    app.state.settings.source = SourceChoice::Demo;
    app.state.stop();
    terminer(app, assistant);
    app.state.message = "mode démonstration : le signal est fabriqué par le logiciel".into();
}

/// Marque l'assistant comme terminé et ouvre le catalogue.
fn terminer(app: &mut super::App, assistant: &mut Assistant) {
    assistant.etape = ETAPES.len();
    app.state.settings.first_run = false;
    let _ = app.state.settings.save();
    app.assistant = None;
    app.page = super::Page::Catalogue;
}

/// Règle udev proposée sous Linux.
const REGLE_UDEV: &str = r#"# /etc/udev/rules.d/60-wavecraft-rtlsdr.rules
SUBSYSTEM=="usb", ATTRS{idVendor}=="0bda", ATTRS{idProduct}=="2838", MODE="0660", TAG+="uaccess"
SUBSYSTEM=="usb", ATTRS{idVendor}=="0bda", ATTRS{idProduct}=="2832", MODE="0660", TAG+="uaccess""#;
