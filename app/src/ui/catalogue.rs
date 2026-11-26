//! Écran « Catalogue » : les stations, classées par heure d'écoute.
//!
//! Un clic sur une ligne applique le préréglage et ouvre l'écoute. L'ordre est
//! celui de la maquette : ce qui passe maintenant, puis ce qui vient bientôt.

use chrono::Utc;
use egui::{RichText, Sense, StrokeKind, vec2};

use super::style;
use crate::format;
use crate::state::Row;
use crate::theme;

#[derive(Default)]
pub struct Catalogue {
    pub search: String,
}

pub fn show(app: &mut super::App, ui: &mut egui::Ui) {
    let now = Utc::now();
    let rows = app.state.rows(now);
    let filtre = app.catalogue.search.trim().to_lowercase();
    let visibles: Vec<Row> = rows
        .into_iter()
        .filter(|row| {
            filtre.is_empty()
                || row.id.to_lowercase().contains(&filtre)
                || row.name.to_lowercase().contains(&filtre)
                || row
                    .frequency_khz
                    .map(|f| f.to_string().contains(&filtre))
                    .unwrap_or(false)
        })
        .collect();

    style::caption(
        ui,
        &format!(
            "{} stations · triées par heure d'écoute",
            app.state.catalog.listenable().len()
        ),
    );
    ui.add_space(16.0);

    let champ = egui::TextEdit::singleline(&mut app.catalogue.search)
        .hint_text("Chercher une station…")
        .desired_width(ui.available_width())
        .margin(egui::Margin::symmetric(14, 10));
    ui.add(champ);
    ui.add_space(10.0);

    // Alerte d'émission imminente ou récapitulatif des enregistrements programmés.
    if let Some(alerte) = app.state.alert.clone() {
        style::panel().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                RichText::new(format!("● {alerte}"))
                    .size(12.0)
                    .color(theme::ACCENT.egui()),
            );
        });
        ui.add_space(8.0);
    } else if !app.state.scheduled.is_empty() {
        let resume: Vec<String> = app
            .state
            .scheduled
            .iter()
            .take(3)
            .map(|s| format!("{} à {} UTC", s.station, s.start.format("%H:%M")))
            .collect();
        style::caption(
            ui,
            &format!(
                "{} enregistrement(s) programmé(s) : {}",
                app.state.scheduled.len(),
                resume.join(" · ")
            ),
        );
        ui.add_space(8.0);
    }
    ui.add_space(4.0);

    if visibles.is_empty() {
        ui.add_space(20.0);
        ui.label(
            RichText::new("Aucune station ne correspond à cette recherche.")
                .color(theme::MUTED.egui()),
        );
        return;
    }

    let mut demandée: Option<String> = None;
    let mut signet: Option<String> = None;
    let mut programme: Option<(String, chrono::DateTime<Utc>)> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for row in &visibles {
                let est_prog = row
                    .next
                    .map(|at| app.state.is_scheduled(&row.id, at))
                    .unwrap_or(false);
                match ligne(ui, row, now, est_prog) {
                    Clic::Ecouter => demandée = Some(row.id.clone()),
                    Clic::Signet => signet = Some(row.id.clone()),
                    Clic::Programmer(at) => programme = Some((row.id.clone(), at)),
                    Clic::Rien => {}
                }
            }
        });

    if let Some(station) = signet {
        app.state.toggle_bookmark(&station);
    }
    if let Some((station, at)) = programme {
        app.state
            .toggle_schedule(&station, at, crate::state::DEFAULT_SCHEDULE_MINS);
    }
    if let Some(station) = demandée {
        app.listen(&station);
    }
}

enum Clic {
    Rien,
    Ecouter,
    Signet,
    Programmer(chrono::DateTime<Utc>),
}

/// Une ligne de station, dessinée à la main pour tenir la maquette.
fn ligne(
    ui: &mut egui::Ui,
    row: &Row,
    now: chrono::DateTime<Utc>,
    est_prog: bool,
) -> Clic {
    let hauteur = 74.0;
    let (rect, réponse) =
        ui.allocate_exact_size(vec2(ui.available_width(), hauteur), Sense::click());
    let en_cours = row.continuous
        || row
            .next
            .map(|at| at <= now + chrono::TimeDelta::minutes(1))
            .unwrap_or(false);

    let peintre = ui.painter();
    if en_cours {
        peintre.rect_filled(rect, 0.0, theme::SURFACE.egui());
    }
    if réponse.hovered() {
        peintre.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, theme::BORDER.egui()),
            StrokeKind::Inside,
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let milieu = rect.center().y;
    let gauche = rect.left() + 18.0;

    // Identifiant, en grand.
    let identifiant = ui.ctx().fonts_mut(|f| {
        f.layout_no_wrap(
            row.id.clone(),
            egui::FontId::new(21.0, egui::FontFamily::Monospace),
            theme::TEXT.egui(),
        )
    });
    let hauteur_id = identifiant.size().y;
    ui.painter().galley(
        egui::pos2(gauche, milieu - hauteur_id / 2.0),
        identifiant,
        theme::TEXT.egui(),
    );

    // Nom de la station, en capitales espacées. Les noms à rallonge sont
    // coupés : la colonne suivante doit rester lisible.
    if !row.name.is_empty() {
        let nom: String = if row.name.chars().count() > 30 {
            row.name.chars().take(29).collect::<String>() + "…"
        } else {
            row.name.clone()
        };
        style::painted_caps_font(
            ui,
            egui::pos2(gauche + 122.0, milieu - 8.0),
            &nom,
            14.0,
            theme::ACCENT,
            egui::FontFamily::Name(style::MONO_BOLD.into()),
        );
    }

    // Fréquence et mode.
    let fréquence = match row.frequency_khz {
        Some(khz) => format!("{} kHz", format::khz(khz)),
        None => "—".to_string(),
    };
    texte(ui, egui::pos2(gauche + 462.0, milieu), &fréquence, theme::TEXT_SOFT, 13.0);
    texte(
        ui,
        egui::pos2(gauche + 616.0, milieu),
        &row.mode.to_uppercase(),
        theme::TEXT_SOFT,
        12.0,
    );

    // Horaire, aligné à droite.
    let horaire = format::when(row.continuous, row.next, now);
    let couleur = if en_cours { theme::ACCENT } else { theme::MUTED };
    let largeur = mesure(ui, &horaire, 11.0);
    style::painted_caps(
        ui,
        egui::pos2(rect.right() - 18.0 - largeur, milieu - 7.0),
        &horaire,
        11.0,
        couleur,
    );

    // Signet : une étoile discrète, à gauche de l'identifiant.
    let étoile = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 6.0, milieu),
        vec2(14.0, 14.0),
    );
    let signet = ui.interact(
        étoile,
        réponse.id.with("signet"),
        Sense::click(),
    );
    if row.bookmarked || signet.hovered() {
        ui.painter().text(
            étoile.center(),
            egui::Align2::CENTER_CENTER,
            "★",
            egui::FontId::new(12.0, egui::FontFamily::Proportional),
            if row.bookmarked {
                theme::ACCENT.egui()
            } else {
                theme::DECOR.egui()
            },
        );
    }

    // Bouton discret de programmation d'enregistrement pour les stations à horaire.
    let mut clic_prog = None;
    if !row.continuous
        && let Some(at) = row.next
    {
        let prog_rect = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 18.0 - largeur - 44.0, milieu),
            vec2(58.0, 22.0),
        );
        let prog_resp = ui.interact(prog_rect, réponse.id.with("prog"), Sense::click());
        let fond = if est_prog {
            theme::ACCENT
        } else if prog_resp.hovered() {
            theme::SURFACE
        } else {
            theme::BACKGROUND
        };
        let avant = if est_prog {
            theme::BACKGROUND
        } else {
            theme::MUTED
        };
        ui.painter().rect_filled(prog_rect, 0.0, fond.egui());
        ui.painter().rect_stroke(
            prog_rect,
            0.0,
            egui::Stroke::new(1.0, if est_prog { theme::ACCENT } else { theme::BORDER }.egui()),
            StrokeKind::Inside,
        );
        ui.painter().text(
            prog_rect.center(),
            egui::Align2::CENTER_CENTER,
            if est_prog { "● PROG" } else { "PROG" },
            egui::FontId::new(10.0, egui::FontFamily::Monospace),
            avant.egui(),
        );
        if prog_resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            prog_resp
                .clone()
                .on_hover_text("Programmer / annuler l'enregistrement automatique (15 min)");
        }
        if prog_resp.clicked() {
            clic_prog = Some(at);
        }
    }

    style::separator(ui);

    if signet.clicked() {
        Clic::Signet
    } else if let Some(at) = clic_prog {
        Clic::Programmer(at)
    } else if réponse.clicked() {
        Clic::Ecouter
    } else {
        Clic::Rien
    }
}

fn texte(ui: &egui::Ui, at: egui::Pos2, texte: &str, couleur: theme::Color, taille: f32) {
    let galley = ui.ctx().fonts_mut(|f| {
        f.layout_no_wrap(
            texte.to_string(),
            egui::FontId::new(taille, egui::FontFamily::Monospace),
            couleur.egui(),
        )
    });
    let hauteur = galley.size().y;
    ui.painter().galley(
        egui::pos2(at.x, at.y - hauteur / 2.0),
        galley,
        couleur.egui(),
    );
}

/// Largeur d'un texte en capitales espacées, pour l'aligner à droite.
fn mesure(ui: &egui::Ui, texte: &str, taille: f32) -> f32 {
    let galley = ui.ctx().fonts_mut(|f| {
        f.layout_no_wrap(
            texte.to_uppercase(),
            egui::FontId::new(taille, egui::FontFamily::Monospace),
            theme::TEXT.egui(),
        )
    });
    galley.size().x + texte.chars().count() as f32 * taille * 0.18
}
