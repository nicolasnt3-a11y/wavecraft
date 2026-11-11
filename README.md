# Wavecraft

**Récepteur d'ondes courtes dédié aux stations de nombres.**
Branchez une clé RTL-SDR, tendez un fil, choisissez une station : Wavecraft règle
la fréquence, le mode et le filtre à votre place, et vous dit qui émet en ce moment.

Présentation et mode d'emploi : **<https://nthill.be/wavecraft/>**

Logiciel libre sous licence GPL-3.0. Windows 10/11 et Linux, 64 bits.

---

## Ce qu'il fait

- **Écoute en un clic.** Catalogue de 168 stations relevées chez Priyom.org, dont
  28 écoutables aujourd'hui (fréquence, mode USB/LSB/AM/CW/NFM, bande passante).
- **Classées par heure d'écoute.** « En cours », « 24 h/24 », « dans 12 min ».
  Heure locale et UTC côte à côte, repère jour/nuit pour la propagation.
- **Planning en direct.** Les horaires du mois viennent du calendrier public de
  Priyom ; hors connexion, le planning hebdomadaire du catalogue prend le relais.
- **Assistant de premier lancement.** Brancher la clé, poser le pilote (fait
  automatiquement sous Windows, règle udev sous Linux), placer l'antenne, écouter
  une mire.
- **RTL-SDR Blog V4.** Bibliothèque officielle livrée avec le paquet Windows,
  convertisseur HF géré. Accepte aussi une clé distante (`rtl_tcp`) et un mode
  démonstration sans matériel.
- **Décodeur Morse.** Tonalité suivie automatiquement, 8 à 40 mots/minute,
  chiffres abrégés des opérateurs traduits, transcription en direct.
- **Journal d'écoute.** Sessions notées, étiquetées, liées à l'enregistrement WAV
  et à la transcription, exportables en CSV, JSON ou Markdown.
- **Enregistrement WAV, cascade et S-mètre.**
- **Mode serveur web.** Sur un conteneur LXC, un Raspberry Pi ou un NAS : son en
  direct, cascade, Morse, catalogue et journal dans le navigateur.

## Installer

### Windows 10/11

1. Téléchargez `wavecraft-setup.exe` depuis la
   [page des versions](https://github.com/nicolasnt3-a11y/wavecraft/releases/latest).
2. Lancez-le : il installe Wavecraft et place automatiquement l'icône sur votre Bureau.
3. Au premier lancement, l'assistant configure le pilote WinUSB de votre clé
   RTL-SDR Blog V4 tout seul (via Zadig intégré).

*(Une version archive `wavecraft-windows-x64.zip` reste également disponible pour une utilisation portable sans installation).*

### Linux

```sh
tar xzf wavecraft-linux-x64.tar.gz
cd wavecraft
sudo ./install.sh        # règle udev, blacklist du pilote TNT, raccourci
wavecraft
```

Le script explique chaque geste avant de le faire et n'écrit rien sans accord.

### Serveur sans écran

```sh
wavecraft --server --bind 0.0.0.0 --port 8073
```

Puis `http://adresse-du-serveur:8073/` depuis n'importe quel navigateur du
réseau local. Un mot de passe peut être exigé avec `--password`. Gardez l'accès
sur le réseau local ; pour l'extérieur, passez par un VPN plutôt que d'ouvrir le
port.

## Options

```
wavecraft                         ouvre l'interface bureau
wavecraft --server                démarre le serveur web
wavecraft --probe                 vérifie la clé, l'agenda et le catalogue

  --server              mode serveur web, sans fenêtre
  --bind <adresse>      interface d'écoute du serveur      (défaut : 127.0.0.1)
  --port <numéro>       port du serveur                    (défaut : 8073)
  --password <mot>      mot de passe d'accès au serveur
  --demo                source de démonstration, sans clé RTL-SDR
  --probe               diagnostic puis sortie
  --installer-pilote    installe le pilote WinUSB de la clé (Windows ; demande
                        l'autorisation de Windows, puis quitte)
  --captures <dossier>  enregistre une capture de chaque écran, puis quitte
```

## Construire

Rust 1.85 ou plus récent. Aucune dépendance C : `rtlsdr.dll` / `librtlsdr.so`
est chargée à l'exécution, cherchée d'abord à côté de l'exécutable.

```sh
cd app
cargo build --release                                   # fenêtre + serveur
cargo build --release --no-default-features --features server   # serveur seul
cargo test
```

| Option de compilation | Contenu |
|---|---|
| `desktop` | fenêtre egui, sortie audio locale, captures d'écran |
| `server` | serveur web et son interface navigateur |
| `local-audio` | sortie sur la carte son |

Le paquet serveur se construit sans `desktop` : il n'a besoin d'aucune
bibliothèque graphique, ce qui convient à un conteneur.

## Catalogue

`app/assets/catalog.json` est produit par les deux outils de `tools/` :

```sh
python tools/fetch_stations.py      # fiches des stations (Priyom)
python tools/update_catalog.py      # créneaux horaires (calendrier Priyom)
```

Le calendrier plafonne à 250 événements par requête, sans pagination : l'outil
interroge donc par tranches de 24 heures, redécoupe une tranche saturée et
dédoublonne aux jonctions. Un garde-fou refuse d'écrire un catalogue manifestement
incomplet. Le relevé actuel couvre 14 jours, 1 679 émissions et 698 créneaux
récurrents.

L'application vérifie d'elle-même qu'une version plus récente du catalogue est
publiée, et relit le planning toutes les six heures.

## Où sont les fichiers

| | Windows | Linux |
|---|---|---|
| Réglages, journal, catalogue | `%APPDATA%\wavecraft\` | `~/.local/share/wavecraft/` |
| Enregistrements | `%APPDATA%\wavecraft\enregistrements\` | `~/.local/share/wavecraft/enregistrements/` |

`WAVECRAFT_DATA_DIR` impose un autre emplacement.

## Licences

Wavecraft : GPL-3.0-or-later (`LICENSE`).
Données des stations et calendrier : [Priyom.org](https://priyom.org/), CC BY-NC-SA 4.0.
Composants livrés et leurs licences : `THIRD_PARTY.md`.

Projet indépendant, sans affiliation avec Priyom.org, Irdial-Discs ni RTL-SDR Blog.
