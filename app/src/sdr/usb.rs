//! Repérage des clés RTL-SDR sur le bus USB, sans passer par la bibliothèque.
//!
//! C'est ce qui permet de distinguer deux pannes très différentes : la clé n'est
//! pas branchée, ou elle est branchée mais c'est un autre pilote qui la détient.
//! Dans le second cas seulement, l'assistant de pilote a quelque chose à faire.

use anyhow::{Context, Result};
use nusb::MaybeFuture;

/// Identifiants des puces utilisées par les clés RTL-SDR.
///
/// Realtek a produit ces contrôleurs pour la télévision numérique ; les clés
/// d'écoute en reprennent les identifiants tels quels.
const KNOWN: [(u16, u16); 2] = [(0x0bda, 0x2832), (0x0bda, 0x2838)];

/// Une clé repérée sur le bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub vendor_id: u16,
    pub product_id: u16,
    /// Nom annoncé par la clé, par exemple « Blog V4 ».
    pub product: String,
    pub manufacturer: String,
    pub serial: String,
}

impl Found {
    /// Désignation lisible, telle qu'elle apparaît dans l'assistant.
    pub fn label(&self) -> String {
        let nom = match (self.manufacturer.trim(), self.product.trim()) {
            ("", "") => format!("{:04x}:{:04x}", self.vendor_id, self.product_id),
            ("", produit) => produit.to_string(),
            (fabricant, "") => fabricant.to_string(),
            (fabricant, produit) => format!("{fabricant} {produit}"),
        };
        if self.serial.trim().is_empty() {
            nom
        } else {
            format!("{nom} (n° {})", self.serial.trim())
        }
    }
}

/// Clés RTL-SDR présentes sur le bus USB.
pub fn devices() -> Result<Vec<Found>> {
    let liste = nusb::list_devices()
        .wait()
        .context("lecture du bus USB")?;
    Ok(liste
        .filter(|d| KNOWN.contains(&(d.vendor_id(), d.product_id())))
        .map(|d| Found {
            vendor_id: d.vendor_id(),
            product_id: d.product_id(),
            product: d.product_string().unwrap_or_default().to_string(),
            manufacturer: d.manufacturer_string().unwrap_or_default().to_string(),
            serial: d.serial_number().unwrap_or_default().to_string(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_bus_usb_se_lit_sans_erreur() {
        // Le résultat dépend de ce qui est branché ; seule la lecture compte.
        let liste = devices().expect("le bus USB doit être lisible");
        for clé in &liste {
            assert!(!clé.label().is_empty());
        }
    }

    #[test]
    fn la_designation_se_passe_des_champs_vides() {
        let clé = Found {
            vendor_id: 0x0bda,
            product_id: 0x2838,
            product: String::new(),
            manufacturer: String::new(),
            serial: String::new(),
        };
        assert_eq!(clé.label(), "0bda:2838");

        let clé = Found {
            product: "Blog V4".into(),
            manufacturer: "RTLSDRBlog".into(),
            serial: "00000001".into(),
            ..clé
        };
        assert_eq!(clé.label(), "RTLSDRBlog Blog V4 (n° 00000001)");
    }
}
