//! Écran « Décodeur Morse » : la cascade en bandeau, le texte en grand.

use egui::{Align, Layout, RichText};

use super::style;
use crate::engine::Command;
use crate::theme;

pub fn show(app: &mut super::App, ui: &mut egui::Ui) {
    let Some(status) = app.state.status() else {
        style::caption(ui, "Aucune station en écoute");
        ui.add_space(24.0);
        ui.label(
            RichText::new("Le décodeur suit la station écoutée : choisissez-en une.")
                .size(15.0)
                .color(theme::TEXT.egui()),
        );
        ui.add_space(16.0);
        if style::bouton(ui, "Ouvrir le catalogue", true).clicked() {
            app.page = super::Page::Catalogue;
        }
        return;
    };

    let station = app
        .state
        .session
        .as_ref()
        .map(|s| s.station.clone())
        .unwrap_or_else(|| "libre".to_string());
    let mut entête = vec![station, status.mode.label().to_string()];
    if status.morse_enabled {
        entête.push(format!("tonalité suivie {:.0} Hz", status.morse_pitch_hz));
        if status.morse_wpm > 0.0 {
            entête.push(format!("{:.0} mots/min", status.morse_wpm));
        }
        if app.state.settings.cut_numbers {
            entête.push("chiffres abrégés".to_string());
        }
    } else {
        entête.push("décodage arrêté".to_string());
    }
    style::caption(ui, &entête.join(" · "));

    ui.add_space(12.0);
    app.waterfall.show(ui, 200.0);

    ui.add_space(14.0);
    let texte = if app.state.settings.cut_numbers {
        crate::morse::cut_numbers(&status.morse_text)
    } else {
        status.morse_text.clone()
    };
    let hauteur = (ui.available_height() - 60.0).max(120.0);
    style::panel().show(ui, |ui| {
        ui.set_min_height(hauteur);
        ui.set_min_width(ui.available_width());
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if texte.trim().is_empty() {
                    ui.label(
                        RichText::new(if status.morse_enabled {
                            "En attente d'un signal…"
                        } else {
                            "Décodage arrêté."
                        })
                        .size(14.0)
                        .color(theme::MUTED.egui()),
                    );
                } else {
                    // Le texte décodé se lit comme sur un téléscripteur : grand,
                    // espacé, les groupes bien détachés.
                    // Le curseur de la maquette : un bloc plein après le dernier
                    // caractère, comme sur un téléscripteur.
                    let réponse = ui.label(
                        RichText::new(groupes(&texte))
                            .size(26.0)
                            .color(theme::TEXT.egui())
                            .extra_letter_spacing(5.0)
                            .line_height(Some(44.0)),
                    );
                    let fin = réponse.rect;
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(fin.right() + 4.0, fin.bottom() - 36.0),
                            egui::vec2(16.0, 30.0),
                        ),
                        0.0,
                        theme::ACCENT.egui(),
                    );
                }
            });
    });

    ui.add_space(12.0);
    ui.horizontal(|ui| {
        let mut actif = status.morse_enabled;
        if ui.checkbox(&mut actif, "Décoder").changed() {
            app.state.send(Command::SetMorse(actif));
        }
        let mut abrégés = app.state.settings.cut_numbers;
        if ui.checkbox(&mut abrégés, "Chiffres abrégés").changed() {
            app.state.settings.cut_numbers = abrégés;
            let _ = app.state.settings.save();
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if style::bouton(ui, "Copier", false).clicked() {
                ui.ctx().copy_text(texte.clone());
                app.state.message = "transcription copiée".into();
            }
            if style::bouton(ui, "Effacer", false).clicked() {
                app.state.send(Command::ClearMorse);
            }
        });
    });
}

/// Rend le texte lisible en lignes de cinq groupes, comme sur la maquette.
fn groupes(texte: &str) -> String {
    let mut out = String::new();
    for (index, groupe) in texte.split_whitespace().enumerate() {
        if index > 0 {
            out.push(if index % 5 == 0 { '\n' } else { ' ' });
        }
        out.push_str(groupe);
    }
    if texte.ends_with(' ') {
        out.push(' ');
    }
    out
}
