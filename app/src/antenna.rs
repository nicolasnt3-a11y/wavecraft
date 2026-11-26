//! Géolocalisation de l'utilisateur et guide d'orientation d'antenne.
//!
//! À partir de la position de l'auditeur (détectée par IP sur demande ou
//! saisie à la main) et du lieu d'émission annoncé par Priyom, ce module
//! calcule l'azimut grand cercle, la distance, et explique simplement comment
//! tendre un fil ou orienter une antenne pour capter la station.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// Position géographique, en degrés décimaux.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub lat: f64,
    pub lon: f64,
    /// Nom lisible du lieu : « Bruxelles, Belgique ».
    #[serde(default)]
    pub label: String,
}

/// Orientation calculée pour une station donnée.
#[derive(Debug, Clone, PartialEq)]
pub struct Guidance {
    /// Lieu d'émission identifié (ex. « Moscou, Russie »).
    pub transmitter_label: String,
    /// Azimut vers l'émetteur, de 0° (Nord) à 359°.
    pub bearing_deg: f64,
    /// Distance grand cercle, en kilomètres.
    pub distance_km: f64,
    /// Axe conseillé pour tendre un fil (perpendiculaire à l'émetteur), de 0° à 179°.
    pub wire_axis_deg: f64,
    /// Direction cardinale de l'émetteur (« Est-Nord-Est »).
    pub direction: &'static str,
    /// Direction cardinale courte (« ENE »).
    pub direction_short: &'static str,
    /// Extrémités de l'axe du fil (« Nord-Nord-Ouest ↔ Sud-Sud-Est »).
    pub wire_axis_label: String,
    /// Conseil court et concret pour un fil tendu.
    pub wire_tip: String,
    /// Conseil pour un fouet télescopique ou une fenêtre.
    pub window_tip: String,
    /// Conseil pour une boucle magnétique (loop).
    pub loop_tip: String,
}

/// Détecte la position approximative de l'utilisateur par son adresse IP.
///
/// Aucun compte ni clé API n'est nécessaire. Utilise `ipwho.is` puis, en
/// secours, `ip-api.com`.
pub fn detect_location() -> Result<Position> {
    if let Ok(pos) = detect_ipwhois() {
        return Ok(pos);
    }
    detect_ipapi().context("géolocalisation impossible (vérifiez votre connexion Internet)")
}

#[derive(Deserialize)]
struct IpWhoIs {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    latitude: f64,
    #[serde(default)]
    longitude: f64,
    #[serde(default)]
    city: String,
    #[serde(default)]
    country: String,
}

fn detect_ipwhois() -> Result<Position> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(6)))
        .build()
        .new_agent();
    let body = agent
        .get("https://ipwho.is/")
        .call()
        .context("interrogation de ipwho.is")?
        .body_mut()
        .read_to_string()
        .context("lecture de ipwho.is")?;
    let parsed: IpWhoIs = serde_json::from_str(&body).context("réponse ipwho.is illisible")?;
    if !parsed.success {
        bail!("ipwho.is a refusé la localisation");
    }
    Ok(Position {
        lat: parsed.latitude,
        lon: parsed.longitude,
        label: join_place(&parsed.city, &parsed.country),
    })
}

#[derive(Deserialize)]
struct IpApi {
    #[serde(default)]
    status: String,
    #[serde(default)]
    lat: f64,
    #[serde(default)]
    lon: f64,
    #[serde(default)]
    city: String,
    #[serde(default)]
    country: String,
}

fn detect_ipapi() -> Result<Position> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(6)))
        .build()
        .new_agent();
    let body = agent
        .get("http://ip-api.com/json/?fields=status,lat,lon,city,country")
        .call()
        .context("interrogation de ip-api.com")?
        .body_mut()
        .read_to_string()
        .context("lecture de ip-api.com")?;
    let parsed: IpApi = serde_json::from_str(&body).context("réponse ip-api.com illisible")?;
    if parsed.status != "success" {
        bail!("ip-api.com a refusé la localisation");
    }
    Ok(Position {
        lat: parsed.lat,
        lon: parsed.lon,
        label: join_place(&parsed.city, &parsed.country),
    })
}

fn join_place(city: &str, country: &str) -> String {
    match (city.trim(), country.trim()) {
        ("", "") => "Position détectée".to_string(),
        ("", c) => c.to_string(),
        (v, "") => v.to_string(),
        (v, c) => format!("{v}, {c}"),
    }
}

/// Coordonnées approximatives d'un lieu d'émission tel qu'écrit dans Priyom.
///
/// Lit d'abord d'éventuelles coordonnées DMS (`52°25'46"N 20°52'53"E`), puis
/// reconnaît les villes et pays connus du catalogue.
pub fn resolve_transmitter(raw_location: &str) -> Option<Position> {
    let text = raw_location.trim();
    if text.is_empty() {
        return None;
    }

    if let Some((lat, lon)) = parse_dms(text) {
        // Garde la mention entre parenthèses si elle existe, ex. « Warsaw, Poland ».
        let label = text
            .split('(')
            .nth(1)
            .and_then(|s| s.split(')').next())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(text)
            .to_string();
        return Some(Position { lat, lon, label });
    }

    let lower = text.to_ascii_lowercase();
    // Villes précises d'abord, puis pays.
    const KNOWN: &[(&str, f64, f64, &str)] = &[
        ("saint petersburg", 59.93, 30.33, "Saint-Pétersbourg, Russie"),
        ("st. petersburg", 59.93, 30.33, "Saint-Pétersbourg, Russie"),
        ("moscow", 55.75, 37.62, "Moscou, Russie"),
        ("naro-fominsk", 55.39, 36.73, "Naro-Fominsk (Moscou), Russie"),
        ("rostov-on-don", 47.23, 39.70, "Rostov-sur-le-Don, Russie"),
        ("smolensk", 54.78, 32.04, "Smolensk, Russie"),
        ("orenburg", 51.77, 55.10, "Orenbourg, Russie"),
        ("chita", 52.03, 113.50, "Tchita, Russie"),
        ("khabarovsk", 48.48, 135.07, "Khabarovsk, Russie"),
        ("warsaw", 52.23, 21.01, "Varsovie, Pologne"),
        ("stuttgart", 48.78, 9.18, "Stuttgart, Allemagne"),
        ("bankya", 42.70, 23.15, "Bankya (Sofia), Bulgarie"),
        ("bulgaria", 42.70, 23.32, "Bulgarie"),
        ("belarus", 53.90, 27.56, "Biélorussie"),
        ("havana", 23.11, -82.37, "La Havane, Cuba"),
        ("cuba", 23.11, -82.37, "Cuba"),
        ("france", 47.08, 2.40, "France"),
        ("germany", 51.16, 10.45, "Allemagne"),
        ("poland", 52.23, 21.01, "Pologne"),
        ("taiwan", 25.03, 121.57, "Taïwan"),
        ("north korea", 39.04, 125.76, "Corée du Nord"),
        ("southern military district", 47.23, 39.70, "Sud de la Russie"),
        ("russia", 55.75, 37.62, "Russie (région de Moscou)"),
    ];

    for &(needle, lat, lon, label) in KNOWN {
        if lower.contains(needle) {
            return Some(Position {
                lat,
                lon,
                label: label.to_string(),
            });
        }
    }
    None
}

/// Extrait une paire de coordonnées DMS (`48°41'26"N 9°05'12"E`) si présente.
fn parse_dms(text: &str) -> Option<(f64, f64)> {
    let re = regex::Regex::new(
        r#"(\d{1,3})°\s*(\d{1,2})'\s*(\d{1,2})(?:\.\d+)?"?\s*([NSns])\s+(\d{1,3})°\s*(\d{1,2})'\s*(\d{1,2})(?:\.\d+)?"?\s*([EWew])"#,
    )
    .ok()?;
    let cap = re.captures(text)?;
    let dms = |d: &str, m: &str, s: &str, hemi: &str| -> Option<f64> {
        let deg: f64 = d.parse().ok()?;
        let min: f64 = m.parse().ok()?;
        let sec: f64 = s.parse().ok()?;
        let val = deg + min / 60.0 + sec / 3600.0;
        let sign = if matches!(hemi, "S" | "s" | "W" | "w") {
            -1.0
        } else {
            1.0
        };
        Some(val * sign)
    };
    let lat = dms(&cap[1], &cap[2], &cap[3], &cap[4])?;
    let lon = dms(&cap[5], &cap[6], &cap[7], &cap[8])?;
    Some((lat, lon))
}

/// Azimut initial grand cercle (en degrés, 0° = Nord, 90° = Est) de `from` vers `to`.
pub fn bearing_deg(from: &Position, to: &Position) -> f64 {
    let lat1 = from.lat.to_radians();
    let lat2 = to.lat.to_radians();
    let dlon = (to.lon - from.lon).to_radians();

    let y = dlon.sin() * lat2.cos();
    let x = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * dlon.cos();
    (y.atan2(x).to_degrees() + 360.0) % 360.0
}

/// Distance grand cercle (formule de Haversine), en kilomètres.
pub fn distance_km(from: &Position, to: &Position) -> f64 {
    const EARTH_RADIUS_KM: f64 = 6_371.0;
    let lat1 = from.lat.to_radians();
    let lat2 = to.lat.to_radians();
    let dlat = (to.lat - from.lat).to_radians();
    let dlon = (to.lon - from.lon).to_radians();

    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).max(0.0).sqrt());
    EARTH_RADIUS_KM * c
}

/// Direction cardinale en français (8 secteurs) : (« Nord-Est », « NE »).
pub fn cardinal(deg: f64) -> (&'static str, &'static str) {
    let index = (((deg + 22.5) % 360.0) / 45.0).floor() as usize;
    const SECTORS: [(&str, &str); 8] = [
        ("Nord", "N"),
        ("Nord-Est", "NE"),
        ("Est", "E"),
        ("Sud-Est", "SE"),
        ("Sud", "S"),
        ("Sud-Ouest", "SO"),
        ("Ouest", "O"),
        ("Nord-Ouest", "NO"),
    ];
    SECTORS[index % 8]
}

/// Calcule les conseils simples d'orientation d'antenne entre l'auditeur et un émetteur.
pub fn guidance(user: &Position, tx: &Position) -> Guidance {
    let bearing = bearing_deg(user, tx);
    let distance = distance_km(user, tx);
    let (direction, direction_short) = cardinal(bearing);

    // Un fil tendu (dipôle ou long fil) capte au maximum perpendiculairement à son axe.
    let wire_a = (bearing + 90.0) % 180.0;
    let wire_b = (wire_a + 180.0) % 360.0;
    let (end_a, _) = cardinal(wire_a);
    let (end_b, _) = cardinal(wire_b);
    let wire_axis_label = format!("{end_a} ↔ {end_b}");

    let wire_tip = format!(
        "Antenne filaire : tendez votre fil sur l'axe {wire_axis_label} \
         (perpendiculaire à l'émetteur, qui se trouve au {direction}). \
         Un fil capte par ses côtés, pas par ses pointes."
    );
    let window_tip = format!(
        "Fenêtre / fouet vertical : placez l'antenne près d'une fenêtre dégagée \
         côté {direction}, le plus loin possible des chargeurs et écrans."
    );
    let loop_tip = format!(
        "Antenne boucle (loop) : pointez la tranche de la boucle vers le {direction} \
         ({:.0}°).",
        bearing
    );

    Guidance {
        transmitter_label: tx.label.clone(),
        bearing_deg: bearing,
        distance_km: distance,
        wire_axis_deg: wire_a,
        direction,
        direction_short,
        wire_axis_label,
        wire_tip,
        window_tip,
        loop_tip,
    }
}

/// Émetteur de référence (Moscou) utilisé quand aucune station précise n'est
/// encore choisie (par exemple dans l'assistant de premier lancement).
pub fn default_reference_tx() -> Position {
    Position {
        lat: 55.75,
        lon: 37.62,
        label: "Moscou, Russie (S28 / stations majeures)".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_calcul_d_azimut_et_de_distance_est_coherent() {
        let bruxelles = Position {
            lat: 50.85,
            lon: 4.35,
            label: "Bruxelles".into(),
        };
        let moscou = Position {
            lat: 55.75,
            lon: 37.62,
            label: "Moscou".into(),
        };
        let az = bearing_deg(&bruxelles, &moscou);
        let dist = distance_km(&bruxelles, &moscou);
        // De Bruxelles vers Moscou : ~64° (Nord-Est / Est) et ~2 250 km.
        assert!((az - 64.0).abs() < 4.0, "azimut inattendu : {az:.1}°");
        assert!((dist - 2_250.0).abs() < 80.0, "distance inattendue : {dist:.0} km");
    }

    #[test]
    fn les_coordonnees_dms_de_priyom_sont_lues() {
        let e11 = resolve_transmitter("52°25'46\"N 20°52'53\"E (Warsaw, Poland)").unwrap();
        assert!((e11.lat - 52.429).abs() < 0.01);
        assert!((e11.lon - 20.881).abs() < 0.01);
        assert_eq!(e11.label, "Warsaw, Poland");

        let v32 = resolve_transmitter("48°41'26\"N 9°05'12\"E (Stuttgart, Germany)").unwrap();
        assert!((v32.lat - 48.69).abs() < 0.02);
        assert!((v32.lon - 9.086).abs() < 0.02);
    }

    #[test]
    fn les_villes_et_pays_du_catalogue_sont_reconnus() {
        assert!(resolve_transmitter("Moscow, Russia").is_some());
        assert!(resolve_transmitter("Controlled from Saint Petersburg").is_some());
        assert!(resolve_transmitter("Republic of Belarus").is_some());
        assert!(resolve_transmitter("Taiwan").is_some());
        assert!(resolve_transmitter("").is_none());
    }

    #[test]
    fn le_conseil_filaire_est_perpendiculaire_a_l_emetteur() {
        let paris = Position {
            lat: 48.86,
            lon: 2.35,
            label: "Paris".into(),
        };
        let moscou = default_reference_tx();
        let g = guidance(&paris, &moscou);
        // Vers l'Est-Nord-Est (~60°) -> le fil doit être tendu Nord-Ouest ↔ Sud-Est (~150°).
        let diff = ((g.wire_axis_deg - g.bearing_deg).abs() % 180.0 - 90.0).abs();
        assert!(diff < 1.0, "axe fil non perpendiculaire : diff={diff}");
        assert!(g.wire_tip.contains("perpendiculaire"));
        assert!(g.window_tip.contains(g.direction));
    }
}
