//! Clé RTL-SDR, pilotée par la bibliothèque `librtlsdr`.
//!
//! La bibliothèque est chargée à l'exécution, pas à la compilation : Wavecraft
//! se compile et se teste sans elle, et la clé reste facultative.
//!
//! L'ordre de recherche place le dossier de l'exécutable en premier, car les
//! paquets Wavecraft livrent la bibliothèque **RTL-SDR Blog**. C'est la seule
//! qui gère correctement la V4 : celle fournie par les distributions ignore son
//! tuner R828D et ne reçoit rien en dessous de 28 MHz.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use libloading::{Library, Symbol};

use super::{Gain, Source};

/// Noms de la bibliothèque, selon le système.
#[cfg(windows)]
const LIB_NAMES: &[&str] = &["rtlsdr.dll", "librtlsdr.dll"];
#[cfg(target_os = "macos")]
const LIB_NAMES: &[&str] = &["librtlsdr.dylib", "librtlsdr.0.dylib"];
#[cfg(all(unix, not(target_os = "macos")))]
const LIB_NAMES: &[&str] = &["librtlsdr.so", "librtlsdr.so.0", "librtlsdr.so.2"];

type Dev = *mut c_void;

/// Points d'entrée utilisés de `librtlsdr`.
struct Api {
    _lib: Library,
    get_device_count: unsafe extern "C" fn() -> u32,
    get_device_name: unsafe extern "C" fn(u32) -> *const c_char,
    open: unsafe extern "C" fn(*mut Dev, u32) -> c_int,
    close: unsafe extern "C" fn(Dev) -> c_int,
    set_center_freq: unsafe extern "C" fn(Dev, u32) -> c_int,
    get_center_freq: unsafe extern "C" fn(Dev) -> u32,
    set_sample_rate: unsafe extern "C" fn(Dev, u32) -> c_int,
    get_sample_rate: unsafe extern "C" fn(Dev) -> u32,
    set_tuner_gain_mode: unsafe extern "C" fn(Dev, c_int) -> c_int,
    set_tuner_gain: unsafe extern "C" fn(Dev, c_int) -> c_int,
    get_tuner_gains: unsafe extern "C" fn(Dev, *mut c_int) -> c_int,
    set_freq_correction: unsafe extern "C" fn(Dev, c_int) -> c_int,
    set_agc_mode: unsafe extern "C" fn(Dev, c_int) -> c_int,
    set_bias_tee: Option<unsafe extern "C" fn(Dev, c_int) -> c_int>,
    reset_buffer: unsafe extern "C" fn(Dev) -> c_int,
    read_sync: unsafe extern "C" fn(Dev, *mut c_void, c_int, *mut c_int) -> c_int,
}

macro_rules! symbol {
    ($lib:expr, $name:literal) => {{
        let symbol: Symbol<_> = unsafe { $lib.get(concat!($name, "\0").as_bytes()) }
            .with_context(|| format!("fonction {} absente de la bibliothèque", $name))?;
        *symbol
    }};
}

impl Api {
    /// Charge la bibliothèque, en cherchant d'abord à côté de l'exécutable.
    fn load() -> Result<Self> {
        let mut tried = Vec::new();
        for candidate in candidates() {
            match unsafe { Library::new(&candidate) } {
                Ok(lib) => return Self::bind(lib),
                Err(err) => tried.push(format!("  {} — {err}", candidate.display())),
            }
        }
        bail!(
            "bibliothèque RTL-SDR introuvable. Emplacements essayés :\n{}\n\
             Placez « {} » à côté de wavecraft, ou installez le paquet rtl-sdr.",
            tried.join("\n"),
            LIB_NAMES[0]
        )
    }

    fn bind(lib: Library) -> Result<Self> {
        // `rtlsdr_set_bias_tee` manque dans les versions anciennes : son absence
        // ne doit pas empêcher d'écouter.
        let set_bias_tee = unsafe {
            lib.get::<unsafe extern "C" fn(Dev, c_int) -> c_int>(b"rtlsdr_set_bias_tee\0")
                .ok()
                .map(|s| *s)
        };
        Ok(Self {
            get_device_count: symbol!(lib, "rtlsdr_get_device_count"),
            get_device_name: symbol!(lib, "rtlsdr_get_device_name"),
            open: symbol!(lib, "rtlsdr_open"),
            close: symbol!(lib, "rtlsdr_close"),
            set_center_freq: symbol!(lib, "rtlsdr_set_center_freq"),
            get_center_freq: symbol!(lib, "rtlsdr_get_center_freq"),
            set_sample_rate: symbol!(lib, "rtlsdr_set_sample_rate"),
            get_sample_rate: symbol!(lib, "rtlsdr_get_sample_rate"),
            set_tuner_gain_mode: symbol!(lib, "rtlsdr_set_tuner_gain_mode"),
            set_tuner_gain: symbol!(lib, "rtlsdr_set_tuner_gain"),
            get_tuner_gains: symbol!(lib, "rtlsdr_get_tuner_gains"),
            set_freq_correction: symbol!(lib, "rtlsdr_set_freq_correction"),
            set_agc_mode: symbol!(lib, "rtlsdr_set_agc_mode"),
            set_bias_tee,
            reset_buffer: symbol!(lib, "rtlsdr_reset_buffer"),
            read_sync: symbol!(lib, "rtlsdr_read_sync"),
            _lib: lib,
        })
    }
}

/// Emplacements où chercher la bibliothèque, dans l'ordre.
fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(explicit) = std::env::var("WAVECRAFT_RTLSDR_LIB") {
        out.push(PathBuf::from(explicit));
    }
    if let Some(dir) = crate::paths::exe_dir() {
        for name in LIB_NAMES {
            out.push(dir.join(name));
        }
    }
    // Enfin, la recherche habituelle du système.
    for name in LIB_NAMES {
        out.push(PathBuf::from(name));
    }
    out
}



/// Essaie d'ouvrir la clé, puis la referme aussitôt.
///
/// La bibliothèque énumère les clés par leurs identifiants USB, même quand le
/// pilote n'est pas le bon : seule une ouverture réussie prouve que WinUSB (ou
/// la règle udev) est en place. C'est ce qu'attend l'assistant.
pub fn can_open(index: u32) -> Result<String> {
    RtlSdr::open(index).map(|clé| clé.name.clone())
}

/// Une clé RTL-SDR ouverte.
pub struct RtlSdr {
    api: Api,
    device: Dev,
    name: String,
    sample_rate: u32,
    center_freq: u32,
}

// `librtlsdr` ne permet qu'un seul fil à la fois sur un appareil donné ; la clé
// est utilisée par le seul fil de lecture du moteur.
unsafe impl Send for RtlSdr {}

impl RtlSdr {
    /// Ouvre la clé d'indice `index`.
    pub fn open(index: u32) -> Result<Self> {
        let api = Api::load()?;
        let count = unsafe { (api.get_device_count)() };
        if count == 0 {
            bail!(
                "aucune clé RTL-SDR détectée. Vérifiez le branchement ; sous \
                 Windows, l'assistant de Wavecraft installe le pilote WinUSB."
            );
        }
        if index >= count {
            bail!("clé n° {index} demandée, mais {count} clé(s) branchée(s)");
        }

        let name = unsafe {
            let raw = (api.get_device_name)(index);
            if raw.is_null() {
                format!("clé {index}")
            } else {
                CStr::from_ptr(raw).to_string_lossy().into_owned()
            }
        };

        let mut device: Dev = std::ptr::null_mut();
        let code = unsafe { (api.open)(&mut device, index) };
        if code != 0 || device.is_null() {
            bail!(
                "ouverture de la clé n° {index} impossible (code {code}). \
                 Sous Windows, le pilote WinUSB doit remplacer le pilote TV : \
                 lancez l'assistant Zadig. Sous Linux, vérifiez la règle udev et \
                 que le module dvb_usb_rtl28xxu est sur liste noire."
            );
        }

        let mut sdr = Self {
            api,
            device,
            name,
            sample_rate: 0,
            center_freq: 0,
        };
        // Le réglage automatique du RTL2832U sature sur les bandes encombrées :
        // le gain reste géré par le tuner seul.
        sdr.call("rtlsdr_set_agc_mode", unsafe {
            (sdr.api.set_agc_mode)(sdr.device, 0)
        })?;
        sdr.set_sample_rate(crate::dsp::DEFAULT_SAMPLE_RATE)?;
        sdr.set_gain(Gain::Auto)?;
        sdr.reset_buffer()?;
        Ok(sdr)
    }

    fn call(&self, what: &str, code: c_int) -> Result<()> {
        if code == 0 {
            Ok(())
        } else {
            Err(anyhow!("{what} a échoué (code {code})"))
        }
    }

    /// Vide la mémoire tampon de la clé : à faire avant chaque lecture suivie,
    /// sans quoi les premiers échantillons sont ceux du réglage précédent.
    pub fn reset_buffer(&mut self) -> Result<()> {
        self.call("rtlsdr_reset_buffer", unsafe {
            (self.api.reset_buffer)(self.device)
        })
    }
}

impl Drop for RtlSdr {
    fn drop(&mut self) {
        if !self.device.is_null() {
            unsafe { (self.api.close)(self.device) };
            self.device = std::ptr::null_mut();
        }
    }
}

impl Source for RtlSdr {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn set_center_freq(&mut self, hz: u32) -> Result<()> {
        self.call("rtlsdr_set_center_freq", unsafe {
            (self.api.set_center_freq)(self.device, hz)
        })?;
        self.center_freq = unsafe { (self.api.get_center_freq)(self.device) };
        Ok(())
    }

    fn center_freq(&self) -> u32 {
        self.center_freq
    }

    fn set_sample_rate(&mut self, hz: u32) -> Result<()> {
        self.call("rtlsdr_set_sample_rate", unsafe {
            (self.api.set_sample_rate)(self.device, hz)
        })?;
        self.sample_rate = unsafe { (self.api.get_sample_rate)(self.device) };
        Ok(())
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn set_gain(&mut self, gain: Gain) -> Result<()> {
        match gain {
            Gain::Auto => self.call("rtlsdr_set_tuner_gain_mode", unsafe {
                (self.api.set_tuner_gain_mode)(self.device, 0)
            }),
            Gain::Manual(tenths) => {
                self.call("rtlsdr_set_tuner_gain_mode", unsafe {
                    (self.api.set_tuner_gain_mode)(self.device, 1)
                })?;
                // Le tuner n'accepte que des paliers précis : on prend le plus
                // proche de la valeur demandée.
                let gains = self.gains();
                let chosen = gains
                    .iter()
                    .copied()
                    .min_by_key(|g| (g - tenths).abs())
                    .unwrap_or(tenths);
                self.call("rtlsdr_set_tuner_gain", unsafe {
                    (self.api.set_tuner_gain)(self.device, chosen)
                })
            }
        }
    }

    fn gains(&self) -> Vec<i32> {
        let count = unsafe { (self.api.get_tuner_gains)(self.device, std::ptr::null_mut()) };
        if count <= 0 {
            return Vec::new();
        }
        let mut buffer = vec![0; count as usize];
        let written = unsafe { (self.api.get_tuner_gains)(self.device, buffer.as_mut_ptr()) };
        buffer.truncate(written.max(0) as usize);
        buffer
    }

    fn set_ppm(&mut self, ppm: i32) -> Result<()> {
        // Régler la même correction deux fois renvoie -2 : ce n'est pas une erreur.
        let code = unsafe { (self.api.set_freq_correction)(self.device, ppm) };
        if code == 0 || code == -2 {
            Ok(())
        } else {
            Err(anyhow!("rtlsdr_set_freq_correction a échoué (code {code})"))
        }
    }

    fn set_bias_tee(&mut self, on: bool) -> Result<()> {
        match self.api.set_bias_tee {
            Some(f) => self.call("rtlsdr_set_bias_tee", unsafe {
                f(self.device, c_int::from(on))
            }),
            None => bail!(
                "cette bibliothèque RTL-SDR ne gère pas l'alimentation d'antenne ; \
                 utilisez celle livrée avec Wavecraft"
            ),
        }
    }

    fn read(&mut self, out: &mut [u8]) -> Result<usize> {
        let mut read = 0;
        let code = unsafe {
            (self.api.read_sync)(
                self.device,
                out.as_mut_ptr().cast::<c_void>(),
                out.len() as c_int,
                &mut read,
            )
        };
        if code != 0 {
            bail!("lecture de la clé interrompue (code {code})");
        }
        Ok(read.max(0) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_dossier_de_l_executable_passe_avant_le_systeme() {
        let list = candidates();
        let exe = crate::paths::exe_dir();
        if exe.is_some() {
            // La bibliothèque livrée avec Wavecraft — celle de RTL-SDR Blog,
            // seule à gérer la V4 — doit l'emporter sur celle du système.
            let first = list.first().unwrap();
            assert!(
                first.parent().is_some_and(|p| !p.as_os_str().is_empty()),
                "le premier candidat doit être un chemin complet : {first:?}"
            );
        }
        assert!(list.iter().any(|p| p == std::path::Path::new(LIB_NAMES[0])));
    }

    #[test]
    fn les_noms_de_bibliotheque_conviennent_au_systeme() {
        assert!(!LIB_NAMES.is_empty());
        #[cfg(windows)]
        assert!(LIB_NAMES[0].ends_with(".dll"));
        #[cfg(all(unix, not(target_os = "macos")))]
        assert!(LIB_NAMES[0].ends_with(".so"));
    }
}
