//! Serveur web : le poste d'écoute dans un navigateur.
//!
//! Le serveur ne possède rien. Il lit l'instantané publié par le fil qui tient
//! l'état ([`crate::link`]) et y dépose les demandes des navigateurs. Le son
//! passe par une file secondaire du moteur, recopiée dans un tampon circulaire :
//! plusieurs navigateurs peuvent donc écouter en même temps, chacun à son
//! rythme, sans se voler les échantillons.

mod page;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};


use crate::dsp::{AUDIO_RATE, Mode};
use crate::format;
use crate::link::{Link, Request as Demande};

/// Durée de son conservée pour les navigateurs, en secondes.
const RING_SECONDS: usize = 10;

/// Nom du biscuit qui porte le mot de passe.
const COOKIE: &str = "wavecraft";

/// Tampon circulaire du son, partagé par tous les navigateurs.
struct Ring {
    samples: std::collections::VecDeque<i16>,
    /// Indice global du plus ancien échantillon conservé.
    first: u64,
}

impl Ring {
    fn new() -> Self {
        Self {
            samples: std::collections::VecDeque::with_capacity(AUDIO_RATE as usize * RING_SECONDS),
            first: 0,
        }
    }

    fn push(&mut self, audio: &[f32]) {
        for &sample in audio {
            self.samples
                .push_back((sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16);
        }
        let max = AUDIO_RATE as usize * RING_SECONDS;
        while self.samples.len() > max {
            self.samples.pop_front();
            self.first += 1;
        }
    }

    fn next_index(&self) -> u64 {
        self.first + self.samples.len() as u64
    }

    /// Échantillons à partir de `from`, au plus `max`.
    ///
    /// Renvoie l'indice du premier échantillon servi : un navigateur en retard
    /// est ainsi replacé au début du tampon plutôt que de recevoir du vide.
    fn read(&self, from: u64, max: usize) -> (u64, Vec<i16>) {
        let depart = from.max(self.first);
        let decalage = (depart - self.first) as usize;
        let fin = (decalage + max).min(self.samples.len());
        if decalage >= fin {
            return (depart, Vec::new());
        }
        (
            depart,
            self.samples.range(decalage..fin).copied().collect(),
        )
    }
}

#[derive(Clone)]
struct Etat {
    link: Link,
    ring: Arc<Mutex<Ring>>,
    password: Option<String>,
    /// Jeton aléatoire déposé dans le biscuit de session (jamais le mot de passe en clair).
    session_token: Option<String>,
}

/// Génère un jeton de session aléatoire (32 chiffres hexadécimaux) à partir
/// des clés aléatoires fournies par le système à `RandomState`.
fn generer_jeton() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let mut h1 = RandomState::new().build_hasher();
    let mut h2 = RandomState::new().build_hasher();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    h1.write_u128(now);
    h1.write_u32(std::process::id());
    h2.write_u128(now.wrapping_add(1));
    h2.write_u32(std::process::id());
    format!("{:016x}{:016x}", h1.finish(), h2.finish())
}

impl Etat {
    fn new(link: Link, ring: Arc<Mutex<Ring>>, password: Option<String>) -> Self {
        let session_token = password.as_ref().map(|_| generer_jeton());
        Self {
            link,
            ring,
            password,
            session_token,
        }
    }

    /// Le navigateur a-t-il le droit d'entrer ?
    fn autorise(&self, headers: &HeaderMap) -> bool {
        let Some(attendu) = &self.session_token else {
            return true;
        };
        let Some(biscuits) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) else {
            return false;
        };
        biscuits
            .split(';')
            .filter_map(|b| b.trim().split_once('='))
            .any(|(nom, valeur)| nom == COOKIE && decode(valeur) == *attendu)
    }
}

/// Poignée sur le serveur en marche.
pub struct Handle {
    url: String,
    arret: Arc<AtomicBool>,
    link: Link,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Handle {
    /// Démarre le serveur. L'adresse est vérifiée tout de suite : une erreur de
    /// port est signalée à l'appelant, pas enterrée dans un fil.
    pub fn start(link: Link, bind: &str, port: u16, password: Option<String>) -> Result<Self> {
        let adresse: SocketAddr = format!("{bind}:{port}")
            .parse()
            .with_context(|| format!("adresse d'écoute illisible : {bind}:{port}"))?;

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .context("démarrage du serveur")?;

        let listener = runtime
            .block_on(tokio::net::TcpListener::bind(adresse))
            .with_context(|| format!("le port {port} n'est pas disponible"))?;

        let etat = Etat::new(
            link.clone(),
            Arc::new(Mutex::new(Ring::new())),
            password,
        );
        let arret = Arc::new(AtomicBool::new(false));

        // Recopie du son dans le tampon, dix fois par seconde.
        let ring = Arc::clone(&etat.ring);
        let lien_audio = link.clone();
        let arret_audio = Arc::clone(&arret);
        std::thread::Builder::new()
            .name("wavecraft-son-web".into())
            .spawn(move || {
                while !arret_audio.load(Ordering::Relaxed) {
                    if let Some(sink) = lien_audio.audio() {
                        let bloc = sink.take(AUDIO_RATE as usize);
                        if !bloc.is_empty()
                            && let Ok(mut ring) = ring.lock()
                        {
                            ring.push(&bloc);
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            })
            .context("démarrage du fil audio du serveur")?;

        let routeur = routeur(etat);
        let arret_serveur = Arc::clone(&arret);
        let thread = std::thread::Builder::new()
            .name("wavecraft-web".into())
            .spawn(move || {
                runtime.block_on(async move {
                    let arret = async move {
                        while !arret_serveur.load(Ordering::Relaxed) {
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        }
                    };
                    if let Err(err) = axum::serve(listener, routeur)
                        .with_graceful_shutdown(arret)
                        .await
                    {
                        eprintln!("serveur web : {err}");
                    }
                });
            })
            .context("démarrage du fil du serveur")?;

        let hôte = if adresse.ip().is_unspecified() {
            // Annoncer « 0.0.0.0 » n'aide personne : le nom de la machine, si.
            nom_de_machine().unwrap_or_else(|| adresse.ip().to_string())
        } else {
            adresse.ip().to_string()
        };

        Ok(Self {
            url: format!("http://{hôte}:{port}/"),
            arret,
            link,
            thread: Some(thread),
        })
    }

    pub fn url(&self) -> String {
        self.url.clone()
    }

    pub fn listeners(&self) -> usize {
        self.link.listeners()
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.arret.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn nom_de_machine() -> Option<String> {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|n| !n.is_empty())
        .map(|n| n.to_lowercase())
}

fn routeur(etat: Etat) -> Router {
    Router::new()
        .route("/", get(accueil))
        .route("/entrer", post(entrer))
        .route("/api/etat", get(api_etat))
        .route("/api/son", get(api_son))
        .route("/api/ecouter/{station}", post(api_ecouter))
        .route("/api/commande/{ordre}", post(api_commande))
        .with_state(etat)
}

async fn accueil(State(etat): State<Etat>, headers: HeaderMap) -> Response {
    if !etat.autorise(&headers) {
        return Html(page::login()).into_response();
    }
    Html(page::index()).into_response()
}

/// Dépose le mot de passe dans un biscuit, s'il est bon.
async fn entrer(State(etat): State<Etat>, corps: String) -> Response {
    let saisi = corps
        .split('&')
        .filter_map(|c| c.split_once('='))
        .find(|(nom, _)| *nom == "mot")
        .map(|(_, valeur)| decode(valeur))
        .unwrap_or_default();

    match (&etat.password, &etat.session_token) {
        (Some(attendu), Some(jeton)) if &saisi == attendu => {
            let mut headers = HeaderMap::new();
            // Le biscuit transporte un jeton de session aléatoire, jamais le
            // mot de passe en clair, et n'est pas lisible en script.
            headers.insert(
                header::SET_COOKIE,
                format!(
                    "{COOKIE}={}; HttpOnly; SameSite=Strict; Path=/",
                    encode(jeton)
                )
                .parse()
                .expect("en-tête de biscuit valide"),
            );
            (headers, Redirect::to("/")).into_response()
        }
        _ => (StatusCode::UNAUTHORIZED, Html(page::login())).into_response(),
    }
}

/// Décodage d'un champ de formulaire ou d'un biscuit.
///
/// Le travail se fait sur les octets : un mot de passe accentué arrive en UTF-8
/// pour-cent-encodé, et le recomposer caractère par caractère le mutilerait.
fn decode(brut: &str) -> String {
    let brut = brut.replace('+', " ");
    let octets = brut.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(octets.len());
    let mut index = 0;
    while index < octets.len() {
        if octets[index] == b'%' && index + 2 < octets.len() {
            let haut = (octets[index + 1] as char).to_digit(16);
            let bas = (octets[index + 2] as char).to_digit(16);
            if let (Some(h), Some(b)) = (haut, bas) {
                out.push((h * 16 + b) as u8);
                index += 3;
                continue;
            }
        }
        out.push(octets[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Encodage inverse, pour déposer une valeur dans un biscuit.
///
/// Un en-tête HTTP ne transporte que de l'ASCII visible : tout le reste est
/// écrit en pour-cent.
fn encode(brut: &str) -> String {
    let mut out = String::with_capacity(brut.len());
    for octet in brut.as_bytes() {
        if octet.is_ascii_alphanumeric() || matches!(octet, b'-' | b'_' | b'.' | b'~') {
            out.push(*octet as char);
        } else {
            out.push_str(&format!("%{octet:02X}"));
        }
    }
    out
}

async fn api_etat(State(etat): State<Etat>, headers: HeaderMap) -> Response {
    if !etat.autorise(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let instantané = etat.link.snapshot();
    let maintenant = chrono::Utc::now();

    let stations: Vec<serde_json::Value> = instantané
        .rows
        .iter()
        .take(60)
        .map(|row| {
            serde_json::json!({
                "id": row.id,
                "nom": row.name,
                "frequence": row.frequency_khz,
                "mode": row.mode,
                "quand": format::when(row.continuous, row.next, maintenant),
                "encours": row.continuous
                    || row.next.map(|at| at <= maintenant).unwrap_or(false),
            })
        })
        .collect();

    let statut = instantané.status.as_ref();
    let prochain = etat
        .ring
        .lock()
        .map(|r| r.next_index())
        .unwrap_or_default();

    let corps = serde_json::json!({
        "station": instantané.station,
        "nom": instantané.station_name,
        "frequence": statut.map(|s| s.frequency_hz).unwrap_or(0),
        "frequence_texte": statut.map(|s| format::tuned(s.frequency_hz)).unwrap_or_default(),
        "mode": statut.map(|s| s.mode.label()).unwrap_or(""),
        "bande": statut.map(|s| s.bandwidth_hz).unwrap_or(0.0),
        "niveau": statut.map(|s| s.level_dbfs).unwrap_or(-120.0),
        "smetre": format::s_meter(statut.map(|s| s.level_dbfs).unwrap_or(-120.0)),
        "enregistre": statut.map(|s| s.recording.is_some()).unwrap_or(false),
        "morse_actif": statut.map(|s| s.morse_enabled).unwrap_or(false),
        "morse": instantané.morse_text,
        "morse_vitesse": statut.map(|s| s.morse_wpm).unwrap_or(0.0),
        "spectre": spectre(statut.map(|s| s.spectrum.as_slice()).unwrap_or(&[])),
        "message": instantané.message,
        "planning": instantané.planning_age,
        "horloge": format::clock_line(maintenant),
        "auditeurs": etat.link.listeners(),
        "son_prochain": prochain,
        "source": statut.map(|s| s.source.clone()).unwrap_or_default(),
        "stations": stations,
        "log": instantané
            .log
            .iter()
            .map(|e| serde_json::json!({
                "at": e.at,
                "station": e.station,
                "frequency_khz": e.frequency_khz,
                "rating": e.rating,
                "tags": e.tags,
            }))
            .collect::<Vec<_>>(),
    });
    axum::Json(corps).into_response()
}

/// Spectre ramené à 256 points et à des entiers de 0 à 255 : la cascade du
/// navigateur n'a pas besoin de plus, et la page reste légère. L'échelle
/// s'adapte au bruit de fond reçu, comme sur l'interface bureau.
fn spectre(brut: &[f32]) -> Vec<u8> {
    if brut.is_empty() {
        return Vec::new();
    }
    let (mut floor_db, mut ceiling_db) = (-110.0f32, -20.0f32);
    let mut tri: Vec<f32> = brut.iter().copied().filter(|v| v.is_finite()).collect();
    if tri.len() >= 8 {
        let rang = |tri: &mut Vec<f32>, part: f32| -> f32 {
            let index = ((tri.len() - 1) as f32 * part) as usize;
            *tri.select_nth_unstable_by(index, f32::total_cmp).1
        };
        let bas = rang(&mut tri, 0.5) + 2.0;
        let haut = rang(&mut tri, 0.995) + 8.0;
        if haut - bas >= 12.0 {
            floor_db = bas;
            ceiling_db = haut;
        }
    }
    (0..256)
        .map(|x| {
            let debut = x * brut.len() / 256;
            let fin = ((x + 1) * brut.len() / 256).max(debut + 1).min(brut.len());
            let db = brut[debut..fin]
                .iter()
                .copied()
                .fold(f32::NEG_INFINITY, f32::max);
            (crate::theme::normalize_db(db, floor_db, ceiling_db) * 255.0) as u8
        })
        .collect()
}

#[derive(serde::Deserialize)]
struct Depuis {
    #[serde(default)]
    depuis: u64,
    #[serde(default)]
    ms: u64,
}

/// Son brut : un en-tête de huit octets (indice du premier échantillon), puis
/// des entiers 16 bits signés à 48 kHz.
async fn api_son(
    State(etat): State<Etat>,
    headers: HeaderMap,
    Query(params): Query<Depuis>,
) -> Response {
    if !etat.autorise(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let max = ((params.ms.clamp(100, 4_000) * u64::from(AUDIO_RATE)) / 1_000) as usize;
    let (depart, échantillons) = match etat.ring.lock() {
        Ok(ring) => ring.read(params.depuis, max),
        Err(_) => (0, Vec::new()),
    };

    let mut corps = Vec::with_capacity(8 + échantillons.len() * 2);
    corps.extend_from_slice(&depart.to_le_bytes());
    for échantillon in &échantillons {
        corps.extend_from_slice(&échantillon.to_le_bytes());
    }
    (
        [(header::CONTENT_TYPE, "application/octet-stream")],
        corps,
    )
        .into_response()
}

async fn api_ecouter(
    State(etat): State<Etat>,
    headers: HeaderMap,
    Path(station): Path<String>,
) -> Response {
    if !etat.autorise(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    etat.link.request(Demande::Listen(station));
    StatusCode::NO_CONTENT.into_response()
}

#[derive(serde::Deserialize)]
struct Valeur {
    #[serde(default)]
    valeur: f32,
    #[serde(default)]
    texte: String,
}

async fn api_commande(
    State(etat): State<Etat>,
    headers: HeaderMap,
    Path(ordre): Path<String>,
    Query(params): Query<Valeur>,
) -> Response {
    if !etat.autorise(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let demande = match ordre.as_str() {
        "morse" => Demande::SetMorse(params.valeur > 0.5),
        "morse-effacer" => Demande::ClearMorse,
        "enregistrer" => Demande::Recording(params.valeur > 0.5),
        "volume" => Demande::Volume(params.valeur),
        "actualiser" => Demande::Refresh,
        "arreter" => Demande::Stop,
        "accord" => {
            let mode = Mode::parse(&params.texte).unwrap_or(Mode::Usb);
            Demande::Tune {
                frequency_hz: params.valeur as u32,
                mode,
                bandwidth_hz: mode.default_bandwidth(),
            }
        }
        "rejoindre" => {
            etat.link.joined();
            return StatusCode::NO_CONTENT.into_response();
        }
        "quitter" => {
            etat.link.left();
            return StatusCode::NO_CONTENT.into_response();
        }
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    etat.link.request(demande);
    StatusCode::NO_CONTENT.into_response()
}

/// Mode serveur sans fenêtre : la boucle qui possède l'état vit ici.
pub fn serve(options: &crate::cli::Options) -> Result<()> {
    let link = Link::new();
    let mut state = crate::state::AppState::new(options.demo)?;

    let handle = Handle::start(
        link.clone(),
        &options.bind,
        options.port,
        options.password.clone(),
    )?;
    println!("Wavecraft {} — serveur web", env!("CARGO_PKG_VERSION"));
    println!("Adresse    : {}", handle.url());
    println!(
        "Accès      : {}",
        if options.password.is_some() {
            "protégé par mot de passe"
        } else {
            "libre — ne l'ouvrez pas au-delà du réseau local"
        }
    );
    println!("Catalogue  : {} stations", state.catalog.stations.len());
    println!("Arrêt      : Ctrl+C");

    // Reprendre la dernière station écoutée évite qu'un serveur sans écran
    // redémarre muet après une coupure de courant.
    if let Some(station) = state.settings.last_station.clone()
        && let Err(err) = state.listen(&station)
    {
        eprintln!("reprise de {station} impossible : {err:#}");
    }

    let arret = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&arret);
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("boucle d'attente du signal d'arrêt");
        runtime.block_on(async {
            let _ = tokio::signal::ctrl_c().await;
        });
        signal.store(true, Ordering::Relaxed);
    });

    while !arret.load(Ordering::Relaxed) {
        state.tick();
        if state.needs_refresh(chrono::Utc::now()) {
            state.refresh();
        }
        state.apply_requests(&link);
        state.publish(&link);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    println!();
    println!("Arrêt du serveur.");
    handle.stop();
    state.stop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_tampon_garde_les_dernieres_secondes() {
        let mut ring = Ring::new();
        let bloc = vec![0.5f32; AUDIO_RATE as usize];
        for _ in 0..(RING_SECONDS + 3) {
            ring.push(&bloc);
        }
        assert_eq!(ring.samples.len(), AUDIO_RATE as usize * RING_SECONDS);
        assert_eq!(ring.first, AUDIO_RATE as u64 * 3);
        assert_eq!(ring.next_index(), AUDIO_RATE as u64 * (RING_SECONDS + 3) as u64);
    }

    #[test]
    fn un_navigateur_en_retard_est_replace_au_debut() {
        let mut ring = Ring::new();
        ring.push(&vec![0.25f32; AUDIO_RATE as usize * (RING_SECONDS + 2)]);
        // L'indice 0 n'existe plus : le lecteur reprend au plus ancien conservé.
        let (depart, bloc) = ring.read(0, 1_000);
        assert_eq!(depart, ring.first);
        assert_eq!(bloc.len(), 1_000);
    }

    #[test]
    fn la_lecture_au_dela_du_tampon_ne_renvoie_rien() {
        let mut ring = Ring::new();
        ring.push(&[0.1, 0.2, 0.3]);
        let (depart, bloc) = ring.read(3, 100);
        assert_eq!(depart, 3);
        assert!(bloc.is_empty());
        let (_, bloc) = ring.read(1, 100);
        assert_eq!(bloc.len(), 2);
    }

    #[test]
    fn le_formulaire_est_decode() {
        assert_eq!(decode("mot+de+passe"), "mot de passe");
        assert_eq!(decode("a%2Db"), "a-b");
        assert_eq!(decode("simple"), "simple");
        // Un accent occupe deux octets en UTF-8 : il doit revenir entier.
        assert_eq!(decode("s%C3%A9same"), "sésame");
        assert_eq!(decode(&encode("mot de passe accentué : é")), "mot de passe accentué : é");
        assert_eq!(encode("sésame"), "s%C3%A9same");
    }

    #[test]
    fn le_spectre_est_ramene_a_256_points() {
        let brut: Vec<f32> = (0..1024).map(|n| -110.0 + n as f32 / 10.0).collect();
        let reduit = spectre(&brut);
        assert_eq!(reduit.len(), 256);
        assert!(reduit[0] < reduit[255], "l'échelle doit rester croissante");
        assert!(spectre(&[]).is_empty());
    }

    #[test]
    fn le_jeton_de_session_est_verifie_dans_le_biscuit() {
        let etat = Etat::new(
            Link::new(),
            Arc::new(Mutex::new(Ring::new())),
            Some("sésame".into()),
        );
        let jeton = etat.session_token.clone().expect("jeton généré");
        assert_ne!(jeton, "sésame", "le jeton ne doit pas être le mot de passe");

        let mut headers = HeaderMap::new();
        assert!(!etat.autorise(&headers), "sans biscuit, l'accès est refusé");
        // Le mot de passe en clair dans le biscuit ne doit pas suffire.
        headers.insert(
            header::COOKIE,
            format!("wavecraft={}", encode("sésame")).parse().unwrap(),
        );
        assert!(!etat.autorise(&headers));
        // Le jeton de session donne l'accès.
        let biscuit = format!("autre=1; wavecraft={}", encode(&jeton));
        headers.insert(header::COOKIE, biscuit.parse().unwrap());
        assert!(etat.autorise(&headers));

        let libre = Etat::new(Link::new(), Arc::new(Mutex::new(Ring::new())), None);
        assert!(libre.autorise(&HeaderMap::new()), "sans mot de passe, accès libre");
    }
}
