//! Charte graphique, reprise telle quelle du site.
//!
//! Les valeurs viennent de `site/assets/css/style.css`, qui reprend lui-même la
//! charte de l'archive The Conet Project sur nthill.be/sw. Le même vocabulaire
//! sert à la fenêtre et au navigateur : les couleurs sont définies une seule
//! fois ici, et le serveur web les reprend dans sa feuille de style sous les
//! mêmes noms que le site. Les deux interfaces se ressemblent donc vraiment,
//! au lieu de dériver chacune de son côté.
//!
//! La charte est en niveaux de gris, sans exception : un signal fort se lit à
//! sa clarté, jamais à sa couleur.

/// Couleur, en composantes 0-255.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Gris de même valeur sur les trois composantes.
    pub const fn grey(value: u8) -> Self {
        Self::new(value, value, value)
    }

    /// Écriture hexadécimale, pour les feuilles de style.
    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    /// Mélange avec une autre couleur, `t` allant de 0 à 1.
    pub fn mix(self, other: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let blend = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t) as u8;
        Color::new(
            blend(self.r, other.r),
            blend(self.g, other.g),
            blend(self.b, other.b),
        )
    }

    #[cfg(feature = "desktop")]
    pub fn egui(self) -> egui::Color32 {
        egui::Color32::from_rgb(self.r, self.g, self.b)
    }
}

/// Fond de l'application — `--bg`.
pub const BACKGROUND: Color = Color::grey(0x05);
/// Fond d'un bandeau ou d'un panneau — `--bg-2`.
pub const PANEL: Color = Color::grey(0x0C);
/// Fond d'une ligne mise en avant — `--bg-3`.
pub const SURFACE: Color = Color::grey(0x14);
/// Texte courant — `--ink`.
pub const TEXT: Color = Color::grey(0xE4);
/// Texte secondaire — `--ink-2`.
pub const TEXT_SOFT: Color = Color::grey(0xA3);
/// Texte tertiaire, mentions discrètes — `--ink-t` (5,9:1 sur le fond).
pub const MUTED: Color = Color::grey(0x8A);
/// Filets et décors uniquement, jamais du texte — `--ink-3`.
pub const DECOR: Color = Color::grey(0x61);
/// Liseré d'un cadre — `--ink-4`.
pub const BORDER: Color = Color::grey(0x3A);
/// Séparateur discret — `--line`.
pub const LINE: Color = Color::grey(0x22);
/// Ce qui est vivant, en cours, à l'antenne — `--white`.
pub const ACCENT: Color = Color::grey(0xFF);

/// Polices du site. Elles sont servies par le site lui-même ; la fenêtre se
/// rabat sur une police à chasse fixe du système si elles manquent.
pub const FONT_MONO: &str = "IBM Plex Mono";
pub const FONT_CONDENSED: &str = "IBM Plex Sans Condensed";

/// Échelle de la cascade : du fond vers le signal le plus fort.
///
/// `level` va de 0 (plancher de bruit) à 1 (saturation). L'échelle est éclaircie
/// vers le bas pour que le bruit de fond garde du relief sans monter en gris.
pub fn waterfall_color(level: f32) -> Color {
    let level = level.clamp(0.0, 1.0);
    let clair = level.powf(0.75);
    BACKGROUND.mix(ACCENT, clair)
}

/// Convertit une puissance en dB en niveau de 0 à 1, pour la cascade.
pub fn normalize_db(db: f32, floor_db: f32, ceiling_db: f32) -> f32 {
    if ceiling_db <= floor_db {
        return 0.0;
    }
    ((db - floor_db) / (ceiling_db - floor_db)).clamp(0.0, 1.0)
}

/// Feuille de style commune, insérée telle quelle dans la page du serveur web.
///
/// Les noms sont ceux du site : une règle écrite pour l'un vaut pour l'autre.
pub fn css_variables() -> String {
    format!(
        ":root {{\n  \
         --bg: {};\n  \
         --bg-2: {};\n  \
         --bg-3: {};\n  \
         --ink: {};\n  \
         --ink-2: {};\n  \
         --ink-3: {};\n  \
         --ink-4: {};\n  \
         --ink-t: {};\n  \
         --line: {};\n  \
         --white: {};\n  \
         --mono: \"{}\", ui-monospace, \"Cascadia Mono\", Consolas, monospace;\n  \
         --sans: \"{}\", \"Arial Narrow\", system-ui, sans-serif;\n  \
         color-scheme: dark;\n}}\n",
        BACKGROUND.hex(),
        PANEL.hex(),
        SURFACE.hex(),
        TEXT.hex(),
        TEXT_SOFT.hex(),
        DECOR.hex(),
        BORDER.hex(),
        MUTED.hex(),
        LINE.hex(),
        ACCENT.hex(),
        FONT_MONO,
        FONT_CONDENSED,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_couleurs_viennent_du_site() {
        // Valeurs de `site/assets/css/style.css`. Elles ne doivent pas dériver :
        // la fenêtre, le navigateur et le site montrent le même gris.
        assert_eq!(BACKGROUND.hex(), "#050505");
        assert_eq!(PANEL.hex(), "#0C0C0C");
        assert_eq!(SURFACE.hex(), "#141414");
        assert_eq!(TEXT.hex(), "#E4E4E4");
        assert_eq!(TEXT_SOFT.hex(), "#A3A3A3");
        assert_eq!(MUTED.hex(), "#8A8A8A");
        assert_eq!(DECOR.hex(), "#616161");
        assert_eq!(BORDER.hex(), "#3A3A3A");
        assert_eq!(LINE.hex(), "#222222");
        assert_eq!(ACCENT.hex(), "#FFFFFF");
    }

    #[test]
    fn la_charte_est_en_niveaux_de_gris() {
        for couleur in [
            BACKGROUND, PANEL, SURFACE, TEXT, TEXT_SOFT, MUTED, DECOR, BORDER, LINE, ACCENT,
        ] {
            assert_eq!(
                (couleur.r, couleur.g),
                (couleur.g, couleur.b),
                "{} n'est pas un gris",
                couleur.hex()
            );
        }
    }

    #[test]
    fn le_melange_va_d_une_couleur_a_l_autre() {
        let noir = Color::grey(0);
        let blanc = Color::grey(255);
        assert_eq!(noir.mix(blanc, 0.0), noir);
        assert_eq!(noir.mix(blanc, 1.0), blanc);
        let milieu = noir.mix(blanc, 0.5);
        assert!((milieu.r as i32 - 127).abs() <= 1);
    }

    #[test]
    fn la_cascade_va_du_fond_au_signal() {
        assert_eq!(waterfall_color(0.0), BACKGROUND);
        assert_eq!(waterfall_color(1.0), ACCENT);
        // L'échelle est continue et croissante.
        let mut precedent = 0u8;
        for pas in 0..=20 {
            let gris = waterfall_color(pas as f32 / 20.0).r;
            assert!(gris >= precedent, "l'échelle redescend à {pas}");
            precedent = gris;
        }
    }

    #[test]
    fn les_niveaux_hors_echelle_sont_ramenes_dans_les_bornes() {
        assert_eq!(normalize_db(-200.0, -120.0, 0.0), 0.0);
        assert_eq!(normalize_db(50.0, -120.0, 0.0), 1.0);
        assert!((normalize_db(-60.0, -120.0, 0.0) - 0.5).abs() < 0.01);
        // Bornes incohérentes : pas de division par zéro.
        assert_eq!(normalize_db(-60.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn la_feuille_de_style_declare_toutes_les_couleurs() {
        let css = css_variables();
        for couleur in [BACKGROUND, PANEL, SURFACE, TEXT, TEXT_SOFT, MUTED, DECOR, BORDER] {
            assert!(css.contains(&couleur.hex()), "{} absente", couleur.hex());
        }
        assert!(css.contains(FONT_MONO));
        assert!(css.contains("--ink-t"));
    }
}
