//! Wavecraft — récepteur dédié aux stations de nombres.
//!
//! Licence GPL-3.0-or-later. Catalogue et planning : Priyom.org.

// Sans la fenêtre, une partie du moteur (gain, bias-tee, sortie locale) est
// compilée sans être appelée : c'est normal pour le paquet serveur, et cela ne
// doit pas noyer la compilation d'avertissements. La variante complète, elle,
// est vérifiée sans cette tolérance.
#![cfg_attr(not(feature = "desktop"), allow(dead_code))]

mod antenna;
mod audio;
mod catalog;
mod cli;
mod dsp;
mod engine;
mod format;
mod link;
mod logbook;
mod morse;
mod paths;
mod priyom;
mod recorder;
mod sdr;
#[cfg(feature = "server")]
mod server;
mod settings;
mod state;
mod theme;
#[cfg(feature = "desktop")]
mod ui;

use anyhow::Result;
use chrono::{TimeDelta, Utc};

use crate::catalog::Catalog;
use crate::cli::{Mode, Options};
use crate::settings::Settings;

fn main() -> std::process::ExitCode {
    filet_de_panique();
    let options = match cli::parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(err) => {
            eprintln!("{err}");
            return std::process::ExitCode::from(2);
        }
    };

    if let Mode::Print(text) = options.mode {
        println!("{text}");
        return std::process::ExitCode::SUCCESS;
    }

    match run(options) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Erreur : {err:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Une panique dans la fenêtre ne laisse aucune trace à l'écran : elle est
/// écrite dans `panique.log`, à côté des réglages, avant l'arrêt.
fn filet_de_panique() {
    let défaut = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let chemin = paths::data_dir().join("panique.log");
        let texte = format!(
            "{} — Wavecraft {}\n{info}\n\n{}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            env!("CARGO_PKG_VERSION"),
            std::backtrace::Backtrace::force_capture()
        );
        let _ = paths::ensure_parent(&chemin).and_then(|_| std::fs::write(&chemin, texte));
        défaut(info);
    }));
}

fn run(options: Options) -> Result<()> {
    let settings = Settings::load()?;

    match options.mode {
        Mode::Probe => probe(&settings),
        Mode::InstallDriver => sdr::pilote::executer_depuis_la_ligne_de_commande(),
        Mode::Server => {
            #[cfg(feature = "server")]
            {
                server::serve(&options)
            }
            #[cfg(not(feature = "server"))]
            {
                anyhow::bail!("cette version a été compilée sans le serveur web");
            }
        }
        Mode::Desktop => {
            #[cfg(feature = "desktop")]
            {
                ui::run(options.demo, options.captures.clone())
            }
            #[cfg(not(feature = "desktop"))]
            {
                anyhow::bail!(
                    "cette version a été compilée sans interface bureau ; \
                     essayez « wavecraft --server » ou « wavecraft --probe »"
                );
            }
        }
        Mode::Print(_) => Ok(()),
    }
}

/// Diagnostic : catalogue, agenda Priyom et réglages.
///
/// Sert au dépannage sur le poste de l'utilisateur — et de vérification de bout
/// en bout du relevé d'agenda, plafond de 250 événements compris.
fn probe(settings: &Settings) -> Result<()> {
    println!("Wavecraft {}", env!("CARGO_PKG_VERSION"));
    println!();

    println!("Réglages   : {}", Settings::path().display());
    println!("Données    : {}", paths::data_dir().display());
    println!("Audio      : {}", settings.recordings().display());
    println!();

    let catalogue = Catalog::load_best(&paths::data_dir().join("catalog.json"));
    let age = catalogue
        .age(Utc::now())
        .map(|d| format!("{} jours", d.as_secs() / 86_400))
        .unwrap_or_else(|| "inconnu".to_string());
    println!(
        "Catalogue  : version {}, {} stations, généré il y a {age}",
        catalogue.version,
        catalogue.stations.len()
    );
    let listenable = catalogue.listenable();
    println!("             {} stations écoutables, dont :", listenable.len());
    for station in listenable.iter().take(5) {
        if let Some(preset) = station.preset() {
            println!(
                "               {:<34} {:>6} kHz {}",
                station.display_name(),
                preset.frequency_khz,
                preset.mode
            );
        }
    }
    println!();

    print!("Agenda     : interrogation de Priyom… ");
    let now = Utc::now();
    let client = priyom::Priyom::new();
    match client.fetch_range(now, now + TimeDelta::days(settings::PLANNING_DAYS)) {
        Ok(transmissions) => {
            println!("{} émissions sur 7 jours", transmissions.len());
            if transmissions.len() <= priyom::MAX_RESULTS {
                println!(
                    "             ⚠ relevé suspect : le plafond de {} événements par requête \
                     n'a peut-être pas été contourné.",
                    priyom::MAX_RESULTS
                );
            }
            for tx in transmissions.iter().take(5) {
                println!(
                    "               {}  {}",
                    tx.start.format("%d/%m %H:%M UTC"),
                    tx.summary
                );
            }
        }
        Err(err) => println!("échec — {err:#}"),
    }
    println!();

    print!("Clé RTL-SDR: ");
    match sdr::usb::devices() {
        Ok(clés) if clés.is_empty() => println!("aucune sur le bus USB"),
        Ok(clés) => {
            println!("{} sur le bus USB", clés.len());
            for clé in &clés {
                println!("               {}", clé.label());
            }
            // Énumérer ne prouve rien : seule une ouverture réussie montre que le
            // pilote WinUSB (ou la règle udev) est en place.
            match sdr::rtlsdr::can_open(0) {
                Ok(nom) => println!("             pilote prêt, clé ouverte : {nom}"),
                Err(err) => {
                    println!("             ⚠ la clé ne s'ouvre pas — {err:#}");
                    println!(
                        "             → « wavecraft --installer-pilote » (Windows) ou \
                         packaging/linux/install.sh (Linux)"
                    );
                }
            }
        }
        Err(err) => println!("bus USB illisible — {err:#}"),
    }
    Ok(())
}
