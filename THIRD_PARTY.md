# Origines et licences

Wavecraft est distribué sous licence **GPL-3.0-or-later** (voir `LICENSE`).
Projet indépendant, sans affiliation avec Priyom.org, Irdial-Discs ni RTL-SDR Blog.

## Données

| Source | Usage | Licence |
|---|---|---|
| [Priyom.org](https://priyom.org/) | fiches des stations (`app/assets/stations.json`) et calendrier public des émissions | CC BY-NC-SA 4.0 |

Le catalogue livré avec l'application (`app/assets/catalog.json`) est produit par
`tools/update_catalog.py` à partir de ces deux sources. Aucune fréquence, aucun
mode, aucun horaire n'y est inventé : tout provient du relevé.

## Logiciels livrés à côté de l'application (paquet Windows)

| Composant | Rôle | Origine | Licence |
|---|---|---|---|
| `rtlsdr.dll` (RTL-SDR Blog V1.4.0) | pilotage de la clé, convertisseur HF de la V4 compris | <https://github.com/rtlsdrblog/rtl-sdr-blog> | GPL-2.0-or-later |
| `zadig-2.9.exe` | installation du pilote WinUSB sous Windows | <https://github.com/pbatard/libwdi> | GPL-3.0-or-later |

Les empreintes SHA-256 des archives téléchargées sont conservées dans
`vendor/*/SOURCE.txt`. La bibliothèque est chargée à l'exécution, jamais liée à
la compilation : Wavecraft se construit sans chaîne C.

## Polices

| Police | Usage | Licence |
|---|---|---|
| IBM Plex Mono, IBM Plex Sans Condensed | site de présentation, et fenêtre si les fichiers sont présents | SIL Open Font License 1.1 |

## Bibliothèques Rust

Les dépendances directes et leurs licences sont déclarées dans `app/Cargo.toml`
et verrouillées dans `app/Cargo.lock`. Les principales :

| Bibliothèque | Rôle | Licence |
|---|---|---|
| `eframe` / `egui` | fenêtre et interface bureau | MIT OR Apache-2.0 |
| `axum` / `tokio` | serveur web | MIT |
| `cpal` | sortie audio | Apache-2.0 |
| `rustfft` | transformée de Fourier de la cascade | MIT OR Apache-2.0 |
| `libloading` | chargement de `rtlsdr.dll` à l'exécution | ISC |
| `nusb` | détection de la clé sur le bus USB | MIT OR Apache-2.0 |
| `hound` | écriture des enregistrements WAV | Apache-2.0 |
| `ureq` | téléchargement du calendrier et du catalogue | MIT OR Apache-2.0 |
| `serde`, `serde_json`, `chrono`, `regex`, `anyhow`, `thiserror`, `png` | outillage général | MIT OR Apache-2.0 |

La liste complète s'obtient avec `cargo tree` ou `cargo license` dans `app/`.

## Enregistrements cités

L'archive [The Conet Project](https://nthill.be/sw/) est citée comme source
documentaire ; les enregistrements restent © Irdial-Discs et ne sont pas
redistribués avec Wavecraft.
