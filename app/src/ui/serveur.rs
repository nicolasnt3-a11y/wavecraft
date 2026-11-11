//! Écran « Serveur » : ouvrir le poste d'écoute au reste du réseau.
//!
//! Le serveur partage le moteur de la fenêtre : ce qui est écouté ici s'entend
//! là-bas, et un navigateur peut changer de station. Il n'y a qu'une clé, donc
//! qu'une écoute à la fois — c'est voulu. L'écran reprend la maquette : l'adresse
//! en haut, la cascade et la liste des stations telles que les voient les
//! navigateurs, et l'action principale en dessous.

use chrono::Utc;
use egui::{RichText, Sense, vec2};

use super::style;
use crate::format;
use crate::theme;

#[derive(Default)]
pub struct ServerView {
    #[cfg(feature = "server")]
    pub handle: Option<crate::server::Handle>,
    /// Mot de passe en cours de saisie.
    pub password: String,
    pub ouvert_au_reseau: bool,
}

impl ServerView {
    pub fn is_running(&self) -> bool {
        #[cfg(feature = "server")]
        {
            self.handle.is_some()
        }
        #[cfg(not(feature = "server"))]
        {
            false
        }
    }
}

pub fn show(app: &mut super::App, ui: &mut egui::Ui) {
    #[cfg(not(feature = "server"))]
    {
        style::caption(ui, "Serveur web");
        ui.add_space(20.0);
        ui.label(
            RichText::new("Cette version a été compilée sans le serveur web.")
                .size(15.0)
                .color(theme::TEXT.egui()),
        );
        let _ = app;
    }

    #[cfg(feature = "server")]
    {
        let en_marche = app.serveur.is_running();
        let adresse = app
            .serveur
            .handle
            .as_ref()
            .map(|h| h.url())
            .unwrap_or_else(|| adresse_prevue(app));

        // L'adresse, dans son cadre, en haut de l'écran.
        style::panel().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(adresse.clone()).size(14.0).color(theme::TEXT.egui()));
        });
        ui.add_space(14.0);

        if !en_marche {
            reglages(app, ui);
            ui.add_space(14.0);
        }

        let auditeurs = app
            .serveur
            .handle
            .as_ref()
            .map(|h| h.listeners())
            .unwrap_or(0);
        style::caption(
            ui,
            &if en_marche {
                format!(
                    "Serveur web · son en direct · {auditeurs} auditeur{}",
                    if auditeurs > 1 { "s" } else { "" }
                )
            } else {
                "Serveur web · arrêté".to_string()
            },
        );
        ui.add_space(12.0);

        // Cascade à gauche, stations à droite : ce que voient les navigateurs.
        let hauteur = (ui.available_height() - 110.0).clamp(160.0, 330.0);
        let largeur_liste = 356.0;
        // Deux colonnes, chacune empilée de haut en bas : `allocate_ui` seul
        // hériterait de la disposition horizontale et alignerait les lignes
        // de la liste côte à côte.
        let vertical = egui::Layout::top_down(egui::Align::Min);
        ui.horizontal_top(|ui| {
            let largeur_cascade = ui.available_width() - largeur_liste - 20.0;
            ui.allocate_ui_with_layout(vec2(largeur_cascade, hauteur), vertical, |ui| {
                ui.set_width(largeur_cascade);
                app.waterfall.show(ui, hauteur);
            });
            ui.add_space(20.0);
            ui.allocate_ui_with_layout(vec2(largeur_liste, hauteur), vertical, |ui| {
                ui.set_width(largeur_liste);
                liste_stations(app, ui, largeur_liste, hauteur);
            });
        });

        ui.add_space(18.0);
        ui.horizontal(|ui| {
            if en_marche {
                if style::bouton_large(ui, "■ Arrêter le serveur", false, 240.0).clicked()
                    && let Some(handle) = app.serveur.handle.take()
                {
                    handle.stop();
                    app.state.message = "serveur arrêté".into();
                }
                ui.add_space(8.0);
                if style::bouton(ui, "Copier l'adresse", false).clicked() {
                    ui.ctx().copy_text(adresse.clone());
                    app.state.message = "adresse copiée".into();
                }
            } else if style::bouton_large(ui, "▶ Lancer le serveur", true, 240.0).clicked() {
                lancer(app);
            }

            ui.add_space(16.0);
            if let Some(status) = app.state.status() {
                ui.label(
                    RichText::new(format!(
                        "{} kHz · {}",
                        format::tuned(status.frequency_hz),
                        status.mode.label()
                    ))
                    .size(14.0)
                    .color(theme::TEXT.egui()),
                );
            }
        });

        ui.add_space(10.0);
        ui.label(
            RichText::new(format!(
                "Sans fenêtre : wavecraft --server --bind 0.0.0.0 --port {} — \
                 gardez l'accès sur le réseau local, passez par un VPN pour l'extérieur.",
                app.state.settings.server_port
            ))
            .size(11.0)
            .color(theme::MUTED.egui()),
        );
    }
}

/// Les premières stations, comme dans le cadre de droite de la maquette.
fn liste_stations(app: &mut super::App, ui: &mut egui::Ui, largeur: f32, hauteur: f32) {
    let now = Utc::now();
    let rows = app.state.rows(now);
    let mut demandée = None;
    // Le cadre a 12 points de marge et 1 de liseré de chaque côté.
    let intérieur = largeur - 26.0;
    style::panel().show(ui, |ui| {
        ui.set_width(intérieur);
        ui.set_min_height(hauteur - 26.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        for row in rows.iter().take(((hauteur - 26.0) / 40.0) as usize) {
            let (rect, réponse) = ui.allocate_exact_size(vec2(intérieur, 40.0), Sense::click());
            if réponse.hovered() {
                ui.painter().rect_filled(rect, 0.0, theme::SURFACE.egui());
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let y = rect.center().y - 8.0;
            style::painted_caps_font(
                ui,
                egui::pos2(rect.left() + 4.0, y),
                &row.id,
                13.0,
                theme::TEXT,
                egui::FontFamily::Name(style::MONO_BOLD.into()),
            );
            let nom: String = row.name.chars().take(12).collect();
            style::painted_caps(ui, egui::pos2(rect.left() + 92.0, y), &nom, 13.0, theme::TEXT);
            let quand = format::when(row.continuous, row.next, now)
                .replace("EN COURS · 24 H/24", "EN COURS")
                .replace("DANS ", "");
            // L'horaire est calé à droite, comme sur la maquette.
            let texte = format!("· {quand}");
            let largeur_texte = texte.chars().count() as f32 * (13.0 * 0.6 + 13.0 * 0.18);
            style::painted_caps(
                ui,
                egui::pos2(rect.right() - largeur_texte, y),
                &texte,
                13.0,
                theme::TEXT,
            );
            if réponse.clicked() {
                demandée = Some(row.id.clone());
            }
        }
    });
    if let Some(station) = demandée {
        let résultat = app.state.listen(&station);
        app.report(résultat);
        app.waterfall.clear();
    }
}

#[cfg(feature = "server")]
fn reglages(app: &mut super::App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        style::caption(ui, "Port");
        let mut port = app.state.settings.server_port;
        if ui
            .add(egui::DragValue::new(&mut port).range(1024..=65535))
            .changed()
        {
            app.state.settings.server_port = port;
            let _ = app.state.settings.save();
        }
        ui.add_space(18.0);
        style::caption(ui, "Mot de passe");
        ui.add(
            egui::TextEdit::singleline(&mut app.serveur.password)
                .password(true)
                .hint_text("facultatif")
                .desired_width(200.0),
        );
        ui.add_space(18.0);
        ui.checkbox(&mut app.serveur.ouvert_au_reseau, "Ouvrir au réseau local");
    });
    if app.serveur.ouvert_au_reseau && app.serveur.password.trim().is_empty() {
        ui.label(
            RichText::new("Sans mot de passe, tout appareil du réseau pourra piloter la clé.")
                .size(12.0)
                .color(theme::ACCENT.egui()),
        );
    }
}

/// Adresse qu'aura le serveur une fois lancé, pour l'afficher avant.
#[cfg(feature = "server")]
fn adresse_prevue(app: &super::App) -> String {
    let hôte = if app.serveur.ouvert_au_reseau {
        std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .map(|n| n.to_lowercase())
            .unwrap_or_else(|_| "0.0.0.0".to_string())
    } else {
        "127.0.0.1".to_string()
    };
    format!("http://{hôte}:{}/", app.state.settings.server_port)
}

#[cfg(feature = "server")]
fn lancer(app: &mut super::App) {
    let bind = if app.serveur.ouvert_au_reseau {
        "0.0.0.0"
    } else {
        "127.0.0.1"
    };
    let password = {
        let saisi = app.serveur.password.trim();
        if saisi.is_empty() {
            None
        } else {
            Some(saisi.to_string())
        }
    };
    let link = app.link.clone();
    match crate::server::Handle::start(link, bind, app.state.settings.server_port, password) {
        Ok(handle) => {
            app.state.message = format!("serveur en marche sur {}", handle.url());
            app.serveur.handle = Some(handle);
            app.error = None;
        }
        Err(err) => app.error = Some(format!("serveur : {err:#}")),
    }
}
