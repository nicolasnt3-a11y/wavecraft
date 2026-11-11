//! Lecture de la ligne de commande.
//!
//! Écrite à la main : les options sont peu nombreuses et stables, et le binaire
//! livré n'a pas besoin d'une dépendance de plus pour les lire.

use std::fmt;

pub const HELP: &str = concat!(
    "Wavecraft ",
    env!("CARGO_PKG_VERSION"),
    r#" — récepteur dédié aux stations de nombres

Usage :
  wavecraft                         ouvre l'interface bureau
  wavecraft --server                démarre le serveur web
  wavecraft --probe                 vérifie la clé, l'agenda et le catalogue

Options :
  --server              mode serveur web, sans fenêtre (conteneur, Raspberry…)
  --bind <adresse>      interface d'écoute du serveur      (défaut : 127.0.0.1)
  --port <numéro>       port du serveur                    (défaut : 8073)
  --password <mot>      mot de passe d'accès au serveur
  --demo                source de démonstration, sans clé RTL-SDR
  --probe               diagnostic puis sortie
  --installer-pilote    installe le pilote WinUSB de la clé (Windows ; demande
                        l'autorisation de Windows, puis quitte)
  --captures <dossier>  enregistre une capture de chaque écran, puis quitte
  --version             affiche la version
  --help, -h            affiche cette aide

Sans mot de passe, le serveur n'écoute que sur l'adresse locale : exposez-le
derrière un mot de passe avant de l'ouvrir au réseau.
"#
);

/// Mode de lancement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Interface bureau.
    Desktop,
    /// Serveur web.
    Server,
    /// Diagnostic puis sortie.
    Probe,
    /// Installation du pilote de la clé (Windows), puis sortie.
    InstallDriver,
    /// Aide ou version : rien à lancer.
    Print(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub mode: Mode,
    pub bind: String,
    pub port: u16,
    pub password: Option<String>,
    /// Source de démonstration : permet d'essayer l'application sans clé.
    pub demo: bool,
    /// Dossier où déposer une capture de chaque écran de la fenêtre.
    pub captures: Option<std::path::PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Mode::Desktop,
            bind: "127.0.0.1".to_string(),
            port: 8073,
            password: None,
            demo: false,
            captures: None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError(String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseError {}

/// Lit les options de la ligne de commande.
pub fn parse<I, S>(args: I) -> Result<Options, ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut options = Options::default();
    let mut args = args.into_iter().map(|a| a.as_ref().to_string()).peekable();

    while let Some(arg) = args.next() {
        let mut value = |name: &str| -> Result<String, ParseError> {
            args.next()
                .ok_or_else(|| ParseError(format!("l'option {name} attend une valeur")))
        };
        match arg.as_str() {
            "--server" => options.mode = Mode::Server,
            "--probe" => options.mode = Mode::Probe,
            "--installer-pilote" => options.mode = Mode::InstallDriver,
            "--demo" => options.demo = true,
            "--help" | "-h" => return Ok(Options { mode: Mode::Print(HELP), ..options }),
            "--version" | "-V" => {
                return Ok(Options {
                    mode: Mode::Print(concat!("Wavecraft ", env!("CARGO_PKG_VERSION"))),
                    ..options
                });
            }
            "--bind" => options.bind = value("--bind")?,
            "--port" => {
                let raw = value("--port")?;
                options.port = raw
                    .parse()
                    .map_err(|_| ParseError(format!("port invalide : {raw}")))?;
            }
            "--password" => options.password = Some(value("--password")?),
            "--captures" => {
                options.captures = Some(std::path::PathBuf::from(value("--captures")?));
                // Les captures se font sur la source de démonstration : elles
                // ne dépendent donc ni de la clé ni de la propagation.
                options.demo = true;
            }
            other => {
                return Err(ParseError(format!(
                    "option inconnue : {other}\nLancez « wavecraft --help » pour la liste."
                )));
            }
        }
    }

    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sans_option_l_interface_bureau_s_ouvre() {
        let o = parse(Vec::<String>::new()).unwrap();
        assert_eq!(o.mode, Mode::Desktop);
        assert_eq!(o.bind, "127.0.0.1");
        assert_eq!(o.port, 8073);
        assert!(!o.demo);
    }

    #[test]
    fn le_mode_serveur_accepte_adresse_port_et_mot_de_passe() {
        let o = parse(["--server", "--bind", "0.0.0.0", "--port", "9000", "--password", "secret"])
            .unwrap();
        assert_eq!(o.mode, Mode::Server);
        assert_eq!(o.bind, "0.0.0.0");
        assert_eq!(o.port, 9000);
        assert_eq!(o.password.as_deref(), Some("secret"));
    }

    #[test]
    fn le_diagnostic_et_la_demonstration_se_combinent() {
        let o = parse(["--probe", "--demo"]).unwrap();
        assert_eq!(o.mode, Mode::Probe);
        assert!(o.demo);
    }

    #[test]
    fn un_port_invalide_est_signale() {
        let err = parse(["--port", "abc"]).unwrap_err();
        assert!(err.to_string().contains("port invalide"), "{err}");
    }

    #[test]
    fn une_option_sans_valeur_est_signalee() {
        let err = parse(["--bind"]).unwrap_err();
        assert!(err.to_string().contains("attend une valeur"), "{err}");
    }

    #[test]
    fn une_option_inconnue_renvoie_vers_l_aide() {
        let err = parse(["--turbo"]).unwrap_err();
        assert!(err.to_string().contains("--help"), "{err}");
    }

    #[test]
    fn l_aide_cite_toutes_les_options() {
        for option in [
            "--server",
            "--bind",
            "--port",
            "--password",
            "--demo",
            "--probe",
            "--installer-pilote",
            "--captures",
        ] {
            assert!(HELP.contains(option), "{option} absent de l'aide");
        }
    }
}
