//! Écran « Réglages ».
//!
//! Deux niveaux : Débutant, qui ne montre que ce qui se décide sans jargon, et
//! Expert, qui ouvre le gain, la correction d'oscillateur et l'alimentation
//! d'antenne. Le reste — dossiers, actualisation, assistant — est commun.

use egui::{RichText, vec2};

use super::style;
use crate::settings::SourceChoice;
use crate::theme;

pub fn show(app: &mut super::App, ui: &mut egui::Ui) {
    style::caption(ui, "Réglages");
    ui.add_space(16.0);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            niveau(app, ui);
            ui.add_space(18.0);
            source(app, ui);
            ui.add_space(18.0);
            ecoute(app, ui);
            ui.add_space(18.0);
            actualisation(app, ui);
            ui.add_space(18.0);
            dossiers(app, ui);
            ui.add_space(18.0);
            a_propos(app, ui);
        });
}

fn niveau(app: &mut super::App, ui: &mut egui::Ui) {
    style::panel().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        style::caption(ui, "Niveau");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            for (libellé, expert) in [("Débutant", false), ("Expert", true)] {
                let actif = app.state.settings.expert == expert;
                let bouton = egui::Button::new(RichText::new(libellé).color(if actif {
                    theme::BACKGROUND.egui()
                } else {
                    theme::TEXT.egui()
                }))
                .fill(if actif {
                    theme::ACCENT.egui()
                } else {
                    theme::BACKGROUND.egui()
                });
                if ui.add_sized(vec2(140.0, 30.0), bouton).clicked() {
                    app.state.settings.expert = expert;
                    let _ = app.state.settings.save();
                }
            }
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new(if app.state.settings.expert {
                "Gain, correction d'oscillateur et alimentation d'antenne sont accessibles."
            } else {
                "Seul l'essentiel est affiché ; les réglages fins restent automatiques."
            })
            .size(12.0)
            .color(theme::MUTED.egui()),
        );
    });
}

fn source(app: &mut super::App, ui: &mut egui::Ui) {
    style::panel().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        style::caption(ui, "Source du signal");
        ui.add_space(8.0);

        let mut choix = app.state.settings.source;
        ui.horizontal(|ui| {
            for valeur in [SourceChoice::Rtl, SourceChoice::Remote, SourceChoice::Demo] {
                if ui
                    .radio_value(&mut choix, valeur, valeur.label())
                    .clicked()
                {
                    app.state.settings.source = valeur;
                    let _ = app.state.settings.save();
                    // La source ne change qu'au prochain démarrage du moteur.
                    app.state.stop();
                    app.waterfall.clear();
                }
            }
        });

        if cfg!(target_os = "windows") && app.state.settings.source == SourceChoice::Rtl {
            ui.add_space(8.0);
            if app.pilote_en_cours() {
                ui.label(
                    RichText::new("Installation du pilote en cours…")
                        .size(12.0)
                        .color(theme::MUTED.egui()),
                );
            } else if style::bouton(ui, "Installer le pilote WinUSB", false).clicked() {
                app.installer_pilote();
            }
        }

        if app.state.settings.source == SourceChoice::Remote {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                style::caption(ui, "Machine");
                let mut hôte = app.state.settings.remote_host.clone();
                if ui
                    .add(egui::TextEdit::singleline(&mut hôte).desired_width(220.0))
                    .changed()
                {
                    app.state.settings.remote_host = hôte;
                    let _ = app.state.settings.save();
                }
                let mut port = app.state.settings.remote_port;
                if ui
                    .add(egui::DragValue::new(&mut port).range(1..=65535))
                    .changed()
                {
                    app.state.settings.remote_port = port;
                    let _ = app.state.settings.save();
                }
            });
            ui.label(
                RichText::new("Une autre machine doit y faire tourner rtl_tcp.")
                    .size(12.0)
                    .color(theme::MUTED.egui()),
            );
        }

        if app.state.settings.expert && app.state.settings.source == SourceChoice::Rtl {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                style::caption(ui, "Clé n°");
                let mut index = app.state.settings.device_index;
                if ui
                    .add(egui::DragValue::new(&mut index).range(0..=7))
                    .changed()
                {
                    app.state.settings.device_index = index;
                    let _ = app.state.settings.save();
                }
                ui.add_space(18.0);
                style::caption(ui, "Correction");
                let mut ppm = app.state.settings.ppm;
                if ui
                    .add(
                        egui::DragValue::new(&mut ppm)
                            .range(-200..=200)
                            .suffix(" ppm"),
                    )
                    .changed()
                {
                    app.state.settings.ppm = ppm;
                    let _ = app.state.settings.save();
                    app.state.send(crate::engine::Command::SetPpm(ppm));
                }
                ui.add_space(18.0);
                if ui.button("Alimentation d'antenne : marche").clicked() {
                    app.state.send(crate::engine::Command::SetBiasTee(true));
                    app.state.message = "alimentation d'antenne activée".into();
                }
                if ui.button("arrêt").clicked() {
                    app.state.send(crate::engine::Command::SetBiasTee(false));
                    app.state.message = "alimentation d'antenne coupée".into();
                }
            });
            ui.label(
                RichText::new(
                    "L'alimentation d'antenne envoie du courant dans le câble : \
                     ne l'activez que pour un ampli ou un filtre qui l'attend.",
                )
                .size(12.0)
                .color(theme::MUTED.egui()),
            );
        }
    });
}

fn ecoute(app: &mut super::App, ui: &mut egui::Ui) {
    style::panel().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        style::caption(ui, "Écoute");
        ui.add_space(8.0);

        ui.label(
            RichText::new(match app.sortie_audio() {
                Some(sortie) => format!("Sortie audio : {sortie}"),
                None => "Sortie audio : ouverte au démarrage de l'écoute".to_string(),
            })
            .size(12.0)
            .color(theme::MUTED.egui()),
        );
        ui.add_space(6.0);

        let mut auto = app.state.settings.morse_auto;
        if ui
            .checkbox(&mut auto, "Décoder le Morse d'office sur les stations en CW")
            .changed()
        {
            app.state.settings.morse_auto = auto;
            let _ = app.state.settings.save();
        }
        let mut abrégés = app.state.settings.cut_numbers;
        if ui
            .checkbox(&mut abrégés, "Traduire les chiffres abrégés des opérateurs")
            .changed()
        {
            app.state.settings.cut_numbers = abrégés;
            let _ = app.state.settings.save();
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            style::caption(ui, "Volume");
            let mut volume = app.state.settings.volume;
            if ui
                .add(egui::Slider::new(&mut volume, 0.0..=1.0).show_value(false))
                .changed()
            {
                app.state.set_volume(volume);
                let _ = app.state.settings.save();
            }
            ui.label(
                RichText::new(format!("{:.0} %", app.state.settings.volume * 100.0))
                    .size(12.0)
                    .color(theme::MUTED.egui()),
            );
        });
    });
}

fn actualisation(app: &mut super::App, ui: &mut egui::Ui) {
    style::panel().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        style::caption(ui, "Planning et catalogue");
        ui.add_space(8.0);

        let mut auto = app.state.settings.auto_refresh;
        if ui
            .checkbox(&mut auto, "Relire le planning Priyom automatiquement")
            .changed()
        {
            app.state.settings.auto_refresh = auto;
            let _ = app.state.settings.save();
        }
        let mut catalogue = app.state.settings.catalog_updates;
        if ui
            .checkbox(&mut catalogue, "Vérifier qu'un catalogue plus récent existe")
            .changed()
        {
            app.state.settings.catalog_updates = catalogue;
            let _ = app.state.settings.save();
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let âge = app.state.planning_age();
            ui.label(
                RichText::new(format!(
                    "Planning : {âge} · catalogue version {} ({} stations)",
                    app.state.catalog.version,
                    app.state.catalog.stations.len()
                ))
                .size(12.0)
                .color(theme::MUTED.egui()),
            );
            ui.add_space(12.0);
            if style::bouton(ui, "Actualiser maintenant", false).clicked() {
                app.state.refresh();
            }
        });
        if app.state.is_refreshing() {
            ui.label(
                RichText::new("Actualisation en cours…")
                    .size(12.0)
                    .color(theme::TEXT_SOFT.egui()),
            );
        }
    });
}

fn dossiers(app: &mut super::App, ui: &mut egui::Ui) {
    style::panel().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        style::caption(ui, "Dossiers");
        ui.add_space(8.0);
        for (libellé, chemin) in [
            ("Réglages", crate::settings::Settings::path()),
            ("Journal", crate::logbook::Logbook::path()),
            ("Enregistrements", app.state.settings.recordings()),
        ] {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{libellé} : {}", chemin.display()))
                        .size(12.0)
                        .color(theme::TEXT_SOFT.egui()),
                );
            });
        }
        ui.add_space(6.0);
        if style::bouton(ui, "Ouvrir le dossier des enregistrements", false).clicked() {
            let dossier = app.state.settings.recordings();
            let _ = std::fs::create_dir_all(&dossier);
            super::journal::ouvrir_dossier(&dossier.join("x"));
        }
    });
}

fn a_propos(app: &mut super::App, ui: &mut egui::Ui) {
    style::panel().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        style::caption(ui, "À propos");
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!("Wavecraft {}", env!("CARGO_PKG_VERSION")))
                .size(13.0)
                .color(theme::TEXT.egui()),
        );
        ui.label(
            RichText::new(
                "Logiciel libre sous licence GPL-3.0. Données des stations et calendrier : \
                 Priyom.org (CC BY-NC-SA 4.0). Projet indépendant, sans affiliation.",
            )
            .size(12.0)
            .color(theme::MUTED.egui()),
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.hyperlink_to("Mode d'emploi", "https://nthill.be/wavecraft/guide/");
            ui.hyperlink_to("Code source", "https://github.com/nicolasnt3-a11y/wavecraft");
            if style::bouton(ui, "Relancer l'assistant", false).clicked() {
                app.assistant = Some(super::assistant::Assistant::new());
            }
        });
    });
}
