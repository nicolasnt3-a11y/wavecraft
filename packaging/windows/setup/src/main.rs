#![windows_subsystem = "windows"]

use std::env;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::process::Command;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_YESNO, MessageBoxW, SW_SHOW,
};

static PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.zip"));

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

fn message_box(texte: &str, titre: &str, flags: u32) -> i32 {
    let t = to_wide(texte);
    let h = to_wide(titre);
    unsafe { MessageBoxW(0 as HWND, t.as_ptr(), h.as_ptr(), flags) }
}

fn main() {
    let silent = env::args().any(|a| a == "/S" || a == "--silent" || a == "-s");

    if !silent {
        let question = "Bienvenue dans l'installation de WAVECRAFT.\n\n\
            Voulez-vous installer Wavecraft sur votre ordinateur ?\n\
            Un raccourci sera automatiquement créé sur votre Bureau.\n\n\
            Emplacement : %LOCALAPPDATA%\\Programs\\Wavecraft";
        let rep = message_box(question, "Installation de Wavecraft", MB_ICONQUESTION | MB_YESNO);
        if rep != IDYES {
            return;
        }
    }

    if PAYLOAD.is_empty() {
        message_box(
            "Erreur : l'archive d'installation est vide.",
            "Erreur d'installation",
            MB_ICONERROR,
        );
        return;
    }

    // 1. Écriture temporaire du zip
    let temp_zip = env::temp_dir().join("wavecraft-install-payload.zip");
    if let Err(e) = std::fs::write(&temp_zip, PAYLOAD) {
        message_box(
            &format!("Impossible d'extraire les fichiers temporaires :\n{e}"),
            "Erreur d'installation",
            MB_ICONERROR,
        );
        return;
    }

    // 2. Déploiement et création des raccourcis
    let local_app_data = env::var("LOCALAPPDATA").unwrap_or_else(|_| "C:\\".to_string());
    let dest = PathBuf::from(&local_app_data).join("Programs").join("Wavecraft");

    let ps_script = format!(
        r#"
$ErrorActionPreference = 'Stop'
$dest = '{dest}'
$tempZip = '{tempZip}'

New-Item -ItemType Directory -Path $dest -Force | Out-Null
Expand-Archive -Path $tempZip -DestinationPath $dest -Force

# Raccourci Bureau
$wsh = New-Object -ComObject WScript.Shell
$desktop = [Environment]::GetFolderPath('Desktop')
$scBureau = $wsh.CreateShortcut("$desktop\Wavecraft.lnk")
$scBureau.TargetPath = "$dest\wavecraft.exe"
$scBureau.WorkingDirectory = "$dest"
$scBureau.IconLocation = "$dest\wavecraft.ico,0"
$scBureau.Description = "Wavecraft - Récepteur d'ondes courtes pour stations de nombres"
$scBureau.Save()

# Raccourci Menu Démarrer
$programs = [Environment]::GetFolderPath('Programs')
$scMenu = $wsh.CreateShortcut("$programs\Wavecraft.lnk")
$scMenu.TargetPath = "$dest\wavecraft.exe"
$scMenu.WorkingDirectory = "$dest"
$scMenu.IconLocation = "$dest\wavecraft.ico,0"
$scMenu.Description = "Wavecraft - Récepteur d'ondes courtes pour stations de nombres"
$scMenu.Save()

# Inscription dans Ajout/Suppression de programmes Windows
$reg = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Wavecraft"
if (-not (Test-Path $reg)) {{ New-Item -Path $reg -Force | Out-Null }}
Set-ItemProperty -Path $reg -Name "DisplayName" -Value "Wavecraft"
Set-ItemProperty -Path $reg -Name "DisplayVersion" -Value "{version}"
Set-ItemProperty -Path $reg -Name "Publisher" -Value "Nicolas"
Set-ItemProperty -Path $reg -Name "DisplayIcon" -Value "$dest\wavecraft.ico"
Set-ItemProperty -Path $reg -Name "InstallLocation" -Value "$dest"
Set-ItemProperty -Path $reg -Name "UninstallString" -Value "`"$dest\desinstaller.cmd`""
Set-ItemProperty -Path $reg -Name "URLInfoAbout" -Value "https://nthill.be/wavecraft/"

# Script de désinstallation
$uninst = @"
@echo off
chcp 65001 >nul
echo Desinstallation de Wavecraft...
set "DEST=%LOCALAPPDATA%\Programs\Wavecraft"
set "DESKTOP=%USERPROFILE%\Desktop"
set "PROGRAMS=%APPDATA%\Microsoft\Windows\Start Menu\Programs"

if exist "%DESKTOP%\Wavecraft.lnk" del /f /q "%DESKTOP%\Wavecraft.lnk"
if exist "%PROGRAMS%\Wavecraft.lnk" del /f /q "%PROGRAMS%\Wavecraft.lnk"
reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Wavecraft" /f >nul 2>&1
echo Raccourcis supprimes.
echo Pour finaliser, supprimez le dossier : %DEST%
pause
"@
[System.IO.File]::WriteAllText("$dest\desinstaller.cmd", $uninst, [System.Text.Encoding]::UTF8)
"#,
        dest = dest.to_string_lossy().replace('\'', "''"),
        tempZip = temp_zip.to_string_lossy().replace('\'', "''"),
        version = env!("CARGO_PKG_VERSION"),
    );

    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &ps_script,
        ])
        .status();

    let _ = std::fs::remove_file(&temp_zip);

    match status {
        Ok(s) if s.success() => {
            if !silent {
                let reponse = message_box(
                    "Wavecraft a été installé avec succès !\n\n\
                    Une icône Wavecraft est désormais disponible sur votre Bureau.\n\n\
                    Voulez-vous lancer Wavecraft dès maintenant ?",
                    "Installation terminée",
                    MB_ICONINFORMATION | MB_YESNO,
                );
                if reponse == IDYES {
                    let exe_path = to_wide(&format!("{}\\wavecraft.exe", dest.display()));
                    let open_op = to_wide("open");
                    let dir_path = to_wide(&dest.to_string_lossy());
                    unsafe {
                        ShellExecuteW(
                            0 as HWND,
                            open_op.as_ptr(),
                            exe_path.as_ptr(),
                            std::ptr::null(),
                            dir_path.as_ptr(),
                            SW_SHOW,
                        );
                    }
                }
            }
        }
        _ => {
            message_box(
                "L'installation a rencontré une difficulté lors de l'extraction des fichiers.",
                "Erreur d'installation",
                MB_ICONERROR,
            );
        }
    }
}
