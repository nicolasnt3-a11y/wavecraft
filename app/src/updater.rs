//! Vérification et installation des mises à jour depuis GitHub Releases.
//!
//! Au démarrage (en arrière-plan, sans jamais bloquer la fenêtre), Wavecraft
//! interroge l'API publique de GitHub pour savoir si une version plus récente
//! est publiée. Si l'utilisateur clique sur « Installer et redémarrer »,
//! l'exécutable de la nouvelle version est téléchargé, installé puis relancé
//! automatiquement.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// Endpoint GitHub Releases de la dernière version publiée.
pub const RELEASES_API_URL: &str =
    "https://api.github.com/repos/nicolasnt3-a11y/wavecraft/releases/latest";

/// Version courante embarquée à la compilation (ex. « 0.2.0 »).
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Informations sur une nouvelle version disponible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    /// Numéro propre sans préfixe « v », ex. « 0.3.0 ».
    pub version: String,
    /// Tag GitHub, ex. « v0.3.0 ».
    pub tag: String,
    /// Page web de la release sur GitHub.
    pub page_url: String,
    /// Nom du fichier téléchargeable retenu pour ce système.
    pub asset_name: Option<String>,
    /// URL directe de téléchargement du fichier retenu.
    pub download_url: Option<String>,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

/// Analyse un numéro de version sémantique (« v0.2.0 » ou « 0.2.0 ») en `(major, minor, patch)`.
pub fn parse_version(raw: &str) -> Option<(u32, u32, u32)> {
    let clean = raw.trim().trim_start_matches(['v', 'V']);
    let core = clean.split('-').next()?;
    let mut parts = core.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next().unwrap_or("0").parse().ok()?;
    let patch: u32 = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

/// Vrai si `candidate` est strictement plus récente que `current`.
pub fn is_newer(current: &str, candidate: &str) -> bool {
    match (parse_version(current), parse_version(candidate)) {
        (Some(cur), Some(cand)) => cand > cur,
        _ => false,
    }
}

/// Choisit l'actif de release adapté à la plateforme courante.
fn pick_asset(assets: &[GhAsset]) -> Option<(String, String)> {
    let preferred: &[&str] = if cfg!(target_os = "windows") {
        &["wavecraft-setup.exe", "wavecraft-windows-x64.zip"]
    } else if cfg!(feature = "desktop") {
        &["wavecraft-linux-x64.tar.gz"]
    } else {
        &["wavecraft-linux-x64-serveur.tar.gz"]
    };

    for wanted in preferred {
        if let Some(a) = assets.iter().find(|a| a.name.eq_ignore_ascii_case(wanted)) {
            return Some((a.name.clone(), a.browser_download_url.clone()));
        }
    }
    None
}

/// Interroge GitHub et renvoie `Some(ReleaseInfo)` si une version plus récente
/// que [`CURRENT_VERSION`] est disponible.
pub fn check_latest() -> Result<Option<ReleaseInfo>> {
    check_latest_from(RELEASES_API_URL, CURRENT_VERSION)
}

pub fn check_latest_from(api_url: &str, current_version: &str) -> Result<Option<ReleaseInfo>> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(6)))
        .build()
        .new_agent();

    let body = agent
        .get(api_url)
        .header("User-Agent", &format!("wavecraft/{CURRENT_VERSION}"))
        .header("Accept", "application/vnd.github+json")
        .call()
        .context("vérification des mises à jour sur GitHub")?
        .body_mut()
        .read_to_string()
        .context("lecture de la réponse GitHub")?;

    let release: GhRelease =
        serde_json::from_str(&body).context("réponse GitHub des versions illisible")?;

    if !is_newer(current_version, &release.tag_name) {
        return Ok(None);
    }

    let (asset_name, download_url) = match pick_asset(&release.assets) {
        Some((n, u)) => (Some(n), Some(u)),
        None => (None, None),
    };

    Ok(Some(ReleaseInfo {
        version: release.tag_name.trim_start_matches(['v', 'V']).to_string(),
        tag: release.tag_name,
        page_url: release.html_url,
        asset_name,
        download_url,
    }))
}

/// Télécharge la mise à jour, prépare l'installation et lance le redémarrage.
///
/// Une fois cette fonction terminée avec `Ok(())`, l'appelant doit fermer
/// proprement l'application courante afin que la nouvelle version prenne le relais.
pub fn download_and_install(release: &ReleaseInfo) -> Result<()> {
    let Some(url) = &release.download_url else {
        bail!("aucun paquet installable trouvé pour ce système ({})", release.page_url);
    };
    let asset_name = release
        .asset_name
        .as_deref()
        .unwrap_or("wavecraft-update");

    let temp_file = std::env::temp_dir().join(format!("wavecraft-update-{}-{asset_name}", release.tag));
    download_to(url, &temp_file)?;

    #[cfg(target_os = "windows")]
    {
        install_windows(&temp_file, asset_name)
    }
    #[cfg(not(target_os = "windows"))]
    {
        install_unix(&temp_file)
    }
}

fn download_to(url: &str, dest: &PathBuf) -> Result<()> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .new_agent();

    let mut response = agent
        .get(url)
        .header("User-Agent", &format!("wavecraft/{CURRENT_VERSION}"))
        .call()
        .with_context(|| format!("téléchargement de la mise à jour ({url})"))?;

    let mut file = std::fs::File::create(dest)
        .with_context(|| format!("création du fichier temporaire ({})", dest.display()))?;
    std::io::copy(&mut response.body_mut().as_reader(), &mut file)
        .context("écriture de la mise à jour téléchargée")?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn install_windows(downloaded: &PathBuf, asset_name: &str) -> Result<()> {
    let pid = std::process::id();
    let current_exe = std::env::current_exe().context("chemin de l'exécutable introuvable")?;

    // Si on a l'installateur autonome (`wavecraft-setup.exe`), on attend la fin
    // du processus courant, on l'exécute silencieusement (/S), puis on relance
    // Wavecraft depuis son dossier d'installation.
    let ps = if asset_name.to_ascii_lowercase().ends_with(".exe") {
        let installed_exe = std::env::var("LOCALAPPDATA")
            .map(|l| PathBuf::from(l).join("Programs").join("Wavecraft").join("wavecraft.exe"))
            .unwrap_or_else(|_| current_exe.clone());
        let launch = if current_exe == installed_exe || !current_exe.exists() {
            installed_exe
        } else {
            // Si Wavecraft tourne depuis l'emplacement installé ou ailleurs,
            // l'installateur met à jour %LOCALAPPDATA%\Programs\Wavecraft.
            installed_exe
        };
        format!(
            "Wait-Process -Id {pid} -ErrorAction SilentlyContinue; \
             Start-Sleep -Milliseconds 400; \
             Start-Process -FilePath '{setup}' -ArgumentList '/S' -Wait; \
             Remove-Item -LiteralPath '{setup}' -Force -ErrorAction SilentlyContinue; \
             if (Test-Path '{launch}') {{ Start-Process -FilePath '{launch}' }}",
            pid = pid,
            setup = downloaded.to_string_lossy().replace('\'', "''"),
            launch = launch.to_string_lossy().replace('\'', "''"),
        )
    } else {
        // Archive portable .zip : extraction sur place après fermeture puis relance.
        let dest_dir = current_exe
            .parent()
            .context("dossier de l'exécutable introuvable")?;
        format!(
            "Wait-Process -Id {pid} -ErrorAction SilentlyContinue; \
             Start-Sleep -Milliseconds 400; \
             $tmp = Join-Path $env:TEMP 'wavecraft-extract'; \
             Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue; \
             Expand-Archive -Path '{zip}' -DestinationPath $tmp -Force; \
             $bin = Get-ChildItem $tmp -Recurse -Filter 'wavecraft.exe' | Select-Object -First 1; \
             if ($bin) {{ Copy-Item (Join-Path $bin.DirectoryName '*') '{dest}' -Recurse -Force }}; \
             Remove-Item -Recurse -Force $tmp, '{zip}' -ErrorAction SilentlyContinue; \
             Start-Process -FilePath '{exe}'",
            pid = pid,
            zip = downloaded.to_string_lossy().replace('\'', "''"),
            dest = dest_dir.to_string_lossy().replace('\'', "''"),
            exe = current_exe.to_string_lossy().replace('\'', "''"),
        )
    };

    std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &ps,
        ])
        .spawn()
        .context("lancement du script de mise à jour Windows")?;

    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn install_unix(downloaded: &PathBuf) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let current_exe = std::env::current_exe().context("chemin de l'exécutable introuvable")?;
    let parent = current_exe
        .parent()
        .context("dossier de l'exécutable introuvable")?;

    let extract_dir = std::env::temp_dir().join("wavecraft-update-extract");
    let _ = std::fs::remove_dir_all(&extract_dir);
    std::fs::create_dir_all(&extract_dir)?;

    let status = std::process::Command::new("tar")
        .args(["-xzf"])
        .arg(downloaded)
        .arg("-C")
        .arg(&extract_dir)
        .status()
        .context("extraction de l'archive tar.gz")?;
    if !status.success() {
        bail!("échec de l'extraction de la mise à jour");
    }

    // Cherche le binaire `wavecraft` dans l'archive extraite.
    let new_bin = find_file(&extract_dir, "wavecraft")
        .context("exécutable « wavecraft » introuvable dans l'archive")?;
    let staged = parent.join(".wavecraft.new");
    std::fs::copy(&new_bin, &staged).with_context(|| {
        format!(
            "impossible d'écrire dans {} (permissions insuffisantes ?)",
            parent.display()
        )
    })?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(&staged, &current_exe).context("remplacement de l'exécutable")?;

    let _ = std::fs::remove_dir_all(&extract_dir);
    let _ = std::fs::remove_file(downloaded);

    // Relance la nouvelle version.
    std::process::Command::new(&current_exe)
        .spawn()
        .context("redémarrage de Wavecraft")?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn find_file(root: &std::path::Path, name: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.is_file() && path.file_name().and_then(|n| n.to_str()) == Some(name) {
            return Some(path);
        }
        if path.is_dir()
            && let Some(found) = find_file(&path, name)
        {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_versions_semantiques_se_comparent_correctement() {
        assert_eq!(parse_version("v0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_version("0.10.1"), Some((0, 10, 1)));
        assert_eq!(parse_version("v1.2.3-beta.1"), Some((1, 2, 3)));
        assert_eq!(parse_version("invalide"), None);

        assert!(is_newer("0.2.0", "v0.2.1"));
        assert!(is_newer("0.2.0", "v0.3.0"));
        assert!(is_newer("0.2.0", "v1.0.0"));
        assert!(is_newer("0.2.0", "v0.10.0"));
        assert!(!is_newer("0.2.0", "v0.2.0"));
        assert!(!is_newer("0.2.0", "v0.1.9"));
        assert!(!is_newer("0.2.0", "n'importe-quoi"));
    }

    #[test]
    fn le_bon_paquet_est_choisi_selon_le_systeme() {
        let assets = vec![
            GhAsset {
                name: "wavecraft-windows-x64.zip".into(),
                browser_download_url: "https://example.test/win.zip".into(),
            },
            GhAsset {
                name: "wavecraft-setup.exe".into(),
                browser_download_url: "https://example.test/setup.exe".into(),
            },
            GhAsset {
                name: "wavecraft-linux-x64.tar.gz".into(),
                browser_download_url: "https://example.test/linux.tar.gz".into(),
            },
            GhAsset {
                name: "wavecraft-linux-x64-serveur.tar.gz".into(),
                browser_download_url: "https://example.test/linux-srv.tar.gz".into(),
            },
        ];
        let picked = pick_asset(&assets).expect("un paquet doit être choisi");
        if cfg!(target_os = "windows") {
            assert_eq!(picked.0, "wavecraft-setup.exe");
        } else if cfg!(feature = "desktop") {
            assert_eq!(picked.0, "wavecraft-linux-x64.tar.gz");
        } else {
            assert_eq!(picked.0, "wavecraft-linux-x64-serveur.tar.gz");
        }
    }
}
