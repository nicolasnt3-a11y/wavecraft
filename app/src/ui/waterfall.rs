//! Cascade, courbe de spectre et S-mètre.
//!
//! Le spectre arrive du moteur en dB par canal. Chaque rafraîchissement ajoute
//! une ligne en haut de la cascade et pousse les anciennes vers le bas : on voit
//! donc défiler le temps, et une porteuse se lit comme un trait vertical clair.

use std::collections::VecDeque;

use egui::{Color32, Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, pos2, vec2};

use crate::format;
use crate::theme;

/// Nombre de lignes conservées : à dix images par seconde, environ une minute.
const HISTORY: usize = 600;

/// Largeur de l'image de la cascade, en points.
const WIDTH: usize = 512;

/// Nombre de cases du S-mètre, comme sur la maquette.
const METER_CELLS: usize = 28;

pub struct Waterfall {
    lines: VecDeque<Vec<Color32>>,
    texture: Option<TextureHandle>,
    /// Plancher et plafond de l'échelle, en dB.
    pub floor_db: f32,
    pub ceiling_db: f32,
    /// Dernier spectre reçu, pour la courbe.
    last: Vec<f32>,
}

impl Default for Waterfall {
    fn default() -> Self {
        Self {
            lines: VecDeque::with_capacity(HISTORY),
            texture: None,
            floor_db: -110.0,
            ceiling_db: -20.0,
            last: Vec::new(),
        }
    }
}

impl Waterfall {
    /// Ajoute un spectre en haut de la cascade.
    pub fn push(&mut self, spectrum: &[f32]) {
        if spectrum.is_empty() {
            return;
        }
        self.last = spectrum.to_vec();
        self.recaler(spectrum);

        // Le spectre a plus de canaux que l'image n'a de points : chaque point
        // garde le plus fort de sa tranche, pour qu'une porteuse étroite ne
        // disparaisse pas au passage.
        let mut ligne = Vec::with_capacity(WIDTH);
        for x in 0..WIDTH {
            let couleur = theme::waterfall_color(self.level_at(spectrum, x));
            ligne.push(Color32::from_rgb(couleur.r, couleur.g, couleur.b));
        }

        self.lines.push_front(ligne);
        self.lines.truncate(HISTORY);
    }

    /// Recale l'échelle sur ce qui est réellement reçu.
    ///
    /// Le plancher suit le bruit de fond, le plafond le signal le plus fort :
    /// la cascade garde du contraste que l'antenne soit sourde ou saturée, sans
    /// réglage à la main. Le déplacement est lent, pour que l'image ne
    /// « respire » pas à chaque bouffée de parasites.
    fn recaler(&mut self, spectrum: &[f32]) {
        let mut tri: Vec<f32> = spectrum.iter().copied().filter(|v| v.is_finite()).collect();
        if tri.len() < 8 {
            return;
        }
        let rang = |tri: &mut Vec<f32>, part: f32| -> f32 {
            let index = ((tri.len() - 1) as f32 * part) as usize;
            *tri.select_nth_unstable_by(index, f32::total_cmp).1
        };
        // Le plancher est posé juste au-dessus du bruit médian : le fond reste
        // noir et seuls les signaux s'éclaircissent, comme sur un récepteur.
        let bas = rang(&mut tri, 0.5) + 2.0;
        let haut = rang(&mut tri, 0.995) + 8.0;
        if haut - bas < 12.0 {
            return;
        }
        self.floor_db += (bas - self.floor_db) * 0.05;
        self.ceiling_db += (haut - self.ceiling_db) * 0.05;
    }

    /// Niveau de 0 à 1 du point `x` de l'image.
    fn level_at(&self, spectrum: &[f32], x: usize) -> f32 {
        let debut = x * spectrum.len() / WIDTH;
        let fin = ((x + 1) * spectrum.len() / WIDTH).max(debut + 1).min(spectrum.len());
        let db = spectrum[debut..fin]
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max);
        theme::normalize_db(db, self.floor_db, self.ceiling_db)
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.last.clear();
        self.texture = None;
    }

    /// Dessine la cascade sur `hauteur` points.
    pub fn show(&mut self, ui: &mut egui::Ui, hauteur: f32) {
        let largeur = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(vec2(largeur, hauteur), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, theme::BACKGROUND.egui());

        if !self.lines.is_empty() {
            let lignes = self.lines.len();
            let mut pixels = Vec::with_capacity(WIDTH * lignes);
            for ligne in &self.lines {
                pixels.extend_from_slice(ligne);
            }
            let image = egui::ColorImage::new([WIDTH, lignes], pixels);
            match &mut self.texture {
                Some(texture) => texture.set(image, TextureOptions::NEAREST),
                None => {
                    self.texture =
                        Some(ui.ctx().load_texture("cascade", image, TextureOptions::NEAREST));
                }
            }
            if let Some(texture) = &self.texture {
                ui.painter().image(
                    texture.id(),
                    rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        } else {
            super::style::scanlines(ui.painter(), rect);
        }

        // Repère central : la station écoutée est toujours au milieu.
        ui.painter().vline(
            rect.center().x,
            rect.y_range(),
            Stroke::new(1.0, theme::ACCENT.egui()),
        );
        ui.painter().rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, theme::LINE.egui()),
            StrokeKind::Inside,
        );
    }

}

/// S-mètre à cases, suivi de sa graduation : « S9 +10 ».
pub fn s_meter(ui: &mut egui::Ui, dbfs: f32) {
    ui.horizontal(|ui| {
        super::style::caption(ui, "S-mètre");
        ui.add_space(14.0);

        let largeur = 420.0_f32.min(ui.available_width() - 120.0).max(120.0);
        let (rect, _) = ui.allocate_exact_size(vec2(largeur, 14.0), Sense::hover());
        // L'échelle couvre S0 à S9+30, soit la plage utile d'une clé non étalonnée.
        let plein = theme::normalize_db(
            dbfs,
            format::S9_DBFS - 9.0 * format::S_STEP_DB,
            format::S9_DBFS + 30.0,
        );
        let cases_allumées = (plein * METER_CELLS as f32).round() as usize;
        let pas = rect.width() / METER_CELLS as f32;
        for index in 0..METER_CELLS {
            let case = Rect::from_min_size(
                pos2(rect.left() + index as f32 * pas, rect.top()),
                vec2(pas - 3.0, rect.height()),
            );
            let couleur = if index < cases_allumées {
                theme::ACCENT
            } else {
                theme::BORDER
            };
            ui.painter().rect_filled(case, 0.0, couleur.egui());
        }

        ui.add_space(14.0);
        ui.label(
            egui::RichText::new(format::s_meter(dbfs))
                .size(13.0)
                .color(theme::TEXT.egui()),
        );
    });
}
