//! Captures d'écran de la fenêtre.
//!
//! `wavecraft --captures <dossier>` ouvre la fenêtre sur la source de
//! démonstration, passe sur chaque écran, en enregistre une image, puis quitte.
//! C'est ainsi que sont faites les images du site : elles montrent le logiciel
//! réel, jamais un dessin, et se refont en une commande quand l'interface change.

use std::path::{Path, PathBuf};

use egui::ColorImage;

use super::Page;

/// Taille des images produites, en points. C'est celle qu'attend le site.
pub const WIDTH: usize = 1280;
pub const HEIGHT: usize = 800;

/// Les écrans capturés, dans l'ordre, avec le nom de fichier attendu.
const ECRANS: [(&str, Etape); 6] = [
    ("catalogue", Etape::Ecran(Page::Catalogue)),
    ("ecoute", Etape::Ecran(Page::Ecoute)),
    ("morse", Etape::Ecran(Page::Morse)),
    ("journal", Etape::Ecran(Page::Journal)),
    ("serveur-web", Etape::Ecran(Page::Serveur)),
    ("assistant", Etape::Assistant),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Etape {
    Ecran(Page),
    Assistant,
}

pub struct Captures {
    dossier: PathBuf,
    index: usize,
    /// Images écoulées depuis l'arrivée sur l'écran courant.
    attente: u32,
    demandée: bool,
    /// Images restant à laisser passer avant de changer d'écran.
    ///
    /// La capture porte sur l'image *suivante* : changer d'écran aussitôt
    /// photographierait le suivant à la place.
    grace: u32,
}

impl Captures {
    pub fn new(dossier: PathBuf) -> Self {
        Self {
            dossier,
            index: 0,
            attente: 0,
            demandée: false,
            grace: 0,
        }
    }

    /// Écran à afficher maintenant, s'il en reste.
    pub fn etape(&self) -> Option<Etape> {
        ECRANS.get(self.index).map(|(_, etape)| *etape)
    }

    pub fn fini(&self) -> bool {
        self.index >= ECRANS.len()
    }

    /// Le temps d'attente laisse la cascade se remplir et le Morse s'écrire.
    ///
    /// Renvoie le nom du fichier quand il est temps de déclencher.
    pub fn declencher(&mut self) -> Option<&'static str> {
        if self.fini() {
            return None;
        }
        if self.demandée {
            // La demande est partie : on laisse l'image être rendue et lue,
            // puis seulement on passe à l'écran suivant.
            self.grace += 1;
            if self.grace >= 3 {
                self.suivant();
            }
            return None;
        }
        self.attente += 1;
        // Le premier écran attend plus longtemps : la démonstration doit avoir
        // eu le temps d'émettre quelques groupes de chiffres.
        let seuil = if self.index == 0 { 80 } else { 40 };
        if self.attente < seuil {
            return None;
        }
        self.demandée = true;
        ECRANS.get(self.index).map(|(nom, _)| *nom)
    }

    /// Passe à l'écran suivant.
    pub fn suivant(&mut self) {
        self.index += 1;
        self.attente = 0;
        self.demandée = false;
        self.grace = 0;
    }

    /// Pendant l'attente du premier écran, l'application change de station
    /// régulièrement : le journal montre ainsi de vraies sessions, et la
    /// cascade a le temps de se remplir sur chacune.
    pub fn station_suivante(&self) -> Option<usize> {
        // Une fois la capture demandée, l'attente est figée : sans cette garde,
        // la même station serait reprise à chaque image.
        if self.index != 0 || self.demandée || self.attente == 0 || self.attente % 20 != 0 {
            return None;
        }
        Some((self.attente / 20) as usize)
    }

    pub fn dossier(&self) -> &Path {
        &self.dossier
    }
}

/// Ramène une image à 1280×800 et l'écrit en PNG.
pub fn ecrire(image: &ColorImage, chemin: &Path) -> Result<(), String> {
    let réduite = redimensionner(image, WIDTH, HEIGHT);
    if let Some(parent) = chemin.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let fichier =
        std::fs::File::create(chemin).map_err(|e| format!("{}: {e}", chemin.display()))?;
    let mut encodeur = png::Encoder::new(
        std::io::BufWriter::new(fichier),
        WIDTH as u32,
        HEIGHT as u32,
    );
    encodeur.set_color(png::ColorType::Rgb);
    encodeur.set_depth(png::BitDepth::Eight);
    encodeur.set_compression(png::Compression::High);
    let mut écriture = encodeur
        .write_header()
        .map_err(|e| format!("en-tête PNG : {e}"))?;
    écriture
        .write_image_data(&réduite)
        .map_err(|e| format!("écriture PNG : {e}"))?;
    écriture.finish().map_err(|e| format!("PNG : {e}"))?;
    Ok(())
}

/// Réduction par moyenne de blocs : nette, sans dépendance, et suffisante
/// puisqu'on ne fait que diviser la taille d'une capture.
fn redimensionner(image: &ColorImage, largeur: usize, hauteur: usize) -> Vec<u8> {
    let [source_l, source_h] = image.size;
    let mut out = vec![0u8; largeur * hauteur * 3];
    if source_l == 0 || source_h == 0 {
        return out;
    }
    for y in 0..hauteur {
        let y0 = y * source_h / hauteur;
        let y1 = ((y + 1) * source_h / hauteur).max(y0 + 1).min(source_h);
        for x in 0..largeur {
            let x0 = x * source_l / largeur;
            let x1 = ((x + 1) * source_l / largeur).max(x0 + 1).min(source_l);
            let (mut r, mut v, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let pixel = image.pixels[sy * source_l + sx];
                    r += u32::from(pixel.r());
                    v += u32::from(pixel.g());
                    b += u32::from(pixel.b());
                    n += 1;
                }
            }
            let n = n.max(1);
            let i = (y * largeur + x) * 3;
            out[i] = (r / n) as u8;
            out[i + 1] = (v / n) as u8;
            out[i + 2] = (b / n) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_reduction_donne_la_taille_demandee() {
        let image = ColorImage::filled([64, 40], egui::Color32::from_rgb(10, 20, 30));
        let réduite = redimensionner(&image, 32, 20);
        assert_eq!(réduite.len(), 32 * 20 * 3);
        assert_eq!(&réduite[0..3], &[10, 20, 30]);
    }

    #[test]
    fn l_agrandissement_ne_perd_pas_de_pixel() {
        // Agrandir n'est pas le but, mais ne doit pas produire d'image vide.
        let image = ColorImage::filled([4, 4], egui::Color32::WHITE);
        let agrandie = redimensionner(&image, 8, 8);
        assert!(agrandie.iter().all(|&c| c == 255));
    }

    #[test]
    fn le_premier_ecran_fait_defiler_quelques_stations() {
        let mut captures = Captures::new(PathBuf::from("."));
        let mut vues = Vec::new();
        for _ in 0..79 {
            captures.declencher();
            if let Some(n) = captures.station_suivante() {
                vues.push(n);
            }
        }
        assert_eq!(vues, vec![1, 2, 3]);
        // La demande de capture arrête le défilé.
        captures.declencher();
        captures.declencher();
        assert_eq!(captures.station_suivante(), None);
    }

    #[test]
    fn les_ecrans_se_suivent_puis_s_arretent() {
        let mut captures = Captures::new(PathBuf::from("."));
        assert_eq!(captures.etape(), Some(Etape::Ecran(Page::Catalogue)));
        for _ in 0..ECRANS.len() {
            captures.suivant();
        }
        assert!(captures.fini());
        assert_eq!(captures.etape(), None);
        assert_eq!(captures.declencher(), None);
    }

    #[test]
    fn le_declenchement_attend_que_l_ecran_soit_rempli() {
        let mut captures = Captures::new(PathBuf::from("."));
        for _ in 0..79 {
            assert_eq!(captures.declencher(), None);
        }
        assert_eq!(captures.declencher(), Some("catalogue"));
        // Une seule demande par écran, et l'écran suivant n'arrive qu'après
        // que l'image a été lue.
        assert_eq!(captures.declencher(), None);
        assert_eq!(captures.etape(), Some(Etape::Ecran(Page::Catalogue)));
        captures.declencher();
        captures.declencher();
        assert_eq!(captures.etape(), Some(Etape::Ecran(Page::Ecoute)));
    }
}
