//! Habillage egui, repris de la charte du site.
//!
//! Rien d'arrondi, rien de coloré : des rectangles, des filets gris, du texte
//! clair espacé sur fond presque noir, et des boutons qui s'inversent au survol
//! — exactement comme sur nthill.be/wavecraft.

use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, RichText, Stroke, TextStyle};

use crate::theme;

/// Hauteur du bandeau du haut, comme sur les maquettes.
pub const HEADER_HEIGHT: f32 = 48.0;
/// Largeur du rail de navigation.
pub const RAIL_WIDTH: f32 = 200.0;

/// Police à chasse fixe demi-grasse : identifiants de stations, marque.
pub const MONO_BOLD: &str = "plex-mono-semibold";
/// Police condensée grasse : les grands titres de l'assistant.
pub const CONDENSED_BOLD: &str = "plex-condensed-bold";

/// Les polices du site, incluses dans l'exécutable (licence OFL, voir
/// `assets/fonts/OFL.txt`) : la fenêtre a exactement la typographie des
/// maquettes, quelle que soit la machine.
/// Une police incluse : son nom, ses octets, les familles qu'elle sert.
type Police = (&'static str, &'static [u8], &'static [(FontFamily, egui::epaint::text::FontPriority)]);

const FONTS: [Police; 4] = [
    (
        "plex-mono",
        include_bytes!("../../assets/fonts/IBMPlexMono-Regular.ttf"),
        &[(FontFamily::Monospace, egui::epaint::text::FontPriority::Highest)],
    ),
    (
        MONO_BOLD,
        include_bytes!("../../assets/fonts/IBMPlexMono-SemiBold.ttf"),
        &[],
    ),
    (
        "plex-condensed",
        include_bytes!("../../assets/fonts/IBMPlexSansCondensed-Regular.ttf"),
        &[(FontFamily::Proportional, egui::epaint::text::FontPriority::Highest)],
    ),
    (
        CONDENSED_BOLD,
        include_bytes!("../../assets/fonts/IBMPlexSansCondensed-Bold.ttf"),
        &[],
    ),
];

/// Applique la charte au contexte egui. À appeler une fois au démarrage.
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);

    let fond = theme::BACKGROUND.egui();
    let panneau = theme::PANEL.egui();
    let liseré = theme::BORDER.egui();
    let texte = theme::TEXT.egui();
    let blanc = theme::ACCENT.egui();

    let mut visuals = egui::Visuals::dark();
    visuals.dark_mode = true;
    visuals.panel_fill = fond;
    visuals.window_fill = panneau;
    visuals.extreme_bg_color = fond;
    visuals.faint_bg_color = theme::SURFACE.egui();
    visuals.code_bg_color = panneau;
    visuals.override_text_color = Some(texte);
    visuals.weak_text_color = Some(theme::MUTED.egui());
    visuals.hyperlink_color = texte;
    visuals.warn_fg_color = blanc;
    visuals.error_fg_color = blanc;
    visuals.window_stroke = Stroke::new(1.0, liseré);
    visuals.window_corner_radius = CornerRadius::ZERO;
    visuals.menu_corner_radius = CornerRadius::ZERO;
    visuals.window_shadow = egui::epaint::Shadow::NONE;
    visuals.popup_shadow = egui::epaint::Shadow::NONE;
    visuals.selection = egui::style::Selection {
        bg_fill: theme::BORDER.egui(),
        stroke: Stroke::new(1.0, blanc),
    };
    visuals.striped = false;
    visuals.slider_trailing_fill = true;

    // Un bouton est un rectangle vide bordé de gris ; au survol il s'inverse,
    // fond blanc et texte noir, comme les liens du site.
    // `bg_fill` sert au rail des glissières et aux cases à cocher, `weak_bg_fill`
    // au fond des boutons : les deux sont distingués pour qu'un rail reste
    // visible sans éclaircir les boutons.
    let carré = |fill: Color32, bord: Color32, avant: Color32| egui::style::WidgetVisuals {
        bg_fill: if fill == fond { theme::SURFACE.egui() } else { fill },
        weak_bg_fill: fill,
        bg_stroke: Stroke::new(1.0, bord),
        corner_radius: CornerRadius::ZERO,
        fg_stroke: Stroke::new(1.0, avant),
        expansion: 0.0,
    };
    visuals.widgets.noninteractive = carré(fond, theme::LINE.egui(), texte);
    visuals.widgets.inactive = carré(fond, liseré, texte);
    visuals.widgets.hovered = carré(blanc, blanc, fond);
    visuals.widgets.active = carré(texte, texte, fond);
    visuals.widgets.open = carré(panneau, liseré, texte);

    // Les deux thèmes reçoivent la même charte : même si le système est en mode
    // clair, Wavecraft reste noir comme le site.
    ctx.set_visuals_of(egui::Theme::Dark, visuals.clone());
    ctx.set_visuals_of(egui::Theme::Light, visuals);

    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(14.0, 8.0);
        style.spacing.window_margin = Margin::same(10);
        style.spacing.menu_margin = Margin::same(6);
        style.spacing.slider_width = 180.0;
        style.spacing.interact_size.y = 26.0;
        style.visuals.button_frame = true;

        let mono = FontFamily::Monospace;
        style.text_styles = [
            (TextStyle::Small, FontId::new(11.0, mono.clone())),
            (TextStyle::Body, FontId::new(13.0, mono.clone())),
            (TextStyle::Button, FontId::new(13.0, mono.clone())),
            (TextStyle::Monospace, FontId::new(13.0, mono.clone())),
            (TextStyle::Heading, FontId::new(18.0, mono)),
        ]
        .into();
    });
}

/// Installe les polices incluses. Les glyphes qu'elles n'ont pas — alphabets
/// cyrillique ou autres noms de stations — viennent des polices du système.
fn install_fonts(ctx: &egui::Context) {
    for (nom, octets, familles) in FONTS {
        let mut cibles: Vec<egui::epaint::text::InsertFontFamily> = familles
            .iter()
            .map(|(famille, priorité)| egui::epaint::text::InsertFontFamily {
                family: famille.clone(),
                priority: *priorité,
            })
            .collect();
        // Chaque police est aussi accessible par son nom, pour les titres.
        cibles.push(egui::epaint::text::InsertFontFamily {
            family: FontFamily::Name(nom.into()),
            priority: egui::epaint::text::FontPriority::Highest,
        });
        ctx.add_font(egui::epaint::text::FontInsert {
            name: nom.to_string(),
            data: egui::FontData::from_static(octets),
            families: cibles,
        });
    }
}

/// Bouton de la charte : capitales espacées ; inversé (fond blanc) s'il est
/// l'action principale de l'écran.
pub fn bouton(ui: &mut egui::Ui, texte: &str, principal: bool) -> egui::Response {
    let (fond, avant) = if principal {
        (theme::ACCENT, theme::BACKGROUND)
    } else {
        (theme::BACKGROUND, theme::TEXT)
    };
    let libellé = RichText::new(texte.to_uppercase())
        .size(12.0)
        .color(avant.egui())
        .extra_letter_spacing(2.5);
    let bouton = egui::Button::new(libellé)
        .fill(fond.egui())
        .stroke(Stroke::new(1.0, if principal { theme::ACCENT } else { theme::BORDER }.egui()))
        .min_size(egui::vec2(0.0, 44.0));
    let réponse = ui.add(bouton);
    if réponse.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    réponse
}

/// Même bouton, à largeur imposée.
pub fn bouton_large(ui: &mut egui::Ui, texte: &str, principal: bool, largeur: f32) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(largeur, 44.0),
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| bouton(ui, texte, principal),
    )
    .inner
}

/// Capitales espacées : la typographie du site.
pub fn caps(texte: &str, taille: f32, couleur: theme::Color) -> RichText {
    RichText::new(texte.to_uppercase())
        .size(taille)
        .color(couleur.egui())
        .extra_letter_spacing(taille * 0.18)
}

/// Ligne de titre d'un écran, en petites capitales discrètes.
pub fn caption(ui: &mut egui::Ui, texte: &str) {
    ui.label(caps(texte, 11.0, theme::MUTED));
}

/// Cadre d'un panneau : fond à peine détaché, liseré gris.
pub fn panel() -> egui::Frame {
    egui::Frame::NONE
        .fill(theme::PANEL.egui())
        .stroke(Stroke::new(1.0, theme::BORDER.egui()))
        .inner_margin(Margin::same(12))
}

/// Filet horizontal discret, entre deux lignes d'une liste.
pub fn separator(ui: &mut egui::Ui) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0, theme::LINE.egui()),
    );
}

/// Lignes de balayage du site, posées sur une zone sombre.
///
/// Une ligne sur trois est à peine éclaircie : de près on voit la trame d'un
/// écran cathodique, de loin c'est du noir.
pub fn scanlines(painter: &egui::Painter, rect: egui::Rect) {
    let voile = Color32::from_rgba_unmultiplied(255, 255, 255, 6);
    let mut y = rect.top();
    while y < rect.bottom() {
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(rect.left(), y), egui::pos2(rect.right(), y + 1.0)),
            0.0,
            voile,
        );
        y += 3.0;
    }
}

/// Écrit des capitales espacées à la main, lettre par lettre.
///
/// `egui` n'espace les lettres que dans un `RichText` ; pour du texte peint
/// directement sur un rectangle, il faut le faire soi-même.
pub fn painted_caps(
    ui: &egui::Ui,
    origine: egui::Pos2,
    texte: &str,
    taille: f32,
    couleur: theme::Color,
) -> f32 {
    painted_caps_font(ui, origine, texte, taille, couleur, FontFamily::Monospace)
}

/// Même chose, dans une famille de police choisie (demi-gras, condensée…).
pub fn painted_caps_font(
    ui: &egui::Ui,
    origine: egui::Pos2,
    texte: &str,
    taille: f32,
    couleur: theme::Color,
    famille: FontFamily,
) -> f32 {
    let mut x = origine.x;
    for lettre in texte.to_uppercase().chars() {
        let galley = ui.ctx().fonts_mut(|f| {
            f.layout_no_wrap(
                lettre.to_string(),
                FontId::new(taille, famille.clone()),
                couleur.egui(),
            )
        });
        let largeur = galley.size().x;
        ui.painter()
            .galley(egui::pos2(x, origine.y), galley, couleur.egui());
        x += largeur + taille * 0.18;
    }
    x - origine.x
}

/// Étoile à cinq branches, pleine ou vide : la note du journal.
///
/// Dessinée plutôt qu'écrite : le glyphe ★ manque à la plupart des polices à
/// chasse fixe, et un carré vide à sa place ferait désordre.
pub fn etoile(painter: &egui::Painter, centre: egui::Pos2, rayon: f32, pleine: bool) {
    let mut points = Vec::with_capacity(10);
    for n in 0..10 {
        let angle = -std::f32::consts::FRAC_PI_2 + n as f32 * std::f32::consts::PI / 5.0;
        let r = if n % 2 == 0 { rayon } else { rayon * 0.45 };
        points.push(egui::pos2(centre.x + r * angle.cos(), centre.y + r * angle.sin()));
    }
    let couleur = if pleine { theme::TEXT_SOFT } else { theme::DECOR };
    if pleine {
        painter.add(egui::Shape::convex_polygon(points, couleur.egui(), Stroke::NONE));
    } else {
        painter.add(egui::Shape::closed_line(points, Stroke::new(1.0, couleur.egui())));
    }
}

/// Coche de validation, en deux traits.
pub fn coche(painter: &egui::Painter, centre: egui::Pos2, taille: f32, couleur: theme::Color) {
    let s = taille / 2.0;
    let trait_ = Stroke::new(1.5, couleur.egui());
    painter.line_segment(
        [egui::pos2(centre.x - s, centre.y), egui::pos2(centre.x - s * 0.3, centre.y + s * 0.8)],
        trait_,
    );
    painter.line_segment(
        [egui::pos2(centre.x - s * 0.3, centre.y + s * 0.8), egui::pos2(centre.x + s, centre.y - s * 0.8)],
        trait_,
    );
}

/// Entrée du rail de navigation : repère blanc à gauche quand elle est active.
pub fn rail_item(ui: &mut egui::Ui, texte: &str, actif: bool) -> egui::Response {
    let hauteur = 46.0;
    let (rect, réponse) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), hauteur), egui::Sense::click());
    let couleur = if actif {
        theme::ACCENT
    } else if réponse.hovered() {
        theme::TEXT
    } else {
        theme::MUTED
    };
    if actif {
        // Le repère touche le bord gauche de la fenêtre, comme sur la maquette.
        ui.painter().rect_filled(
            egui::Rect::from_min_size(rect.left_top(), egui::vec2(2.0, hauteur)),
            0.0,
            theme::ACCENT.egui(),
        );
    }
    painted_caps(
        ui,
        egui::pos2(rect.left() + 28.0, rect.center().y - 9.0),
        texte,
        13.0,
        couleur,
    );
    if réponse.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    réponse
}
