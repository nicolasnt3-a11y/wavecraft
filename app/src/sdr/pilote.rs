//! Installation du pilote de la clé, sans intervention de l'utilisateur.
//!
//! Sous Windows, une clé RTL-SDR neuve est prise par le pilote de télévision
//! numérique de Realtek, ou n'a pas de pilote du tout. La bibliothèque
//! `librtlsdr` exige WinUSB. L'outil de référence pour l'installer est Zadig
//! (libwdi, GPL-3), fourni avec Wavecraft — mais Zadig n'a pas de ligne de
//! commande : il faut y choisir la clé dans une liste et cliquer. C'est ce que
//! fait ce module à la place de l'utilisateur, par messages Win32, après une
//! seule demande d'autorisation de Windows.
//!
//! Le déroulé, tel que le ferait une main humaine :
//!
//! 1. relancer Wavecraft avec les droits d'administration (`--installer-pilote`) ;
//! 2. lancer Zadig avec un fichier `zadig.ini` qui liste tous les appareils ;
//! 3. parcourir la liste, retenir l'entrée dont l'identifiant USB est celui
//!    d'un RTL2832U (0BDA:2838 ou 0BDA:2832), de préférence l'appareil entier
//!    plutôt qu'une de ses interfaces ;
//! 4. vérifier que la cible est WinUSB, puis « Replace Driver » ;
//! 5. fermer le message de réussite, laisser Zadig se fermer, et prouver le
//!    résultat en ouvrant la clé.
//!
//! Chaque étape est consignée dans `pilote.log`, dans le dossier de données.
//! En cas d'échec, Zadig reste ouvert pour que l'utilisateur termine à la main.
//!
//! Sous Linux, la question ne se pose pas : la règle udev et la liste noire du
//! pilote TNT sont posées par `packaging/linux/install.sh`.

use anyhow::Result;

/// Identifiants USB des clés reconnues : RTL2838 (la grande majorité, dont la
/// RTL-SDR Blog V4) et RTL2832.
pub const VENDEUR: u16 = 0x0bda;
pub const PRODUITS: [u16; 2] = [0x2838, 0x2832];

/// Installe le pilote, en demandant l'élévation à Windows si nécessaire.
///
/// Bloque jusqu'à la fin ; à appeler depuis un fil secondaire.
#[cfg(windows)]
pub fn demander() -> Result<()> {
    if windows::est_administrateur() {
        return windows::executer();
    }
    windows::relancer_eleve()
}

/// Mode `--installer-pilote` : le travail lui-même, une fois élevé.
#[cfg(windows)]
pub fn executer_depuis_la_ligne_de_commande() -> Result<()> {
    println!("Wavecraft — installation du pilote WinUSB");
    demander()
}

#[cfg(not(windows))]
pub fn demander() -> Result<()> {
    anyhow::bail!(
        "l'installation automatique du pilote ne concerne que Windows ; sous Linux, \
         lancez packaging/linux/install.sh (règle udev et liste noire du pilote TNT)"
    )
}

#[cfg(not(windows))]
pub fn executer_depuis_la_ligne_de_commande() -> Result<()> {
    demander()
}

/// Cherche Zadig à côté de l'exécutable (paquet livré), puis dans le dépôt.
pub fn chercher_zadig() -> Option<std::path::PathBuf> {
    let mut dossiers = Vec::new();
    if let Some(dir) = crate::paths::exe_dir() {
        dossiers.push(dir.join("pilote"));
        dossiers.push(dir.clone());
        // Depuis `cargo run` : app/target/release → vendor/zadig.
        dossiers.push(dir.join("..").join("..").join("..").join("vendor").join("zadig"));
    }
    if let Ok(racine) = std::env::current_dir() {
        dossiers.push(racine.join("vendor").join("zadig"));
        dossiers.push(racine.join("..").join("vendor").join("zadig"));
    }
    for dossier in dossiers {
        let Ok(entrées) = std::fs::read_dir(&dossier) else {
            continue;
        };
        for entrée in entrées.flatten() {
            let nom = entrée.file_name().to_string_lossy().to_lowercase();
            if nom.starts_with("zadig") && nom.ends_with(".exe") {
                return Some(entrée.path());
            }
        }
    }
    None
}

/// Réglages donnés à Zadig : tout lister, se fermer une fois le pilote posé.
pub const ZADIG_INI: &str = "# Écrit par Wavecraft : Zadig est piloté automatiquement.\n\
[general]\n\
  advanced_mode = false\n\
  exit_on_success = true\n\
  log_level = 0\n\
[device]\n\
  list_all = true\n\
  include_hubs = false\n\
  trim_whitespaces = true\n\
[driver]\n\
  default_driver = 0\n\
  extract_only = false\n";

/// L'entrée de la liste de Zadig à retenir, parmi celles qui portent
/// l'identifiant d'un RTL2832U.
///
/// Zadig nomme les interfaces « … (Interface n) ». Quand un pilote de
/// télévision tient l'appareil entier, c'est lui qu'il faut remplacer ; sinon,
/// `librtlsdr` ne parle qu'à l'interface 0.
pub fn preference(nom: &str, vid: &str, pid: &str) -> Option<u8> {
    let vid = u16::from_str_radix(vid.trim(), 16).ok()?;
    let pid = u16::from_str_radix(pid.trim(), 16).ok()?;
    if vid != VENDEUR || !PRODUITS.contains(&pid) {
        return None;
    }
    let minuscules = nom.to_lowercase();
    Some(if !minuscules.contains("(interface") {
        3
    } else if minuscules.contains("(interface 0)") {
        2
    } else {
        1
    })
}

#[cfg(windows)]
mod windows {
    use std::io::Write;
    use std::time::{Duration, Instant};

    use anyhow::{Context, Result, anyhow, bail};
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, HANDLE_FLAG_INHERIT, HWND, INVALID_HANDLE_VALUE, LPARAM,
        SetHandleInformation,
    };
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, INFINITE, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::Shell::{
        IsUserAnAdmin, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
        ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumChildWindows, EnumWindows, GetDlgItem, GetWindowTextW, GetWindowThreadProcessId,
        IsWindowVisible, PostMessageW, SW_SHOWNORMAL, SendMessageW,
    };

    // Messages Win32 utilisés ; les valeurs sont celles de <winuser.h>.
    const WM_CLOSE: u32 = 0x0010;
    const WM_GETTEXT: u32 = 0x000D;
    const WM_COMMAND: u32 = 0x0111;
    const BM_CLICK: u32 = 0x00F5;
    const CB_GETCOUNT: u32 = 0x0146;
    const CB_GETLBTEXT: u32 = 0x0148;
    const CB_SETCURSEL: u32 = 0x014E;
    const BN_CLICKED: usize = 0;
    const CBN_SELCHANGE: usize = 1;
    const ERROR_CANCELLED: u32 = 1223;

    // Identifiants des contrôles du dialogue de Zadig 2.x (resource.h de libwdi).
    const IDC_DEVICELIST: i32 = 1001;
    const IDC_VID: i32 = 1002;
    const IDC_PID: i32 = 1003;
    const IDC_STATUS: i32 = 1006;
    const IDC_DRIVER: i32 = 1008;
    const IDC_INSTALL: i32 = 1009;
    const IDC_TARGET: i32 = 1011;
    const IDOK: i32 = 1;

    /// Journal de l'installation : à l'écran et dans `pilote.log`.
    struct Journal {
        fichier: Option<std::fs::File>,
    }

    impl Journal {
        fn ouvrir() -> Self {
            let chemin = crate::paths::data_dir().join("pilote.log");
            let fichier = crate::paths::ensure_parent(&chemin)
                .and_then(|_| std::fs::File::create(&chemin))
                .ok();
            Self { fichier }
        }

        fn note(&mut self, texte: impl AsRef<str>) {
            let ligne = format!(
                "{} {}",
                chrono::Local::now().format("%H:%M:%S"),
                texte.as_ref()
            );
            println!("{ligne}");
            if let Some(f) = self.fichier.as_mut() {
                let _ = writeln!(f, "{ligne}");
            }
        }
    }

    pub fn est_administrateur() -> bool {
        unsafe { IsUserAnAdmin() != 0 }
    }

    fn utf16(texte: &str) -> Vec<u16> {
        texte.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Relance Wavecraft lui-même avec les droits d'administration et attend
    /// la fin de l'installation.
    pub fn relancer_eleve() -> Result<()> {
        let exe = std::env::current_exe().context("chemin de l'exécutable inconnu")?;
        let fichier = utf16(&exe.to_string_lossy());
        let verbe = utf16("runas");
        let arguments = utf16("--installer-pilote");
        let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
        info.lpVerb = verbe.as_ptr();
        info.lpFile = fichier.as_ptr();
        info.lpParameters = arguments.as_ptr();
        info.nShow = SW_SHOWNORMAL;
        let lancé = unsafe { ShellExecuteExW(&mut info) };
        if lancé == 0 {
            let code = unsafe { GetLastError() };
            if code == ERROR_CANCELLED {
                bail!("autorisation refusée : Windows n'a pas laissé installer le pilote");
            }
            bail!("impossible de relancer Wavecraft avec les droits d'administration (code {code})");
        }
        if info.hProcess.is_null() {
            bail!("Windows n'a pas rendu la main sur l'installation");
        }
        let code = unsafe {
            WaitForSingleObject(info.hProcess, INFINITE);
            let mut code = 1u32;
            GetExitCodeProcess(info.hProcess, &mut code);
            CloseHandle(info.hProcess);
            code
        };
        if code == 0 {
            Ok(())
        } else {
            let journal = crate::paths::data_dir().join("pilote.log");
            let dernière = std::fs::read_to_string(&journal)
                .ok()
                .and_then(|s| s.lines().last().map(|l| l.to_string()))
                .unwrap_or_default();
            bail!(
                "l'installation du pilote a échoué ({}) — détail dans {}",
                dernière.trim(),
                journal.display()
            )
        }
    }

    /// Texte d'une fenêtre ou d'un contrôle d'un autre processus.
    fn texte(h: HWND) -> String {
        if h.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 2048];
        let n = unsafe { SendMessageW(h, WM_GETTEXT, buf.len(), buf.as_mut_ptr() as LPARAM) };
        let n = (n.max(0) as usize).min(buf.len());
        String::from_utf16_lossy(&buf[..n])
    }

    fn titre(h: HWND) -> String {
        let mut buf = [0u16; 512];
        let n = unsafe { GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32) };
        let n = (n.max(0) as usize).min(buf.len());
        String::from_utf16_lossy(&buf[..n])
    }

    struct Collecte {
        pid: u32,
        fenêtres: Vec<HWND>,
    }

    unsafe extern "system" fn rappel_fenetres(h: HWND, l: LPARAM) -> i32 {
        let collecte = unsafe { &mut *(l as *mut Collecte) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, &mut pid) };
        if pid == collecte.pid && unsafe { IsWindowVisible(h) } != 0 {
            collecte.fenêtres.push(h);
        }
        1
    }

    unsafe extern "system" fn rappel_enfants(h: HWND, l: LPARAM) -> i32 {
        let liste = unsafe { &mut *(l as *mut Vec<HWND>) };
        liste.push(h);
        1
    }

    /// Fenêtres visibles de premier niveau d'un processus.
    fn fenetres(pid: u32) -> Vec<HWND> {
        let mut collecte = Collecte {
            pid,
            fenêtres: Vec::new(),
        };
        unsafe { EnumWindows(Some(rappel_fenetres), &mut collecte as *mut Collecte as LPARAM) };
        collecte.fenêtres
    }

    /// Textes de tous les contrôles d'une fenêtre, bout à bout.
    fn contenu(h: HWND) -> String {
        let mut enfants: Vec<HWND> = Vec::new();
        unsafe { EnumChildWindows(h, Some(rappel_enfants), &mut enfants as *mut Vec<HWND> as LPARAM) };
        enfants
            .iter()
            .map(|&e| texte(e))
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn commande(dialogue: HWND, id: i32, code: usize, controle: HWND) {
        let wparam = (id as usize & 0xFFFF) | (code << 16);
        unsafe { PostMessageW(dialogue, WM_COMMAND, wparam, controle as LPARAM) };
    }

    fn libelle_item(combo: HWND, index: usize) -> String {
        let mut buf = [0u16; 1024];
        let n = unsafe { SendMessageW(combo, CB_GETLBTEXT, index, buf.as_mut_ptr() as LPARAM) };
        let n = (n.max(0) as usize).min(buf.len());
        String::from_utf16_lossy(&buf[..n])
    }

    fn selectionner(dialogue: HWND, combo: HWND, index: usize) {
        unsafe { SendMessageW(combo, CB_SETCURSEL, index, 0) };
        commande(dialogue, IDC_DEVICELIST, CBN_SELCHANGE, combo);
        std::thread::sleep(Duration::from_millis(150));
    }

    /// Le travail, une fois administrateur : Zadig est lancé puis piloté.
    pub fn executer() -> Result<()> {
        let mut journal = Journal::ouvrir();
        let résultat = piloter(&mut journal);
        match &résultat {
            Ok(nom) => journal.note(format!("réussi : la clé s'ouvre ({nom})")),
            Err(err) => journal.note(format!("échec : {err:#}")),
        }
        résultat.map(|_| ())
    }

    fn piloter(journal: &mut Journal) -> Result<String> {
        if let Ok(nom) = crate::sdr::rtlsdr::can_open(0) {
            journal.note(format!("la clé s'ouvre déjà ({nom}) : rien à installer"));
            return Ok(nom);
        }
        let zadig = super::chercher_zadig().ok_or_else(|| {
            anyhow!("Zadig est introuvable à côté de l'application (dossier « pilote »)")
        })?;
        journal.note(format!("Zadig : {}", zadig.display()));
        if let Some(dossier) = zadig.parent() {
            match std::fs::write(dossier.join("zadig.ini"), super::ZADIG_INI) {
                Ok(()) => journal.note("zadig.ini écrit"),
                Err(err) => journal.note(format!("zadig.ini non écrit ({err}) : on continue")),
            }
        }

        // Un Zadig déjà ouvert refuserait une seconde instance : on le prie de
        // se fermer, et « /W » laisse au nouveau quinze secondes pour prendre
        // sa place.
        fermer_les_zadig_ouverts(journal);
        // Ses sorties sont détachées : sinon, qui lit celle de Wavecraft attend
        // aussi la fin de Zadig.
        // Zadig lit `zadig.ini` dans son répertoire courant : c'est son dossier.
        // Et Wavecraft ne lui lègue pas ses descripteurs (journal, tube) : sinon,
        // qui attend la sortie de Wavecraft attendrait aussi la fin de Zadig.
        ne_pas_leguer_les_descripteurs();
        let mut enfant = std::process::Command::new(&zadig)
            .current_dir(zadig.parent().unwrap_or(std::path::Path::new(".")))
            .arg("/W")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .with_context(|| format!("lancement de {}", zadig.display()))?;
        let pid = enfant.id();
        journal.note(format!("Zadig lancé (pid {pid})"));

        // La fenêtre principale est celle qui porte la liste d'appareils ; un
        // message de Zadig peut avoir le même titre qu'elle. Toute autre fenêtre
        // vue en attendant est consignée : c'est elle qui explique un blocage.
        let mut vus: Vec<String> = Vec::new();
        let dialogue = attendre(Duration::from_secs(90), || {
            for h in fenetres(pid) {
                if !unsafe { GetDlgItem(h, IDC_DEVICELIST) }.is_null() {
                    return Some(h);
                }
                signaler(journal, &mut vus, h);
            }
            None
        })
        .ok_or_else(|| anyhow!("la fenêtre de Zadig n'est pas apparue"))?;
        journal.note(format!("fenêtre de Zadig trouvée : « {} »", titre(dialogue)));
        let combo = unsafe { GetDlgItem(dialogue, IDC_DEVICELIST) };
        let compte = attendre(Duration::from_secs(90), || {
            let compte = unsafe { SendMessageW(combo, CB_GETCOUNT, 0, 0) }.max(0) as usize;
            for h in fenetres(pid) {
                if h != dialogue {
                    signaler(journal, &mut vus, h);
                }
            }
            (compte > 0).then_some(compte)
        })
        .ok_or_else(|| {
            anyhow!(
                "Zadig ne liste aucun appareil USB (état : « {} »)",
                texte(unsafe { GetDlgItem(dialogue, IDC_STATUS) }).trim()
            )
        })?;
        journal.note(format!("{compte} appareils listés par Zadig"));

        let mut meilleur: Option<(u8, usize, String)> = None;
        for index in 0..compte {
            selectionner(dialogue, combo, index);
            let nom = libelle_item(combo, index);
            let vid = texte(unsafe { GetDlgItem(dialogue, IDC_VID) });
            let pid_usb = texte(unsafe { GetDlgItem(dialogue, IDC_PID) });
            let courant = texte(unsafe { GetDlgItem(dialogue, IDC_DRIVER) });
            journal.note(format!("  {index:2} {vid}:{pid_usb} {nom} — pilote actuel : {courant}"));
            if let Some(score) = super::preference(&nom, &vid, &pid_usb)
                && meilleur.as_ref().is_none_or(|m| score > m.0)
            {
                meilleur = Some((score, index, nom));
            }
        }
        let Some((_, index, nom)) = meilleur else {
            bail!("aucune clé RTL-SDR (0BDA:2838 ou 0BDA:2832) dans la liste de Zadig");
        };
        selectionner(dialogue, combo, index);
        journal.note(format!("clé retenue : {nom}"));

        // Dernières vérifications avant de cliquer : la bonne clé, WinUSB en cible.
        let vid = texte(unsafe { GetDlgItem(dialogue, IDC_VID) });
        let pid_usb = texte(unsafe { GetDlgItem(dialogue, IDC_PID) });
        let courant = texte(unsafe { GetDlgItem(dialogue, IDC_DRIVER) });
        let cible = texte(unsafe { GetDlgItem(dialogue, IDC_TARGET) });
        let bouton = unsafe { GetDlgItem(dialogue, IDC_INSTALL) };
        let libellé = texte(bouton);
        journal.note(format!("sélection : {vid}:{pid_usb} · actuel {courant} · cible {cible} · bouton « {libellé} »"));
        if super::preference(&nom, &vid, &pid_usb).is_none() {
            bail!("la sélection de Zadig a changé en cours de route");
        }
        if !cible.starts_with("WinUSB") {
            bail!("la cible de Zadig n'est pas WinUSB ({cible})");
        }
        if !["Install Driver", "Replace Driver", "Reinstall Driver"].contains(&libellé.as_str()) {
            bail!("bouton inattendu dans Zadig : « {libellé} »");
        }

        journal.note("installation demandée à Zadig");
        commande(dialogue, IDC_INSTALL, BN_CLICKED, bouton);

        // Zadig travaille une à deux minutes (certificat, catalogue, pose), puis
        // affiche le résultat dans une boîte à part.
        let début = Instant::now();
        let mut réussi = false;
        while début.elapsed() < Duration::from_secs(300) {
            std::thread::sleep(Duration::from_millis(500));
            if let Ok(Some(_)) = enfant.try_wait() {
                journal.note("Zadig s'est fermé");
                break;
            }
            for fenêtre in fenetres(pid) {
                if fenêtre == dialogue {
                    continue;
                }
                let contenu_ = contenu(fenêtre);
                signaler(journal, &mut vus, fenêtre);
                let minuscules = contenu_.to_lowercase();
                if minuscules.contains("success") {
                    réussi = true;
                    let ok = unsafe { GetDlgItem(fenêtre, IDOK) };
                    if !ok.is_null() {
                        unsafe { PostMessageW(ok, BM_CLICK, 0, 0) };
                    }
                } else if minuscules.contains("fail") || minuscules.contains("error") {
                    bail!("Zadig signale un échec : {contenu_} (Zadig est resté ouvert)");
                }
            }
            if réussi {
                break;
            }
        }
        if !réussi {
            bail!("Zadig n'a pas confirmé l'installation en cinq minutes (il est resté ouvert)");
        }

        // Zadig se ferme tout seul (exit_on_success) ; on l'y aide au besoin.
        if attendre(Duration::from_secs(10), || enfant.try_wait().ok().flatten()).is_none() {
            unsafe { PostMessageW(dialogue, WM_CLOSE, 0, 0) };
            let _ = enfant.wait();
        }

        // La preuve : la clé s'ouvre. Windows met quelques secondes à la réénumérer.
        attendre(Duration::from_secs(20), || crate::sdr::rtlsdr::can_open(0).ok())
            .ok_or_else(|| anyhow!("le pilote est posé mais la clé ne s'ouvre toujours pas : débranchez-la et rebranchez-la"))
    }

    /// Retire le droit d'héritage aux descripteurs standard du processus.
    fn ne_pas_leguer_les_descripteurs() {
        for quel in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            let h = unsafe { GetStdHandle(quel) };
            if !h.is_null() && h != INVALID_HANDLE_VALUE {
                unsafe { SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0) };
            }
        }
    }

    /// Consigne une fenêtre de Zadig (titre et textes) la première fois qu'on la voit.
    fn signaler(journal: &mut Journal, vus: &mut Vec<String>, fenêtre: HWND) {
        let clé = format!("{} : {}", titre(fenêtre), contenu(fenêtre));
        if !vus.contains(&clé) {
            journal.note(format!("fenêtre « {clé} »"));
            vus.push(clé);
        }
    }

    unsafe extern "system" fn rappel_zadig(h: HWND, l: LPARAM) -> i32 {
        let liste = unsafe { &mut *(l as *mut Vec<HWND>) };
        if unsafe { IsWindowVisible(h) } != 0 && titre(h) == "Zadig" {
            liste.push(h);
        }
        1
    }

    /// Demande aux fenêtres Zadig déjà ouvertes de se fermer.
    fn fermer_les_zadig_ouverts(journal: &mut Journal) {
        let mut liste: Vec<HWND> = Vec::new();
        unsafe { EnumWindows(Some(rappel_zadig), &mut liste as *mut Vec<HWND> as LPARAM) };
        for h in liste {
            journal.note("un Zadig était déjà ouvert : fermeture demandée");
            unsafe { PostMessageW(h, WM_CLOSE, 0, 0) };
        }
    }

    /// Répète `essai` toutes les 250 ms jusqu'à obtenir une valeur ou expirer.
    fn attendre<T>(délai: Duration, mut essai: impl FnMut() -> Option<T>) -> Option<T> {
        let début = Instant::now();
        loop {
            if let Some(valeur) = essai() {
                return Some(valeur);
            }
            if début.elapsed() > délai {
                return None;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l_appareil_entier_passe_avant_ses_interfaces() {
        assert_eq!(preference("Blog V4", "0BDA", "2838"), Some(3));
        assert_eq!(preference("Bulk-In, Interface (Interface 0)", "0BDA", "2838"), Some(2));
        assert_eq!(preference("Bulk-In, Interface (Interface 1)", "0BDA", "2838"), Some(1));
        assert_eq!(preference("RTL2838UHIDIR", "0bda", "2832"), Some(3));
    }

    #[test]
    fn les_autres_appareils_sont_ignores() {
        // Un récepteur de clavier ne doit jamais recevoir WinUSB.
        assert_eq!(preference("USB Receiver (Interface 0)", "046D", "C52B"), None);
        assert_eq!(preference("Blog V4", "", ""), None);
        assert_eq!(preference("Blog V4", "0BDA", "2839"), None);
    }

    #[test]
    fn zadig_se_ferme_et_liste_tout() {
        assert!(ZADIG_INI.contains("exit_on_success = true"));
        assert!(ZADIG_INI.contains("list_all = true"));
        assert!(ZADIG_INI.contains("default_driver = 0"));
    }
}
