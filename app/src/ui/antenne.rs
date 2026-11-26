//! Boussole et guide visuel d'orientation d'antenne.
//!
//! Dessiné dans la charte monochrome de Wavecraft : une flèche blanche pointe
//! vers l'émetteur, et un trait perpendiculaire montre comment tendre le fil.

use std::f32::consts::FRAC_PI_2;

use egui::{Align2, FontFamily, FontId, Pos2, RichText, Sense, Stroke, pos2, vec2};

use super::style;
use crate::antenna::Guidance;
use crate::format;
use crate::theme;

/// Dessine une boussole de `taille × taille` points.
pub fn boussole(ui: &mut egui::Ui, guidance: &Guidance, taille: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(taille, taille), Sense::hover());
    let p = ui.painter();
    let centre = rect.center();
    let rayon = taille * 0.40;

    // Cercle principal et croix cardinale discrète.
    p.circle_filled(centre, rayon, theme::BACKGROUND.egui());
    p.circle_stroke(centre, rayon, Stroke::new(1.0, theme::BORDER.egui()));
    p.line_segment(
        [pos2(centre.x - rayon, centre.y), pos2(centre.x + rayon, centre.y)],
        Stroke::new(1.0, theme::LINE.egui()),
    );
    p.line_segment(
        [pos2(centre.x, centre.y - rayon), pos2(centre.x, centre.y + rayon)],
        Stroke::new(1.0, theme::LINE.egui()),
    );

    // Repères N / E / S / O.
    let font = FontId::new(10.0, FontFamily::Monospace);
    let marge = rayon + 9.0;
    for (label, dx, dy, col) in [
        ("N", 0.0, -marge, theme::ACCENT),
        ("E", marge, 0.0, theme::MUTED),
        ("S", 0.0, marge, theme::MUTED),
        ("O", -marge, 0.0, theme::MUTED),
    ] {
        p.text(
            pos2(centre.x + dx, centre.y + dy),
            Align2::CENTER_CENTER,
            label,
            font.clone(),
            col.egui(),
        );
    }

    // Conversion azimut (0° = Nord, sens horaire) → coordonnées écran.
    let vers = |deg: f64, r: f32| -> Pos2 {
        let rad = (deg as f32).to_radians() - FRAC_PI_2;
        pos2(centre.x + r * rad.cos(), centre.y + r * rad.sin())
    };

    // 1. Axe conseillé du fil tendu (perpendiculaire à l'émetteur).
    let fil_a = vers(guidance.wire_axis_deg, rayon * 0.78);
    let fil_b = vers((guidance.wire_axis_deg + 180.0) % 360.0, rayon * 0.78);
    p.line_segment([fil_a, fil_b], Stroke::new(2.5, theme::TEXT_SOFT.egui()));
    p.circle_filled(fil_a, 3.0, theme::TEXT_SOFT.egui());
    p.circle_filled(fil_b, 3.0, theme::TEXT_SOFT.egui());

    // 2. Flèche blanche pointant vers l'émetteur.
    let pointe = vers(guidance.bearing_deg, rayon * 0.86);
    p.line_segment([centre, pointe], Stroke::new(2.0, theme::ACCENT.egui()));
    let aile_g = vers((guidance.bearing_deg + 155.0) % 360.0, 9.0);
    let aile_d = vers((guidance.bearing_deg + 205.0) % 360.0, 9.0);
    // Décaler les ailes sur la pointe.
    let vec_g = aile_g - centre;
    let vec_d = aile_d - centre;
    p.line_segment([pointe, pointe + vec_g], Stroke::new(2.0, theme::ACCENT.egui()));
    p.line_segment([pointe, pointe + vec_d], Stroke::new(2.0, theme::ACCENT.egui()));
    p.circle_filled(centre, 2.5, theme::ACCENT.egui());
}

/// Panneau complet : localisation en 1 clic puis boussole + explications simples.
pub fn panneau(app: &mut super::App, ui: &mut egui::Ui, station_id: Option<&str>) {
    style::panel().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            style::caption(ui, "Orienter votre antenne");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if app.state.is_locating() {
                    ui.label(
                        RichText::new("Localisation en cours…")
                            .size(11.0)
                            .color(theme::MUTED.egui()),
                    );
                } else {
                    let libellé = if app.state.settings.position.is_some() {
                        "Actualiser ma position"
                    } else {
                        "Me localiser automatiquement"
                    };
                    if ui.button(libellé).clicked() {
                        app.state.locate();
                    }
                }
            });
        });
        ui.add_space(8.0);

        let Some(pos) = app.state.settings.position.clone() else {
            ui.label(
                RichText::new(
                    "Cliquez sur « Me localiser automatiquement » (ou indiquez votre position \
                     dans Réglages) pour calculer la direction de l'émetteur et voir comment \
                     tendre votre antenne.",
                )
                .size(12.0)
                .color(theme::TEXT_SOFT.egui()),
            );
            return;
        };

        let Some(guide) = app.state.antenna_guidance(station_id) else {
            return;
        };

        ui.horizontal(|ui| {
            boussole(ui, &guide, 128.0);
            ui.add_space(14.0);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(format!(
                        "Depuis {} → {} ({} · {:.0}° · {} km)",
                        pos.label,
                        guide.transmitter_label,
                        guide.direction,
                        guide.bearing_deg,
                        format::khz(guide.distance_km.round() as u32)
                    ))
                    .size(13.0)
                    .color(theme::ACCENT.egui()),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("━ {}", guide.wire_tip))
                        .size(12.0)
                        .color(theme::TEXT.egui()),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!("→ {}", guide.window_tip))
                        .size(12.0)
                        .color(theme::TEXT_SOFT.egui()),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!("○ {}", guide.loop_tip))
                        .size(12.0)
                        .color(theme::MUTED.egui()),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Légende : flèche blanche = émetteur · trait gris = fil tendu")
                        .size(11.0)
                        .color(theme::MUTED.egui()),
                );
            });
        });
    });
}
