//! Écran « Écoute » : la station en cours, la cascade et le S-mètre.

use egui::{Align, Layout, RichText, Sense, vec2};

use super::{style, waterfall};
use crate::dsp::Mode;
use crate::engine::Command;
use crate::format;
use crate::theme;

pub fn show(app: &mut super::App, ui: &mut egui::Ui) {
    let Some(status) = app.state.status() else {
        rien_a_ecouter(app, ui);
        return;
    };

    // Ligne de rappel : station, mode et largeur de filtre.
    let station = app
        .state
        .session
        .as_ref()
        .map(|s| s.station.clone())
        .unwrap_or_else(|| "libre".to_string());
    let nom = app
        .state
        .catalog
        .station(&station)
        .map(|s| s.name_only())
        .unwrap_or_default();
    let mut entête = vec![station.clone()];
    if !nom.is_empty() {
        entête.push(nom);
    }
    entête.push(status.mode.label().to_string());
    entête.push(format!("{:.0} kHz", status.bandwidth_hz / 1_000.0));
    style::caption(ui, &entête.join(" · "));

    // La fréquence, en très grand : c'est le chiffre qu'on cherche des yeux.
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format::tuned(status.frequency_hz))
                .size(54.0)
                .color(theme::ACCENT.egui()),
        );
        ui.add_space(8.0);
        ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
            ui.add_space(12.0);
            ui.label(RichText::new("kHz").size(16.0).color(theme::MUTED.egui()));
        });
    });

    ui.add_space(14.0);
    let hauteur = (ui.available_height() - 150.0).clamp(160.0, 430.0);
    app.waterfall.show(ui, hauteur);

    ui.add_space(16.0);
    ui.horizontal(|ui| {
        waterfall::s_meter(ui, status.level_dbfs);

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let enregistre = status.recording.is_some();
            let libellé = if enregistre {
                format!("■ Arrêter ({})", format::duration(status.recorded_secs))
            } else {
                "● Enregistrer".to_string()
            };
            // Bouton inversé, comme sur la maquette : c'est l'action de l'écran.
            if style::bouton_large(ui, &libellé, !enregistre, 236.0).clicked() {
                app.state.toggle_recording();
            }
        });
    });

    ui.add_space(14.0);
    commandes(app, ui, status.mode, status.bandwidth_hz);
}

/// Mode, filtre, volume et arrêt — puis les réglages fins en mode Expert.
fn commandes(app: &mut super::App, ui: &mut egui::Ui, mode: Mode, bande: f32) {
    let frequence = app
        .state
        .status()
        .map(|s| s.frequency_hz)
        .unwrap_or_default();

    ui.horizontal(|ui| {
        for choix in [Mode::Usb, Mode::Lsb, Mode::Am, Mode::Cw, Mode::Nfm] {
            let actif = choix == mode;
            let texte = RichText::new(choix.label()).color(if actif {
                theme::BACKGROUND.egui()
            } else {
                theme::TEXT.egui()
            });
            let bouton = egui::Button::new(texte).fill(if actif {
                theme::ACCENT.egui()
            } else {
                theme::BACKGROUND.egui()
            });
            if ui.add_sized(vec2(54.0, 28.0), bouton).clicked() {
                // Changer de mode remet la largeur de filtre habituelle du mode.
                app.state.send(Command::Tune {
                    frequency_hz: frequence,
                    mode: choix,
                    bandwidth_hz: choix.default_bandwidth(),
                });
            }
        }

        ui.add_space(24.0);
        ui.label(RichText::new("Filtre").size(11.0).color(theme::MUTED.egui()));
        let mut largeur = bande;
        // La valeur est écrite à côté de la glissière : laissée à `Slider`,
        // elle part se coller au bord opposé de la fenêtre.
        ui.spacing_mut().slider_width = 240.0;
        let glissiere = egui::Slider::new(&mut largeur, 200.0..=12_000.0)
            .show_value(false)
            .step_by(100.0);
        let bougé = ui.add(glissiere).changed();
        ui.label(
            RichText::new(format!("{:.1} kHz", largeur / 1_000.0))
                .size(12.0)
                .color(theme::TEXT_SOFT.egui()),
        );
        if bougé {
            app.state.send(Command::Tune {
                frequency_hz: frequence,
                mode,
                bandwidth_hz: largeur,
            });
        }
    });

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Volume").size(11.0).color(theme::MUTED.egui()));
        let mut volume = app.state.settings.volume;
        ui.spacing_mut().slider_width = 200.0;
        let glissiere = egui::Slider::new(&mut volume, 0.0..=1.0).show_value(false);
        if ui.add(glissiere).changed() {
            app.state.set_volume(volume);
        }

        ui.add_space(20.0);
        let mut morse = app
            .state
            .status()
            .map(|s| s.morse_enabled)
            .unwrap_or(false);
        if ui.checkbox(&mut morse, "Décoder le Morse").changed() {
            app.state.send(Command::SetMorse(morse));
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if style::bouton(ui, "Arrêter", false).clicked() {
                app.state.stop();
                app.waterfall.clear();
            }
            if style::bouton(ui, "Journal", false).clicked() {
                app.page = super::Page::Journal;
            }
        });
    });

    if app.state.settings.expert {
        ui.add_space(10.0);
        expert(app, ui);
    }
}

/// Réglages réservés au mode Expert : gain, correction d'oscillateur, bias-tee.
fn expert(app: &mut super::App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        style::caption(ui, "Expert");
        ui.add_space(14.0);

        let mut automatique = app.state.settings.gain_tenths_db.is_none();
        if ui.checkbox(&mut automatique, "Gain automatique").changed() {
            app.state.settings.gain_tenths_db = if automatique { None } else { Some(300) };
            let _ = app.state.settings.save();
            app.state.send(crate::engine::Command::SetGain(
                match app.state.settings.gain_tenths_db {
                    Some(v) => crate::sdr::Gain::Manual(v),
                    None => crate::sdr::Gain::Auto,
                },
            ));
        }
        if let Some(gain) = app.state.settings.gain_tenths_db {
            let mut db = gain as f32 / 10.0;
            if ui
                .add(egui::Slider::new(&mut db, 0.0..=49.6).suffix(" dB"))
                .changed()
            {
                let tenths = (db * 10.0).round() as i32;
                app.state.settings.gain_tenths_db = Some(tenths);
                let _ = app.state.settings.save();
                app.state
                    .send(crate::engine::Command::SetGain(crate::sdr::Gain::Manual(
                        tenths,
                    )));
            }
        }

        ui.add_space(18.0);
        let mut ppm = app.state.settings.ppm;
        if ui
            .add(egui::DragValue::new(&mut ppm).range(-200..=200).suffix(" ppm"))
            .changed()
        {
            app.state.settings.ppm = ppm;
            let _ = app.state.settings.save();
            app.state.send(crate::engine::Command::SetPpm(ppm));
        }

        ui.add_space(18.0);
        let mut morse = app
            .state
            .status()
            .map(|s| s.morse_enabled)
            .unwrap_or(false);
        if ui.checkbox(&mut morse, "Décodage Morse").changed() {
            app.state.send(crate::engine::Command::SetMorse(morse));
        }
    });
}

/// Écran d'accueil de l'écoute, tant qu'aucune station n'est accordée.
fn rien_a_ecouter(app: &mut super::App, ui: &mut egui::Ui) {
    style::caption(ui, "Aucune station en écoute");
    ui.add_space(24.0);
    ui.label(
        RichText::new("Choisissez une station dans le catalogue.")
            .size(16.0)
            .color(theme::TEXT.egui()),
    );
    ui.add_space(20.0);

    // Raccourci : la première station écoutable tout de suite.
    let suggestion = app
        .state
        .rows(chrono::Utc::now())
        .into_iter()
        .find(|row| row.continuous);
    ui.horizontal(|ui| {
        if style::bouton(ui, "Ouvrir le catalogue", true).clicked() {
            app.page = super::Page::Catalogue;
        }
        if let Some(row) = suggestion {
            let libellé = format!("Écouter {} tout de suite", row.id);
            if style::bouton(ui, &libellé, false).clicked() {
                app.listen(&row.id);
            }
        }
    });

    ui.add_space(26.0);
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), (ui.available_height() - 40.0).max(80.0)),
        Sense::hover(),
    );
    ui.painter().rect_filled(rect, 0.0, theme::BACKGROUND.egui());
    style::scanlines(ui.painter(), rect);
}
