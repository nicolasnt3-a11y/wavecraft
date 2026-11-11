//! Mises en forme communes à la fenêtre et au navigateur.
//!
//! Heures, fréquences, S-mètre, délai avant la prochaine émission : ces textes
//! apparaissent à l'identique dans les deux interfaces, et plusieurs sont assez
//! subtils (« DANS 1 H 05 », « DEMAIN 06:00 UTC ») pour mériter des tests.

use chrono::{DateTime, Local, TimeDelta, Timelike, Utc};

/// Heure locale, heure UTC et repère jour/nuit, pour le bandeau du haut.
///
/// Le fuseau et l'heure d'été viennent du système. Le repère jour/nuit est
/// donné par l'heure locale, sans calcul solaire : c'est une indication de
/// propagation, pas une éphéméride.
pub fn clock_line(now: DateTime<Utc>) -> String {
    let locale = now.with_timezone(&Local);
    format!(
        "{} LOC · {} UTC · {}",
        locale.format("%H:%M"),
        now.format("%H:%M"),
        day_night(locale.hour())
    )
}

/// Repère jour/nuit : les fréquences basses passent mieux la nuit.
pub fn day_night(local_hour: u32) -> &'static str {
    if (7..19).contains(&local_hour) {
        "☀ JOUR"
    } else {
        "☾ NUIT"
    }
}

/// Fréquence en kilohertz, avec une espace fine aux milliers : « 4 625 ».
pub fn khz(frequency_khz: u32) -> String {
    let brut = frequency_khz.to_string();
    let mut out = String::new();
    for (index, chiffre) in brut.chars().enumerate() {
        if index > 0 && (brut.len() - index) % 3 == 0 {
            out.push('\u{202f}');
        }
        out.push(chiffre);
    }
    out
}

/// Fréquence au hertz près, telle qu'elle s'affiche en grand : « 4 625.000 ».
pub fn tuned(frequency_hz: u32) -> String {
    format!("{}.{:03}", khz(frequency_hz / 1_000), frequency_hz % 1_000)
}

/// Quand une station est-elle écoutable ?
///
/// `next` est l'heure de la prochaine émission annoncée, `continuous` indique
/// une station qui émet sans interruption.
pub fn when(continuous: bool, next: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    if continuous {
        return "EN COURS · 24 H/24".to_string();
    }
    let Some(at) = next else {
        return "HORAIRE INCONNU".to_string();
    };
    let reste = at - now;
    if reste <= TimeDelta::zero() {
        return "EN COURS".to_string();
    }
    let minutes = reste.num_minutes();
    match minutes {
        0 => "DANS MOINS D'UNE MINUTE".to_string(),
        1..=59 => format!("DANS {minutes} MIN"),
        60..=1439 => {
            // Au-delà d'une heure, l'heure exacte compte plus que le décompte.
            if at.date_naive() == now.date_naive() {
                format!("DANS {} H {:02}", minutes / 60, minutes % 60)
            } else if at.date_naive() == now.date_naive().succ_opt().unwrap_or(at.date_naive()) {
                format!("DEMAIN {} UTC", at.format("%H:%M"))
            } else {
                format!("LE {} UTC", at.format("%d/%m %H:%M"))
            }
        }
        _ => format!("LE {} UTC", at.format("%d/%m %H:%M")),
    }
}

/// Niveau de référence du S-mètre, en dB pleine échelle.
///
/// Une clé RTL-SDR n'est pas étalonnée : l'échelle est relative au niveau
/// d'entrée du convertisseur, pas en microvolts à l'antenne. S9 est posé à
/// −20 dBFS, et chaque point S vaut 6 dB, comme sur un récepteur de trafic.
pub const S9_DBFS: f32 = -20.0;
/// Écart entre deux points S, en dB.
pub const S_STEP_DB: f32 = 6.0;

/// S-mètre : « S4 », « S9 », « S9 +10 ».
pub fn s_meter(dbfs: f32) -> String {
    let points = (dbfs - S9_DBFS) / S_STEP_DB + 9.0;
    if points < 1.0 {
        return "S0".to_string();
    }
    if points <= 9.0 {
        return format!("S{}", points.floor() as i32);
    }
    // Au-delà de S9, l'usage est de compter en décibels au-dessus.
    let au_dessus = ((dbfs - S9_DBFS) / 10.0).floor() as i32 * 10;
    if au_dessus <= 0 {
        "S9".to_string()
    } else {
        format!("S9 +{au_dessus}")
    }
}

/// Durée d'écoute, en minutes et secondes.
pub fn duration(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let heures = total / 3_600;
    if heures > 0 {
        format!("{heures} h {:02} min", (total % 3_600) / 60)
    } else {
        format!("{:02}:{:02}", total / 60, total % 60)
    }
}

/// Date d'une entrée de journal, en UTC comme sur la maquette.
pub fn stamp(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%d %H:%M").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn instant(jour: u32, heure: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, jour, heure, minute, 0).unwrap()
    }

    #[test]
    fn les_milliers_sont_separes() {
        assert_eq!(khz(4_625), "4\u{202f}625");
        assert_eq!(khz(11_435), "11\u{202f}435");
        assert_eq!(khz(500), "500");
        assert_eq!(tuned(4_625_000), "4\u{202f}625.000");
        assert_eq!(tuned(6_917_500), "6\u{202f}917.500");
    }

    #[test]
    fn une_station_permanente_est_toujours_en_cours() {
        let maintenant = instant(11, 20, 42);
        assert_eq!(when(true, None, maintenant), "EN COURS · 24 H/24");
    }

    #[test]
    fn le_decompte_suit_la_maquette() {
        let maintenant = instant(11, 20, 42);
        assert_eq!(when(false, Some(instant(11, 20, 54)), maintenant), "DANS 12 MIN");
        assert_eq!(when(false, Some(instant(11, 21, 47)), maintenant), "DANS 1 H 05");
        assert_eq!(when(false, Some(instant(12, 6, 0)), maintenant), "DEMAIN 06:00 UTC");
        assert_eq!(when(false, Some(instant(14, 7, 0)), maintenant), "LE 14/11 07:00 UTC");
        assert_eq!(when(false, Some(instant(11, 20, 41)), maintenant), "EN COURS");
        assert_eq!(when(false, None, maintenant), "HORAIRE INCONNU");
    }

    #[test]
    fn le_s_metre_couvre_l_echelle() {
        assert_eq!(s_meter(S9_DBFS), "S9");
        assert_eq!(s_meter(S9_DBFS - S_STEP_DB), "S8");
        assert_eq!(s_meter(S9_DBFS - 8.0 * S_STEP_DB), "S1");
        assert_eq!(s_meter(-120.0), "S0");
        assert_eq!(s_meter(S9_DBFS + 10.0), "S9 +10");
        assert_eq!(s_meter(S9_DBFS + 25.0), "S9 +20");
    }

    #[test]
    fn le_bandeau_annonce_les_deux_heures() {
        let ligne = clock_line(instant(11, 20, 42));
        assert!(ligne.contains("20:42 UTC"), "{ligne}");
        assert!(ligne.contains("LOC"), "{ligne}");
        assert!(ligne.contains("JOUR") || ligne.contains("NUIT"), "{ligne}");
    }

    #[test]
    fn la_nuit_commence_le_soir() {
        assert_eq!(day_night(21), "☾ NUIT");
        assert_eq!(day_night(3), "☾ NUIT");
        assert_eq!(day_night(12), "☀ JOUR");
        assert_eq!(day_night(7), "☀ JOUR");
        assert_eq!(day_night(19), "☾ NUIT");
    }

    #[test]
    fn les_durees_restent_lisibles() {
        assert_eq!(duration(0.0), "00:00");
        assert_eq!(duration(95.0), "01:35");
        assert_eq!(duration(3_900.0), "1 h 05 min");
        assert_eq!(duration(-5.0), "00:00");
    }

    #[test]
    fn l_horodatage_du_journal_est_en_utc() {
        assert_eq!(stamp(instant(9, 21, 0)), "2026-11-09 21:00");
    }
}
