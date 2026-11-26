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

    // Raccourcis clavier (uniquement hors champ de texte).
    raccourcis_clavier(app, ui, &status);

    // Ligne de rappel : station, mode et largeur de filtre.
    let station = app
        .state
        .session
        .as_ref()
        .map(|s| s.station.clone())
        .unwrap_or_else(|| "libre".to_string());
    let (nom, presets) = app
        .state
        .catalog
        .station(&station)
        .map(|s| (s.name_only(), s.presets.clone()))
        .unwrap_or_default();
    let mut entête = vec![station.clone()];
    if !nom.is_empty() {
        entête.push(nom);
    }
    entête.push(status.mode.label().to_string());
    entête.push(format::bandwidth(status.bandwidth_hz));
    ui.horizontal(|ui| {
        style::caption(ui, &entête.join(" · "));
        // Fréquences secondaires connues pour la station en cours.
        if presets.len() > 1 {
            ui.add_space(16.0);
            for preset in presets.iter().take(5) {
                let active = (status.frequency_hz / 1_000) == preset.frequency_khz;
                let label = format!("{} kHz", format::khz(preset.frequency_khz));
                let btn = egui::Button::new(
                    RichText::new(label)
                        .size(11.0)
                        .color(if active {
                            theme::BACKGROUND.egui()
                        } else {
                            theme::TEXT_SOFT.egui()
                        }),
                )
                .fill(if active {
                    theme::ACCENT.egui()
                } else {
                    theme::BACKGROUND.egui()
                });
                if ui.add_sized(vec2(72.0, 22.0), btn).clicked() && !active {
                    let mode = Mode::parse(&preset.mode).unwrap_or(status.mode);
                    app.state.send(Command::Tune {
                        frequency_hz: preset.frequency_khz * 1_000,
                        mode,
                        bandwidth_hz: preset.bandwidth() as f32,
                    });
                }
            }
        }
    });

    // La fréquence, en très grand, avec accord fin et saisie directe à droite.
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

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Saisie directe en kHz (ex: « 6804 » ou « 4625.5 »).
            let aller = ui.add_sized(vec2(56.0, 28.0), egui::Button::new("Aller")).clicked();
            let champ = ui.add(
                egui::TextEdit::singleline(&mut app.freq_input)
                    .hint_text("kHz…")
                    .desired_width(96.0),
            );
            let valide = aller || (champ.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if valide {
                if let Some(hz) = parse_khz(&app.freq_input) {
                    let res = app.state.tune_manual(hz, status.mode, status.bandwidth_hz);
                    app.report(res);
                    app.freq_input.clear();
                } else if !app.freq_input.trim().is_empty() {
                    app.error = Some("fréquence invalide (ex. 4625 ou 6804.5)".into());
                }
            }

            ui.add_space(10.0);
            // Pas d'accord fin.
            for (libellé, delta) in [
                ("+1k", 1_000i64),
                ("+100", 100),
                ("-100", -100),
                ("-1k", -1_000),
            ] {
                if ui.add_sized(vec2(48.0, 28.0), egui::Button::new(libellé)).clicked() {
                    nudge_freq(app, &status, delta);
                }
            }
        });
    });

    ui.add_space(12.0);
    let mut reserve = if app.state.settings.expert {
        235.0
    } else {
        190.0
    };
    if app.show_antenna {
        reserve += 185.0;
    }
    let hauteur = (ui.available_height() - reserve).clamp(150.0, 430.0);
    if let Some(target_hz) = app.waterfall.show_interactive(
        ui,
        hauteur,
        status.frequency_hz,
        status.sample_rate,
        status.mode,
        status.bandwidth_hz,
    ) {
        app.state.send(Command::Tune {
            frequency_hz: target_hz,
            mode: status.mode,
            bandwidth_hz: status.bandwidth_hz,
        });
    }

    if app.show_antenna {
        ui.add_space(10.0);
        let station_id = app.state.session.as_ref().map(|s| s.station.clone());
        super::antenne::panneau(app, ui, station_id.as_deref());
    }

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
                // Changer de mode remet la largeur de filtre habituelle du mode
                // sans réaccorder la clé ni couper l'audio.
                app.state.send(Command::SetMode(choix));
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
            RichText::new(format::bandwidth(largeur))
                .size(12.0)
                .color(theme::TEXT_SOFT.egui()),
        );
        if bougé {
            app.state.send(Command::SetBandwidth(largeur));
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
            if style::bouton(ui, "Antenne", app.show_antenna).clicked() {
                app.show_antenna = !app.show_antenna;
                if app.show_antenna && app.state.settings.position.is_none() {
                    app.state.locate();
                }
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
        let mut bias_tee = app
            .state
            .status()
            .map(|s| s.bias_tee)
            .unwrap_or(false);
        if ui
            .checkbox(&mut bias_tee, "Alimentation d'antenne (bias-tee)")
            .changed()
        {
            app.state.send(crate::engine::Command::SetBiasTee(bias_tee));
            app.state.message = if bias_tee {
                "alimentation d'antenne activée".into()
            } else {
                "alimentation d'antenne coupée".into()
            };
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

    ui.add_space(16.0);
    ui.horizontal(|ui| {
        style::caption(ui, "Accord libre");
        ui.add_space(10.0);
        let champ = ui.add(
            egui::TextEdit::singleline(&mut app.freq_input)
                .hint_text("Fréquence en kHz (ex. 4625)…")
                .desired_width(220.0),
        );
        let aller = ui.add_sized(vec2(80.0, 28.0), egui::Button::new("Écouter")).clicked();
        if aller || (champ.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
            if let Some(hz) = parse_khz(&app.freq_input) {
                let res = app
                    .state
                    .tune_manual(hz, Mode::Usb, Mode::Usb.default_bandwidth());
                app.report(res);
                app.freq_input.clear();
            } else if !app.freq_input.trim().is_empty() {
                app.error = Some("fréquence invalide (ex. 4625 ou 6804.5)".into());
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

/// Décale la fréquence en cours de `delta_hz` sans perdre le mode ni le filtre.
fn nudge_freq(app: &mut super::App, status: &crate::engine::Status, delta_hz: i64) {
    let target = (i64::from(status.frequency_hz) + delta_hz).clamp(10_000, 1_700_000_000) as u32;
    app.state.send(Command::Tune {
        frequency_hz: target,
        mode: status.mode,
        bandwidth_hz: status.bandwidth_hz,
    });
}

/// Raccourcis clavier de l'écran Écoute : ←/→ accord fin (Shift = ±1 kHz),
/// R enregistrement, M décodeur Morse, Espace muet.
fn raccourcis_clavier(app: &mut super::App, ui: &egui::Ui, status: &crate::engine::Status) {
    if ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    let (left, right, shift, key_r, key_m, space) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowLeft),
            i.key_pressed(egui::Key::ArrowRight),
            i.modifiers.shift,
            i.key_pressed(egui::Key::R),
            i.key_pressed(egui::Key::M),
            i.key_pressed(egui::Key::Space),
        )
    });
    let step = if shift { 1_000 } else { 100 };
    if left {
        nudge_freq(app, status, -step);
    }
    if right {
        nudge_freq(app, status, step);
    }
    if key_r {
        app.state.toggle_recording();
    }
    if key_m {
        app.state.send(Command::SetMorse(!status.morse_enabled));
    }
    if space {
        let vol = app.state.settings.volume;
        app.volume_coupe(vol);
    }
}

/// Lit une fréquence saisie en kHz (virgule ou point acceptés) et la convertit en Hz.
pub fn parse_khz(raw: &str) -> Option<u32> {
    let cleaned = raw.trim().replace([' ', '\u{202f}'], "").replace(',', ".");
    let khz: f64 = cleaned.parse().ok()?;
    if !khz.is_finite() || !(10.0..=1_700_000.0).contains(&khz) {
        return None;
    }
    Some((khz * 1_000.0).round() as u32)
}

#[cfg(test)]
mod tests {
    use super::parse_khz;

    #[test]
    fn la_saisie_de_frequence_accepte_entiers_et_decimales() {
        assert_eq!(parse_khz("4625"), Some(4_625_000));
        assert_eq!(parse_khz("6804.5"), Some(6_804_500));
        assert_eq!(parse_khz("4 625,25"), Some(4_625_250));
        assert_eq!(parse_khz(""), None);
        assert_eq!(parse_khz("-50"), None);
        assert_eq!(parse_khz("abc"), None);
    }
}
