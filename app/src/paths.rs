//! Emplacements des fichiers de Wavecraft.
//!
//! Aucune dépendance système : les dossiers sont déduits des variables
//! d'environnement, ce qui garde le binaire léger et prévisible, y compris dans
//! un conteneur LXC sans session de bureau.

use std::path::PathBuf;

/// Nom du dossier de l'application.
const APP: &str = "wavecraft";

/// Dossier des réglages et du logbook.
///
/// - Windows : `%APPDATA%\wavecraft`
/// - Linux   : `$XDG_CONFIG_HOME/wavecraft`, sinon `~/.config/wavecraft`
/// - à défaut : `./wavecraft-data`, à côté de l'exécutable.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("WAVECRAFT_DATA_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(windows)]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join(APP);
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(xdg).join(APP);
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".config").join(APP);
        }
    }
    PathBuf::from("wavecraft-data")
}

/// Dossier des enregistrements audio.
pub fn recordings_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("WAVECRAFT_RECORDINGS_DIR") {
        return PathBuf::from(dir);
    }
    data_dir().join("enregistrements")
}

/// Dossier de l'exécutable : c'est là qu'est cherchée en priorité la
/// bibliothèque RTL-SDR livrée avec l'application.
pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
}

/// Crée le dossier s'il manque, et renvoie le chemin demandé.
pub fn ensure_parent(path: &std::path::Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Un seul test touche aux variables d'environnement : elles sont communes à
    // tout le processus, et les tests s'exécutent en parallèle.
    #[test]
    fn emplacement_des_donnees() {
        unsafe { std::env::remove_var("WAVECRAFT_DATA_DIR") };
        assert!(
            data_dir().ends_with(APP) || data_dir().ends_with("wavecraft-data"),
            "{}",
            data_dir().display()
        );

        // SAFETY: variable propre à Wavecraft, remise en état aussitôt.
        unsafe { std::env::set_var("WAVECRAFT_DATA_DIR", "C:/tmp/wavecraft-test") };
        assert_eq!(data_dir(), PathBuf::from("C:/tmp/wavecraft-test"));
        assert_eq!(
            recordings_dir(),
            PathBuf::from("C:/tmp/wavecraft-test").join("enregistrements")
        );
        unsafe { std::env::remove_var("WAVECRAFT_DATA_DIR") };
    }
}
