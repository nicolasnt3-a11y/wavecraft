//! Écran « Journal » : les sessions d'écoute, notées, étiquetées, exportables.

use egui::{RichText, Sense, vec2};

use super::style;
use crate::format;
use crate::logbook::{Entry, Export};
use crate::theme;

#[derive(Default)]
pub struct JournalView {
    pub search: String,
    /// Entrée dépliée pour être commentée.
    pub ouverte: Option<String>,
    pub note: String,
    pub etiquettes: String,
}

pub fn show(app: &mut super::App, ui: &mut egui::Ui) {
    // La ligne de titre de la maquette : le journal, son nombre de sessions, et
    // les trois exports écrits en capitales espacées, cliquables.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        style::caption(
            ui,
            &format!(
                "Journal d'écoute · {} session{} · export ",
                app.state.logbook.len(),
                if app.state.logbook.len() > 1 { "s" } else { "" }
            ),
        );
        let mut demandé = None;
        for (index, format) in [Export::Csv, Export::Json, Export::Markdown]
            .into_iter()
            .enumerate()
        {
            if index > 0 {
                style::caption(ui, " · ");
            }
            let lien = ui.add(
                egui::Label::new(style::caps(format.label(), 11.0, theme::TEXT))
                    .sense(Sense::click()),
            );
            if lien.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if lien.clicked() {
                demandé = Some(format);
            }
        }
        if let Some(format) = demandé {
            exporter(app, format);
        }
    });

    ui.add_space(14.0);
    ui.add(
        egui::TextEdit::singleline(&mut app.journal.search)
            .hint_text("Chercher : station, mode, étiquette, note…")
            .desired_width(ui.available_width())
            .margin(egui::Margin::symmetric(14, 10)),
    );
    let étiquettes = app.state.logbook.tags();
    if !étiquettes.is_empty() {
        ui.label(
            RichText::new(format!("Étiquettes : {}", étiquettes.join(" · ")))
                .size(11.0)
                .color(theme::MUTED.egui()),
        );
    }
    ui.add_space(12.0);

    let entrées: Vec<Entry> = app
        .state
        .logbook
        .search(&app.journal.search)
        .into_iter()
        .cloned()
        .collect();

    if entrées.is_empty() {
        ui.add_space(18.0);
        ui.label(
            RichText::new(if app.state.logbook.is_empty() {
                "Le journal est vide : chaque écoute y laissera une entrée."
            } else {
                "Aucune session ne correspond à cette recherche."
            })
            .color(theme::MUTED.egui()),
        );
        return;
    }

    entête(ui);
    let noms: std::collections::HashMap<String, String> = app
        .state
        .catalog
        .stations
        .iter()
        .map(|s| (s.id.clone(), s.name_only()))
        .collect();
    let mut modifiée: Option<Entry> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for entrée in &entrées {
                if ligne(ui, entrée, &noms).clicked() {
                    if app.journal.ouverte.as_deref() == Some(entrée.id.as_str()) {
                        app.journal.ouverte = None;
                    } else {
                        app.journal.ouverte = Some(entrée.id.clone());
                        app.journal.note = entrée.notes.clone();
                        app.journal.etiquettes = entrée.tags.join(", ");
                    }
                }
                if app.journal.ouverte.as_deref() == Some(entrée.id.as_str())
                    && let Some(nouvelle) = détail(app, ui, entrée)
                {
                    modifiée = Some(nouvelle);
                }
            }
        });

    if let Some(entrée) = modifiée {
        let résultat = app.state.logbook.update(entrée);
        app.report(résultat);
        app.state.message = "session mise à jour".into();
    }
}

fn entête(ui: &mut egui::Ui) {
    let colonnes = [
        ("Date (UTC)", 0.0),
        ("Station", 190.0),
        ("Fréq.", 400.0),
        ("Note", 540.0),
        ("Étiquettes", 640.0),
        ("Pièces", 880.0),
    ];
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    for (titre, x) in colonnes {
        style::painted_caps(
            ui,
            egui::pos2(rect.left() + x, rect.top()),
            titre,
            10.0,
            theme::MUTED,
        );
    }
    style::separator(ui);
}

fn ligne(
    ui: &mut egui::Ui,
    entrée: &Entry,
    noms: &std::collections::HashMap<String, String>,
) -> egui::Response {
    let (rect, réponse) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
    if réponse.hovered() {
        ui.painter().rect_filled(rect, 0.0, theme::SURFACE.egui());
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let y = rect.center().y;
    let colonne = |ui: &egui::Ui, x: f32, texte: String, couleur: theme::Color, taille: f32| {
        let galley = ui.ctx().fonts_mut(|f| {
            f.layout_no_wrap(
                texte,
                egui::FontId::new(taille, egui::FontFamily::Monospace),
                couleur.egui(),
            )
        });
        let hauteur = galley.size().y;
        ui.painter().galley(
            egui::pos2(rect.left() + x, y - hauteur / 2.0),
            galley,
            couleur.egui(),
        );
    };

    colonne(ui, 0.0, format::stamp(entrée.started_at), theme::TEXT_SOFT, 13.0);
    // « S28 THE BUZZER » : l'identifiant puis le nom, en demi-gras.
    let nom = noms.get(entrée.station.as_str()).cloned().unwrap_or_default();
    // Un nom à rallonge est coupé : la colonne suivante doit rester lisible.
    let nom: String = if nom.chars().count() > 16 {
        nom.chars().take(15).collect::<String>() + "…"
    } else {
        nom
    };
    let station = if nom.is_empty() {
        entrée.station.clone()
    } else {
        format!("{} {nom}", entrée.station)
    };
    let galley = ui.ctx().fonts_mut(|f| {
        f.layout_no_wrap(
            station,
            egui::FontId::new(13.0, egui::FontFamily::Name(style::MONO_BOLD.into())),
            theme::ACCENT.egui(),
        )
    });
    let h = galley.size().y;
    ui.painter().galley(
        egui::pos2(rect.left() + 190.0, y - h / 2.0),
        galley,
        theme::ACCENT.egui(),
    );
    colonne(
        ui,
        400.0,
        format::khz(entrée.frequency_khz),
        theme::TEXT_SOFT,
        13.0,
    );
    // Les cinq étoiles de la maquette, pleines jusqu'à la note donnée.
    let pleines = entrée.rating.min(5) as usize;
    for n in 0..5 {
        style::etoile(
            ui.painter(),
            egui::pos2(rect.left() + 546.0 + n as f32 * 13.0, y),
            5.5,
            n < pleines,
        );
    }
    colonne(ui, 640.0, entrée.tags.join(" · "), theme::TEXT_SOFT, 12.0);
    let pièces = match (&entrée.recording, entrée.decoded.trim().is_empty()) {
        (Some(_), false) => "WAV · TXT",
        (Some(_), true) => "WAV",
        (None, false) => "TXT",
        (None, true) => "—",
    };
    colonne(ui, 880.0, pièces.to_string(), theme::MUTED, 12.0);

    style::separator(ui);
    réponse
}

/// Panneau dépliable : note, étiquettes, transcription.
fn détail(app: &mut super::App, ui: &mut egui::Ui, entrée: &Entry) -> Option<Entry> {
    let mut résultat = None;
    style::panel().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            style::caption(ui, "Appréciation");
            ui.add_space(10.0);
            let note = entrée.rating;
            for valeur in 1..=5u8 {
                let plein = valeur <= note;
                let bouton = egui::Button::new(
                    RichText::new(valeur.to_string()).color(if plein {
                        theme::BACKGROUND.egui()
                    } else {
                        theme::TEXT.egui()
                    }),
                )
                .fill(if plein {
                    theme::ACCENT.egui()
                } else {
                    theme::BACKGROUND.egui()
                });
                if ui.add_sized(vec2(30.0, 26.0), bouton).clicked() {
                    let mut nouvelle = entrée.clone();
                    // Recliquer sur la note en cours l'efface.
                    nouvelle.rating = if note == valeur { 0 } else { valeur };
                    résultat = Some(nouvelle);
                }
            }
            ui.add_space(20.0);
            if let Some(duration) = entrée.duration() {
                ui.label(
                    RichText::new(format!(
                        "durée {}",
                        format::duration(duration.num_seconds() as f64)
                    ))
                    .size(12.0)
                    .color(theme::MUTED.egui()),
                );
            }
            if let Some(chemin) = &entrée.recording {
                ui.add_space(14.0);
                if style::bouton(ui, "Ouvrir le dossier", false).clicked() {
                    ouvrir_dossier(chemin);
                }
            }
        });

        ui.add_space(8.0);
        ui.add(
            egui::TextEdit::multiline(&mut app.journal.note)
                .hint_text("Ce qui a été entendu…")
                .desired_width(ui.available_width())
                .desired_rows(3),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut app.journal.etiquettes)
                    .hint_text("Étiquettes, séparées par des virgules")
                    .desired_width(ui.available_width() - 140.0),
            );
            if style::bouton(ui, "Enregistrer", true).clicked() {
                let mut nouvelle = entrée.clone();
                nouvelle.notes = app.journal.note.clone();
                nouvelle.tags = app
                    .journal
                    .etiquettes
                    .split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect();
                résultat = Some(nouvelle);
            }
        });

        if !entrée.decoded.trim().is_empty() {
            ui.add_space(8.0);
            style::caption(ui, "Transcription Morse");
            ui.label(
                RichText::new(entrée.decoded.clone())
                    .size(13.0)
                    .color(theme::TEXT_SOFT.egui()),
            );
        }
    });
    ui.add_space(8.0);
    résultat
}

/// Écrit l'export à côté du journal et le signale.
fn exporter(app: &mut super::App, format: Export) {
    let chemin = crate::paths::data_dir().join(format!("journal.{}", format.extension()));
    let contenu = app.state.logbook.export(format);
    match crate::paths::ensure_parent(&chemin).and_then(|()| std::fs::write(&chemin, contenu)) {
        Ok(()) => {
            app.state.message = format!("journal exporté : {}", chemin.display());
            app.error = None;
        }
        Err(err) => app.error = Some(format!("export impossible : {err}")),
    }
}

/// Ouvre le dossier d'un enregistrement dans l'explorateur du système.
pub fn ouvrir_dossier(chemin: &std::path::Path) {
    let dossier = chemin.parent().unwrap_or(chemin);
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(dossier).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(dossier).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let _ = dossier;
}
