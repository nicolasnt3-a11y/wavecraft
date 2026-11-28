//! Interface bureau.
//!
//! La fenêtre suit les maquettes du site : un bandeau de 48 points en haut, un
//! rail de navigation de 200 points à gauche avec un repère blanc sur l'entrée
//! active, et un seul écran à la fois au centre. Rien n'y est décoratif : tout
//! ce qui est affiché vient du moteur, du catalogue ou du journal.

mod antenne;
mod assistant;
mod captures;
mod catalogue;
mod ecoute;
mod journal;
mod morse;
mod reglages;
mod serveur;
mod style;
mod waterfall;

use std::time::Duration;

use anyhow::{Result, anyhow};
use chrono::Utc;

use crate::format;
use crate::state::AppState;
use crate::theme;
use waterfall::Waterfall;

/// Les six écrans du rail de navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Catalogue,
    Ecoute,
    Morse,
    Journal,
    Serveur,
    Reglages,
}

impl Page {
    const ALL: [Page; 6] = [
        Page::Catalogue,
        Page::Ecoute,
        Page::Morse,
        Page::Journal,
        Page::Serveur,
        Page::Reglages,
    ];

    /// Libellé dans le rail.
    fn label(self) -> &'static str {
        match self {
            Page::Catalogue => "Catalogue",
            Page::Ecoute => "Écoute",
            Page::Morse => "Morse",
            Page::Journal => "Journal",
            Page::Serveur => "Serveur",
            Page::Reglages => "Réglages",
        }
    }

    /// Titre rappelé dans le bandeau du haut.
    fn title(self) -> &'static str {
        match self {
            Page::Morse => "Décodeur Morse",
            Page::Serveur => "Serveur",
            autre => autre.label(),
        }
    }
}

/// Charge l'icône de la fenêtre depuis l'image PNG embarquée.
fn load_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../../assets/icon.png");
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    Some(egui::IconData {
        rgba: buf,
        width: info.width,
        height: info.height,
    })
}

/// Ouvre la fenêtre et rend la main quand elle se ferme.
pub fn run(demo: bool, captures: Option<std::path::PathBuf>) -> Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Wavecraft")
        .with_app_id("wavecraft")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([960.0, 640.0]);

    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Wavecraft",
        options,
        Box::new(move |cc| {
            style::apply(&cc.egui_ctx);
            let app = App::new(demo, captures)?;
            Ok(Box::new(app))
        }),
    )
    .map_err(|err| anyhow!("ouverture de la fenêtre : {err}"))
}

pub struct App {
    pub state: AppState,
    pub page: Page,
    pub waterfall: Waterfall,
    /// Sortie audio locale, rouverte quand le moteur redémarre.
    #[cfg(feature = "local-audio")]
    output: Option<crate::audio::output::AudioOutput>,
    audio_generation: u64,
    /// Assistant de premier lancement, tant qu'il n'est pas terminé.
    pub assistant: Option<assistant::Assistant>,
    pub catalogue: catalogue::Catalogue,
    pub journal: journal::JournalView,
    pub serveur: serveur::ServerView,
    /// Saisie de fréquence libre en kHz sur l'écran Écoute.
    pub freq_input: String,
    /// Affiche le panneau d'aide à l'orientation d'antenne sur l'écran Écoute.
    pub show_antenna: bool,
    /// Lien avec le serveur web, créé au premier lancement de celui-ci.
    #[cfg(feature = "server")]
    pub link: crate::link::Link,
    /// Dernière erreur à montrer à l'utilisateur.
    pub error: Option<String>,
    /// Installation du pilote en cours dans un fil secondaire ; son résultat
    /// arrive par ce canal.
    pilote: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    /// Niveau retenu quand le son est coupé depuis le bandeau.
    volume_avant_coupure: f32,
    /// Campagne de captures en cours, le cas échéant.
    captures: Option<captures::Captures>,
    /// Mise à jour disponible détectée sur GitHub.
    pub update_available: Option<crate::updater::ReleaseInfo>,
    update_check: Option<std::sync::mpsc::Receiver<Result<Option<crate::updater::ReleaseInfo>, String>>>,
    update_install: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    update_manual: bool,
}

impl App {
    fn new(
        demo: bool,
        captures: Option<std::path::PathBuf>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let state = AppState::new(demo).map_err(|e| e.to_string())?;
        // L'assistant ne s'ouvre qu'au tout premier lancement ; ensuite il reste
        // accessible depuis les réglages.
        let assistant = (state.settings.first_run && !demo).then(assistant::Assistant::new);
        let captures = captures.map(captures::Captures::new);
        let mut app = Self {
            page: if assistant.is_some() {
                Page::Reglages
            } else {
                Page::Catalogue
            },
            state,
            waterfall: Waterfall::default(),
            #[cfg(feature = "local-audio")]
            output: None,
            audio_generation: 0,
            assistant,
            catalogue: catalogue::Catalogue::default(),
            journal: journal::JournalView::default(),
            serveur: serveur::ServerView::default(),
            freq_input: String::new(),
            show_antenna: false,
            #[cfg(feature = "server")]
            link: crate::link::Link::new(),
            error: None,
            pilote: None,
            volume_avant_coupure: 0.7,
            captures,
            update_available: None,
            update_check: None,
            update_install: None,
            update_manual: false,
        };
        if app.state.settings.app_updates && app.captures.is_none() {
            app.check_updates(false);
        }
        Ok(app)
    }

    /// Vérifie sur GitHub si une version plus récente est publiée.
    pub fn check_updates(&mut self, manual: bool) {
        if self.update_check.is_some() || self.update_install.is_some() {
            return;
        }
        self.update_manual = manual;
        if manual {
            self.state.message = "vérification des mises à jour sur GitHub…".into();
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(crate::updater::check_latest().map_err(|e| format!("{e:#}")));
        });
        self.update_check = Some(rx);
    }

    /// Lance le téléchargement et l'installation de la mise à jour disponible.
    pub fn install_update(&mut self) {
        if self.update_install.is_some() {
            return;
        }
        let Some(release) = self.update_available.clone() else {
            return;
        };
        self.state.message = format!("téléchargement de Wavecraft {}…", release.tag);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(crate::updater::download_and_install(&release).map_err(|e| format!("{e:#}")));
        });
        self.update_install = Some(rx);
    }

    pub fn update_checking(&self) -> bool {
        self.update_check.is_some()
    }

    pub fn update_installing(&self) -> bool {
        self.update_install.is_some()
    }

    fn suivre_mise_a_jour(&mut self, ui: &egui::Ui) {
        if let Some(rx) = &self.update_check {
            match rx.try_recv() {
                Ok(Ok(Some(release))) => {
                    self.update_check = None;
                    self.state.message = format!("nouvelle version disponible : {}", release.tag);
                    self.update_available = Some(release);
                }
                Ok(Ok(None)) => {
                    self.update_check = None;
                    if self.update_manual {
                        self.state.message = format!(
                            "Wavecraft {} est déjà à jour",
                            crate::updater::CURRENT_VERSION
                        );
                    }
                }
                Ok(Err(err)) => {
                    self.update_check = None;
                    if self.update_manual {
                        self.error = Some(format!("mise à jour : {err}"));
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.update_check = None,
            }
        }

        if let Some(rx) = &self.update_install {
            match rx.try_recv() {
                Ok(Ok(())) => {
                    self.update_install = None;
                    self.state.stop();
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Ok(Err(err)) => {
                    self.update_install = None;
                    self.error = Some(format!("installation de la mise à jour : {err}"));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.update_install = None,
            }
        }
    }

    fn update_banner(&mut self, ui: &mut egui::Ui) {
        if self.captures.is_some() {
            return;
        }
        let Some(release) = self.update_available.clone() else {
            return;
        };
        let installing = self.update_installing();
        egui::Panel::top("bandeau-maj")
            .exact_size(40.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(theme::SURFACE.egui())
                    .stroke(egui::Stroke::new(1.0, theme::BORDER.egui()))
                    .inner_margin(egui::Margin::symmetric(20, 0)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "● Nouvelle version disponible : Wavecraft {} (actuelle : {})",
                            release.tag,
                            crate::updater::CURRENT_VERSION
                        ))
                        .size(12.0)
                        .color(theme::ACCENT.egui()),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !installing {
                            if ui.button("Plus tard").clicked() {
                                self.update_available = None;
                            }
                            if !release.page_url.is_empty() {
                                ui.hyperlink_to("Notes", &release.page_url);
                            }
                            if release.download_url.is_some()
                                && ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new("INSTALLER ET REDÉMARRER")
                                                .size(11.0)
                                                .color(theme::BACKGROUND.egui()),
                                        )
                                        .fill(theme::ACCENT.egui()),
                                    )
                                    .clicked()
                            {
                                self.install_update();
                            }
                        } else {
                            ui.label(
                                egui::RichText::new("Téléchargement et installation en cours…")
                                    .size(12.0)
                                    .color(theme::TEXT_SOFT.egui()),
                            );
                        }
                    });
                });
            });
    }

    /// Ouvre la sortie audio, ou la referme si le moteur est arrêté.
    fn sync_audio(&mut self) {
        // Les captures n'ont pas besoin de son, et une machine sans carte son ne
        // doit pas les salir d'un message d'erreur.
        if self.captures.is_some() {
            return;
        }
        #[cfg(feature = "local-audio")]
        {
            let generation = self.state.generation();
            match self.state.audio() {
                Some(sink) if self.output.is_none() || generation != self.audio_generation => {
                    match crate::audio::output::AudioOutput::open(sink) {
                        Ok(sortie) => {
                            self.output = Some(sortie);
                            self.audio_generation = generation;
                        }
                        Err(err) => {
                            self.error = Some(format!("sortie audio : {err:#}"));
                            self.audio_generation = generation;
                        }
                    }
                }
                None => {
                    self.output = None;
                }
                _ => {}
            }
        }
    }

    /// Déroule la campagne de captures, écran par écran.
    ///
    /// Chaque écran est affiché quelques secondes — le temps que la cascade se
    /// remplisse et que la démonstration ait émis du Morse — puis photographié.
    fn captures(&mut self, ui: &mut egui::Ui) {
        let Some(campagne) = &self.captures else {
            return;
        };
        if campagne.fini() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // L'écran courant est imposé, et la démonstration tourne dès le début.
        match campagne.etape() {
            Some(captures::Etape::Ecran(page)) => {
                self.assistant = None;
                self.page = page;
            }
            Some(captures::Etape::Assistant) => {
                if self.assistant.is_none() {
                    self.assistant = Some(assistant::Assistant::new());
                }
            }
            None => return,
        }

        // La démonstration écoute les stations permanentes l'une après l'autre.
        let rang = match self.state.status() {
            None => Some(0),
            Some(_) => self.captures.as_ref().and_then(|c| c.station_suivante()),
        };
        if let Some(rang) = rang {
            let permanentes: Vec<String> = self
                .state
                .rows(Utc::now())
                .into_iter()
                .filter(|row| row.continuous)
                .map(|row| row.id)
                .collect();
            if !permanentes.is_empty() {
                let station = permanentes[rang % permanentes.len()].clone();
                let résultat = self.state.listen(&station);
                self.report(résultat);
                self.state.send(crate::engine::Command::SetMorse(true));
                self.waterfall.clear();
            }
        }

        let Some(campagne) = &mut self.captures else {
            return;
        };
        let Some(nom) = campagne.declencher() else {
            return;
        };
        let chemin = campagne.dossier().join(format!("{nom}.png"));
        ui.ctx().request_screenshot(move |image| {
            match captures::ecrire(&image, &chemin) {
                Ok(()) => println!("capture : {}", chemin.display()),
                Err(err) => eprintln!("capture {} : {err}", chemin.display()),
            }
        });
    }

    /// Sortie audio en service : « Haut-parleurs (Realtek) · 48 000 Hz ».
    pub fn sortie_audio(&self) -> Option<String> {
        #[cfg(feature = "local-audio")]
        {
            self.output
                .as_ref()
                .map(|o| format!("{} · {} Hz", o.device_name(), o.sample_rate()))
        }
        #[cfg(not(feature = "local-audio"))]
        {
            None
        }
    }

    /// Lance l'installation du pilote WinUSB dans un fil secondaire. Windows
    /// demande son autorisation ; le résultat revient dans le bandeau du bas.
    pub fn installer_pilote(&mut self) {
        if self.pilote.is_some() {
            return;
        }
        // La clé ne doit pas être tenue par le moteur pendant que le pilote change.
        self.state.stop();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(crate::sdr::pilote::demander().map_err(|e| format!("{e:#}")));
        });
        self.pilote = Some(rx);
        self.error = None;
        self.state.message = "installation du pilote : acceptez l'autorisation de Windows".into();
    }

    /// Coupe le son d'un clic, et le rétablit au niveau d'avant.
    pub fn volume_coupe(&mut self, actuel: f32) {
        if actuel > 0.0 {
            self.volume_avant_coupure = actuel;
            self.state.set_volume(0.0);
        } else {
            let retour = if self.volume_avant_coupure > 0.0 {
                self.volume_avant_coupure
            } else {
                0.7
            };
            self.state.set_volume(retour);
        }
    }

    pub fn pilote_en_cours(&self) -> bool {
        self.pilote.is_some()
    }

    /// Relève le résultat de l'installation du pilote, s'il est arrivé.
    fn suivre_pilote(&mut self) {
        let Some(rx) = &self.pilote else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(())) => {
                self.pilote = None;
                self.state.message = "pilote installé : la clé répond".into();
                if let Some(assistant) = self.assistant.as_mut() {
                    assistant.examiner();
                }
            }
            Ok(Err(err)) => {
                self.pilote = None;
                self.error = Some(format!("pilote : {err}"));
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.pilote = None,
        }
    }

    /// Signale une erreur dans le bandeau du bas.
    pub fn report(&mut self, résultat: Result<()>) {
        match résultat {
            Ok(()) => self.error = None,
            Err(err) => self.error = Some(format!("{err:#}")),
        }
    }

    /// Écoute une station et bascule sur l'écran d'écoute.
    pub fn listen(&mut self, station: &str) {
        let résultat = self.state.listen(station);
        let bon = résultat.is_ok();
        self.report(résultat);
        if bon {
            self.waterfall.clear();
            self.page = Page::Ecoute;
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("bandeau")
            .exact_size(style::HEADER_HEIGHT)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(theme::PANEL.egui())
                    .inner_margin(egui::Margin::symmetric(20, 0)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    marque(ui);
                    ui.add_space(14.0);
                    let titre = match &self.assistant {
                        Some(_) => "Assistant".to_string(),
                        None => self.page.title().to_string(),
                    };
                    style::painted_caps(
                        ui,
                        egui::pos2(ui.cursor().left(), ui.cursor().center().y - 8.0),
                        &titre,
                        12.0,
                        theme::MUTED,
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format::clock_line(Utc::now()))
                                .size(12.0)
                                .color(theme::TEXT_SOFT.egui())
                                .extra_letter_spacing(2.0),
                        );
                        // Le volume, à portée de main sur tous les écrans — y
                        // compris pendant le test d'écoute de l'assistant.
                        ui.add_space(26.0);
                        let mut volume = self.state.settings.volume;
                        ui.label(
                            egui::RichText::new(format!("{:>3.0} %", volume * 100.0))
                                .size(12.0)
                                .color(theme::TEXT_SOFT.egui()),
                        );
                        ui.spacing_mut().slider_width = 110.0;
                        let glissière = egui::Slider::new(&mut volume, 0.0..=1.0).show_value(false);
                        if ui.add(glissière).on_hover_text("Volume").changed() {
                            self.state.set_volume(volume);
                        }
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new(if volume > 0.0 { "SON" } else { "MUET" })
                                    .size(11.0)
                                    .extra_letter_spacing(2.0),
                            ))
                            .on_hover_text("Couper ou rétablir le son")
                            .clicked()
                        {
                            self.volume_coupe(volume);
                        }
                    });
                });
            });
    }

    fn rail(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("rail")
            .exact_size(style::RAIL_WIDTH)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(theme::BACKGROUND.egui())
                    .inner_margin(egui::Margin {
                        left: 0,
                        right: 0,
                        top: 56,
                        bottom: 10,
                    }),
            )
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for page in Page::ALL {
                    let actif = self.assistant.is_none() && self.page == page;
                    if style::rail_item(ui, page.label(), actif).clicked() {
                        self.page = page;
                        self.assistant = None;
                    }
                }

                // Pied du rail : licence et origine des données, comme sur le site.
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.add_space(28.0);
                        ui.label(
                            egui::RichText::new("Priyom.org · GPL-3.0")
                                .size(10.0)
                                .color(theme::DECOR.egui()),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(28.0);
                        ui.hyperlink_to(
                            egui::RichText::new("nthill.be/wavecraft")
                                .size(10.0)
                                .color(theme::DECOR.egui()),
                            "https://nthill.be/wavecraft/",
                        );
                    });
                });
            });
    }

    fn footer(&mut self, ui: &mut egui::Ui) {
        let message = match (&self.error, self.state.message.is_empty()) {
            (Some(err), _) => err.clone(),
            (None, false) => self.state.message.clone(),
            (None, true) => String::new(),
        };
        if message.is_empty() {
            return;
        }
        egui::Panel::bottom("message")
            .exact_size(34.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(theme::PANEL.egui())
                    .inner_margin(egui::Margin::symmetric(20, 0)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let couleur = if self.error.is_some() {
                        theme::ACCENT
                    } else {
                        theme::TEXT_SOFT
                    };
                    ui.label(egui::RichText::new(message).size(12.0).color(couleur.egui()));
                    if self.error.is_some() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Fermer").clicked() {
                                self.error = None;
                            }
                        });
                    }
                });
            });
    }
}

/// La marque encadrée du site : un pylône en traits fins, puis « WAVECRAFT ».
fn marque(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
    let peintre = ui.painter();
    peintre.rect_filled(rect, 0.0, theme::BACKGROUND.egui());
    peintre.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, theme::BORDER.egui()),
        egui::StrokeKind::Inside,
    );
    let trait_ = egui::Stroke::new(1.0, theme::TEXT.egui());
    let centre = rect.center().x;
    // Le pylône : un mât et deux haubans.
    peintre.vline(centre, rect.top() + 5.0..=rect.bottom() - 3.0, trait_);
    peintre.hline(centre - 4.0..=centre + 4.0, rect.top() + 9.0, trait_);
    peintre.hline(centre - 3.0..=centre + 3.0, rect.top() + 13.0, trait_);

    ui.add_space(14.0);
    style::painted_caps_font(
        ui,
        egui::pos2(ui.cursor().left(), ui.cursor().center().y - 9.0),
        "Wavecraft",
        14.0,
        theme::ACCENT,
        egui::FontFamily::Name(style::MONO_BOLD.into()),
    );
    ui.add_space(118.0);
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.state.tick();
        if self.state.needs_refresh(Utc::now()) {
            self.state.refresh();
        }
        if let Some(status) = self.state.status() {
            self.waterfall.push(&status.spectrum);
            if let Some(err) = &status.error {
                self.error = Some(err.clone());
            }
        }
        self.sync_audio();
        self.suivre_pilote();
        self.suivre_mise_a_jour(ui);

        // Quand le serveur web tourne, il voit le même état que la fenêtre et
        // ses demandes sont appliquées ici, dans le fil propriétaire.
        #[cfg(feature = "server")]
        if self.serveur.is_running() {
            self.state.apply_requests(&self.link);
            self.state.publish(&self.link);
        }

        self.captures(ui);

        self.header(ui);
        self.update_banner(ui);
        self.rail(ui);
        self.footer(ui);

        egui::CentralPanel::no_frame()
            .frame(
                egui::Frame::NONE
                    .fill(theme::BACKGROUND.egui())
                    .inner_margin(egui::Margin {
                        left: 32,
                        right: 24,
                        top: 36,
                        bottom: 16,
                    }),
            )
            .show(ui, |ui| {
                if self.assistant.is_some() {
                    assistant::show(self, ui);
                    return;
                }
                match self.page {
                    Page::Catalogue => catalogue::show(self, ui),
                    Page::Ecoute => ecoute::show(self, ui),
                    Page::Morse => morse::show(self, ui),
                    Page::Journal => journal::show(self, ui),
                    Page::Serveur => serveur::show(self, ui),
                    Page::Reglages => reglages::show(self, ui),
                }
            });

        // Le niveau, la cascade et les décomptes bougent sans action de
        // l'utilisateur : la fenêtre se redessine dix fois par seconde.
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    fn on_exit(&mut self) {
        self.state.stop();
    }
}
