use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let payload_zip = out_dir.join("payload.zip");

    let racine = Path::new("../../..");
    let staging = out_dir.join("staging");

    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    std::fs::create_dir_all(staging.join("pilote")).unwrap();

    // Copie des composants
    let exe = racine.join("app/target/release/wavecraft.exe");
    if exe.exists() {
        std::fs::copy(&exe, staging.join("wavecraft.exe")).unwrap();
    } else {
        // En phase de dev/check, fichier factice si non encore compilé
        std::fs::write(staging.join("wavecraft.exe"), b"WAVECRAFT").unwrap();
    }

    let dll_candidates = [
        racine.join("wavecraft-windows-x64/rtlsdr.dll"),
        racine.join("vendor/rtl-sdr-blog/x64/rtlsdr.dll"),
        racine.join("tiers/rtlsdr/x64/rtlsdr.dll"),
        racine.join("tiers/rtlsdr/rtlsdr.dll"),
    ];
    for d in dll_candidates {
        if d.exists() {
            let _ = std::fs::copy(&d, staging.join("rtlsdr.dll"));
            break;
        }
    }

    let ico_candidates = [
        racine.join("wavecraft-windows-x64/wavecraft.ico"),
        racine.join("site/assets/img/wavecraft.ico"),
        racine.join("app/assets/wavecraft.ico"),
    ];
    for i in ico_candidates {
        if i.exists() {
            let _ = std::fs::copy(&i, staging.join("wavecraft.ico"));
            break;
        }
    }

    let zadig_candidates = [
        racine.join("wavecraft-windows-x64/pilote/zadig.exe"),
        racine.join("vendor/zadig/zadig-2.9.exe"),
        racine.join("tiers/zadig.exe"),
    ];
    for z in zadig_candidates {
        if z.exists() {
            let _ = std::fs::copy(&z, staging.join("pilote/zadig.exe"));
            break;
        }
    }

    let zadig_ini_candidates = [
        racine.join("wavecraft-windows-x64/pilote/zadig.ini"),
        racine.join("vendor/zadig/zadig.ini"),
    ];
    for zi in zadig_ini_candidates {
        if zi.exists() {
            let _ = std::fs::copy(&zi, staging.join("pilote/zadig.ini"));
            break;
        }
    }

    for doc in ["README.md", "LICENSE", "THIRD_PARTY.md"] {
        let doc_path = racine.join(doc);
        if doc_path.exists() {
            std::fs::copy(&doc_path, staging.join(doc)).unwrap();
        }
    }

    if payload_zip.exists() {
        let _ = std::fs::remove_file(&payload_zip);
    }

    // Compression du payload en zip via PowerShell
    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "Compress-Archive -Path '{}/*' -DestinationPath '{}' -Force",
                staging.display(),
                payload_zip.display()
            ),
        ])
        .status();

    match status {
        Ok(s) if s.success() => {}
        _ => {
            // Repli : créer un fichier vide si échec
            if !payload_zip.exists() {
                std::fs::write(&payload_zip, b"").unwrap();
            }
        }
    }
}
