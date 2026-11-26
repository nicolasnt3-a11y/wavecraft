//! Cascade, courbe de spectre et S-mètre.
//!
//! Le spectre arrive du moteur en dB par canal. Chaque rafraîchissement ajoute
//! une ligne en haut de la cascade et pousse les anciennes vers le bas : on voit
//! donc défiler le temps, et une porteuse se lit comme un trait vertical clair.

use std::collections::VecDeque;

use egui::{
    Align2, Color32, FontFamily, FontId, Rect, Sense, Stroke, StrokeKind, TextureHandle,
    TextureOptions, pos2, vec2,
};

use crate::dsp::Mode;
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

    /// Dessine la cascade simple sur `hauteur` points (utilisé par l'assistant et Morse).
    pub fn show(&mut self, ui: &mut egui::Ui, hauteur: f32) {
        let largeur = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(vec2(largeur, hauteur), Sense::hover());
        self.paint_waterfall(ui, rect);
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

    /// Dessine la courbe de spectre + la cascade interactive (bande passante du
    /// filtre, graduation en kHz, clic pour accorder, molette pour accorder finement).
    ///
    /// Renvoie la nouvelle fréquence demandée (en Hz) en cas d'interaction.
    pub fn show_interactive(
        &mut self,
        ui: &mut egui::Ui,
        hauteur: f32,
        frequency_hz: u32,
        sample_rate: u32,
        mode: Mode,
        bandwidth_hz: f32,
    ) -> Option<u32> {
        let largeur = ui.available_width();
        let spec_h = (hauteur * 0.24).clamp(44.0, 72.0);
        let grad_h = 18.0;
        let wf_h = (hauteur - spec_h - grad_h).max(80.0);

        let (rect, response) =
            ui.allocate_exact_size(vec2(largeur, spec_h + grad_h + wf_h), Sense::click());
        let spec_rect = Rect::from_min_size(rect.min, vec2(largeur, spec_h));
        let grad_rect = Rect::from_min_size(pos2(rect.left(), spec_rect.bottom()), vec2(largeur, grad_h));
        let wf_rect = Rect::from_min_size(pos2(rect.left(), grad_rect.bottom()), vec2(largeur, wf_h));

        // Largeur spectrale visible : demi-bande d'échantillonnage (480 kHz à 960 kHz).
        let span_hz = (sample_rate.max(1) as f32) / 2.0;

        // 1. Courbe de spectre temps-réel.
        ui.painter().rect_filled(spec_rect, 0.0, theme::BACKGROUND.egui());
        if self.last.len() >= 2 {
            let n = self.last.len();
            let points: Vec<egui::Pos2> = (0..WIDTH)
                .map(|x| {
                    let level = self.level_at(&self.last, x);
                    let px = spec_rect.left() + (x as f32 / (WIDTH - 1) as f32) * spec_rect.width();
                    let py = spec_rect.bottom() - 2.0 - level * (spec_rect.height() - 6.0);
                    pos2(px, py)
                })
                .collect();
            let _ = n;
            ui.painter().add(egui::Shape::line(
                points,
                Stroke::new(1.0, theme::TEXT_SOFT.egui()),
            ));
        } else {
            super::style::scanlines(ui.painter(), spec_rect);
        }

        // 2. Graduation en kHz.
        ui.painter().rect_filled(grad_rect, 0.0, theme::PANEL.egui());
        let ticks = [-200.0f32, -100.0, 0.0, 100.0, 200.0];
        let font = FontId::new(10.0, FontFamily::Monospace);
        for offset_khz in ticks {
            let frac = 0.5 + (offset_khz * 1_000.0) / span_hz;
            if !(0.04..=0.96).contains(&frac) {
                continue;
            }
            let x = grad_rect.left() + frac * grad_rect.width();
            ui.painter().vline(
                x,
                grad_rect.top()..=grad_rect.top() + 4.0,
                Stroke::new(1.0, theme::BORDER.egui()),
            );
            let label = if offset_khz == 0.0 {
                format!("{} kHz", format::khz(frequency_hz / 1_000))
            } else {
                format!("{offset_khz:+.0}")
            };
            ui.painter().text(
                pos2(x, grad_rect.center().y + 1.0),
                Align2::CENTER_CENTER,
                label,
                font.clone(),
                if offset_khz == 0.0 {
                    theme::TEXT.egui()
                } else {
                    theme::MUTED.egui()
                },
            );
        }

        // 3. Cascade défilante.
        self.paint_waterfall(ui, wf_rect);

        // 4. Bandeau visuel de la bande passante du filtre + repère central.
        let cx = rect.center().x;
        let bw_px = ((bandwidth_hz / span_hz) * rect.width()).max(3.0);
        let (f_left, f_right) = match mode {
            Mode::Usb => (cx, (cx + bw_px).min(rect.right())),
            Mode::Lsb => ((cx - bw_px).max(rect.left()), cx),
            Mode::Am | Mode::Cw | Mode::Nfm => (
                (cx - bw_px / 2.0).max(rect.left()),
                (cx + bw_px / 2.0).min(rect.right()),
            ),
        };
        let band_color = Color32::from_rgba_unmultiplied(255, 255, 255, 26);
        ui.painter().rect_filled(
            Rect::from_min_max(pos2(f_left, spec_rect.top()), pos2(f_right, spec_rect.bottom())),
            0.0,
            band_color,
        );
        ui.painter().rect_filled(
            Rect::from_min_max(pos2(f_left, wf_rect.top()), pos2(f_right, wf_rect.bottom())),
            0.0,
            band_color,
        );
        ui.painter().vline(cx, spec_rect.y_range(), Stroke::new(1.0, theme::ACCENT.egui()));
        ui.painter().vline(cx, wf_rect.y_range(), Stroke::new(1.0, theme::ACCENT.egui()));

        // Bordures.
        ui.painter().rect_stroke(
            spec_rect,
            0.0,
            Stroke::new(1.0, theme::LINE.egui()),
            StrokeKind::Inside,
        );
        ui.painter().rect_stroke(
            wf_rect,
            0.0,
            Stroke::new(1.0, theme::LINE.egui()),
            StrokeKind::Inside,
        );

        // 5. Survol, clic (accord direct) et molette (accord fin).
        let mut requested: Option<u32> = None;
        if let Some(hover) = response.hover_pos() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            let frac = ((hover.x - rect.left()) / rect.width()).clamp(0.0, 1.0) - 0.5;
            let delta_hz = (frac * span_hz).round() as i64;
            let target_hz = (i64::from(frequency_hz) + delta_hz).clamp(10_000, 1_700_000_000) as u32;

            // Filet de visée + étiquette de fréquence sous le curseur.
            ui.painter().vline(
                hover.x,
                wf_rect.y_range(),
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 90)),
            );
            let tip = format!("{} kHz", format::tuned(target_hz));
            let tip_x = hover.x.clamp(wf_rect.left() + 56.0, wf_rect.right() - 56.0);
            ui.painter().text(
                pos2(tip_x, wf_rect.top() + 10.0),
                Align2::CENTER_CENTER,
                tip,
                FontId::new(11.0, FontFamily::Monospace),
                theme::ACCENT.egui(),
            );

            if response.clicked() {
                requested = Some(target_hz);
            }

            let scroll_y = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll_y.abs() > 1.0 {
                let shift = ui.input(|i| i.modifiers.shift);
                let step: i64 = if shift { 1_000 } else { 100 };
                let dir: i64 = if scroll_y > 0.0 { 1 } else { -1 };
                let next = (i64::from(frequency_hz) + dir * step).clamp(10_000, 1_700_000_000) as u32;
                requested = Some(next);
            }
        }
        requested
    }

    fn paint_waterfall(&mut self, ui: &mut egui::Ui, rect: Rect) {
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
