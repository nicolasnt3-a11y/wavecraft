//! La page servie aux navigateurs.
//!
//! Une seule page, sans framework ni ressource extérieure : la feuille de style
//! reprend les variables de [`crate::theme`], donc exactement les gris du site.
//! Le son est reconstruit dans le navigateur à partir des échantillons bruts
//! servis par `/api/son`, ce qui évite tout format audio intermédiaire.

use crate::theme;

/// Page d'entrée, quand un mot de passe protège le serveur.
pub fn login() -> String {
    format!(
        r#"<!doctype html>
<html lang="fr"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Wavecraft</title>
<style>{style}
.entree {{ max-width: 380px; margin: 18vh auto; }}
</style></head>
<body><div class="entree">
<h1 class="marque">WAVECRAFT</h1>
<p class="discret">Ce poste d'écoute est protégé par un mot de passe.</p>
<form method="post" action="/entrer">
  <input type="password" name="mot" autofocus placeholder="Mot de passe">
  <button type="submit">Entrer</button>
</form>
</div></body></html>"#,
        style = style()
    )
}

/// Le poste d'écoute.
pub fn index() -> String {
    format!(
        r#"<!doctype html>
<html lang="fr"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Wavecraft</title>
<style>{style}</style></head>
<body>
<header>
  <span class="marque">WAVECRAFT</span>
  <span class="discret espace">SERVEUR · NAVIGATEUR</span>
  <span class="horloge" id="horloge"></span>
</header>

<main>
  <section class="colonne">
    <p class="discret espace" id="entete">—</p>
    <p class="frequence"><span id="frequence">—</span> <span class="unite">kHz</span></p>
    <canvas id="cascade" width="512" height="300"></canvas>
    <div class="barre">
      <button id="son">▶ Écouter</button>
      <span class="smetre"><span id="jauge"></span></span>
      <span id="smetre" class="discret">S0</span>
      <button id="enregistrer">● Enregistrer</button>
    </div>
    <div class="barre">
      <label class="discret"><input type="checkbox" id="morseactif"> Décoder le Morse</label>
      <span class="discret" id="vitesse"></span>
      <button id="effacer">Effacer</button>
    </div>
    <pre id="morse" class="morse"></pre>
  </section>

  <aside class="colonne">
    <p class="discret espace">Catalogue</p>
    <div id="stations" class="liste"></div>
    <p class="discret espace">Journal</p>
    <div id="journal" class="liste journal"></div>
  </aside>
</main>

<footer>
  <span id="message" class="discret"></span>
  <span class="discret">Priyom.org · GPL-3.0 · <a href="https://nthill.be/wavecraft/">nthill.be/wavecraft</a></span>
</footer>

<script>{script}</script>
</body></html>"#,
        style = style(),
        script = script()
    )
}

fn style() -> String {
    format!(
        r#"{variables}
* {{ box-sizing: border-box; }}
body {{
  margin: 0; background: var(--bg); color: var(--ink);
  font-family: var(--mono); font-size: 13px; line-height: 1.5;
}}
a {{ color: var(--ink); }}
header {{
  display: flex; align-items: center; gap: 16px; height: 48px; padding: 0 20px;
  background: var(--bg-2); border-bottom: 1px solid var(--line);
}}
.marque {{ letter-spacing: 3px; font-weight: 600; color: var(--white); }}
.espace {{ letter-spacing: 2px; text-transform: uppercase; font-size: 11px; }}
.horloge {{ margin-left: auto; color: var(--ink-2); letter-spacing: 2px; font-size: 12px; }}
main {{ display: flex; gap: 24px; padding: 24px 20px; align-items: flex-start; flex-wrap: wrap; }}
.colonne {{ flex: 1 1 420px; min-width: 320px; }}
.discret {{ color: var(--ink-t); }}
.frequence {{ font-size: 44px; margin: 4px 0 14px; color: var(--white); }}
.unite {{ font-size: 15px; color: var(--ink-t); }}
canvas {{
  width: 100%; height: 300px; display: block; background: var(--bg);
  border: 1px solid var(--line); image-rendering: pixelated;
}}
.barre {{ display: flex; align-items: center; gap: 14px; margin: 14px 0; flex-wrap: wrap; }}
.barre label {{ display: flex; align-items: center; gap: 8px; }}
.barre input[type=checkbox] {{ width: auto; margin: 0; accent-color: var(--white); }}
button, input {{
  font-family: var(--mono); font-size: 13px; color: var(--ink);
  background: var(--bg); border: 1px solid var(--ink-4); padding: 8px 14px;
}}
button:hover {{ background: var(--white); color: var(--bg); cursor: pointer; }}
input {{ width: 100%; margin-bottom: 10px; }}
.smetre {{
  flex: 1 1 140px; height: 14px; background: var(--bg);
  border: 1px solid var(--ink-4); display: block; max-width: 280px;
}}
.smetre > span {{ display: block; height: 100%; width: 0; background: var(--white); }}
.morse {{
  margin: 0; padding: 14px; min-height: 90px; white-space: pre-wrap; word-break: break-all;
  background: var(--bg-2); border: 1px solid var(--ink-4);
  font-size: 20px; letter-spacing: 4px; color: var(--ink);
}}
.liste {{ border-top: 1px solid var(--line); margin-bottom: 20px; }}
.ligne {{
  display: flex; align-items: center; gap: 12px; padding: 10px 8px;
  border-bottom: 1px solid var(--line); cursor: pointer;
}}
.ligne:hover {{ background: var(--bg-3); }}
.ligne .id {{ width: 56px; color: var(--white); }}
.ligne .nom {{ flex: 1; color: var(--ink); }}
.ligne .freq {{ width: 90px; color: var(--ink-2); text-align: right; }}
.ligne .quand {{ width: 150px; text-align: right; font-size: 11px; letter-spacing: 1px; color: var(--ink-t); }}
.ligne.encours .quand {{ color: var(--white); }}
.ligne.encours {{ background: var(--bg-3); }}
.journal .ligne {{ cursor: default; }}
footer {{
  display: flex; gap: 20px; justify-content: space-between; flex-wrap: wrap;
  padding: 12px 20px; border-top: 1px solid var(--line); font-size: 11px;
}}
@media (max-width: 900px) {{ .frequence {{ font-size: 32px; }} }}"#,
        variables = theme::css_variables()
    )
}

fn script() -> String {
    // Le script est écrit à la main, sans dépendance : il tient en une page et
    // n'a besoin que de `fetch`, d'un canevas et de l'API audio du navigateur.
    r#"
const $ = (id) => document.getElementById(id);
const cascade = $('cascade');
const dessin = cascade.getContext('2d');
let etat = null;
let audio = null, prochain = 0, joue = false, horloge = 0;

async function commande(ordre, valeur, texte) {
  const q = new URLSearchParams();
  if (valeur !== undefined) q.set('valeur', valeur);
  if (texte !== undefined) q.set('texte', texte);
  await fetch(`/api/commande/${ordre}?${q}`, { method: 'POST' });
}

async function rafraichir() {
  try {
    const reponse = await fetch('/api/etat');
    if (!reponse.ok) { return; }
    etat = await reponse.json();
  } catch (e) { return; }

  $('horloge').textContent = etat.horloge + (etat.auditeurs ? ` · ${etat.auditeurs} auditeur${etat.auditeurs > 1 ? 's' : ''}` : '');
  const entete = [etat.station || 'libre', etat.nom, etat.mode].filter(Boolean);
  $('entete').textContent = entete.join(' · ');
  $('frequence').textContent = etat.frequence_texte || '—';
  $('smetre').textContent = etat.smetre;
  $('message').textContent = etat.message || '';
  $('morseactif').checked = etat.morse_actif;
  $('vitesse').textContent = etat.morse_actif && etat.morse_vitesse > 0
    ? `${Math.round(etat.morse_vitesse)} mots/min` : '';
  $('morse').textContent = etat.morse || '';
  $('enregistrer').textContent = etat.enregistre ? '■ Arrêter' : '● Enregistrer';

  // Le S-mètre va de S0 à S9+30, comme dans la fenêtre.
  const niveau = Math.max(0, Math.min(1, (etat.niveau + 74) / 84));
  $('jauge').style.width = `${(niveau * 100).toFixed(0)}%`;

  if (etat.spectre && etat.spectre.length) { ligneCascade(etat.spectre); }
  listeStations(etat.stations || []);
  listeJournal(etat.log || []);
}

function ligneCascade(spectre) {
  const l = cascade.width, h = cascade.height;
  // On pousse l'image d'un point vers le bas, puis on écrit la ligne du haut.
  dessin.drawImage(cascade, 0, 1);
  const image = dessin.createImageData(l, 1);
  for (let x = 0; x < l; x++) {
    const v = spectre[Math.floor(x * spectre.length / l)];
    const i = x * 4;
    image.data[i] = image.data[i + 1] = image.data[i + 2] = v;
    image.data[i + 3] = 255;
  }
  dessin.putImageData(image, 0, 0);
  // Repère central : la station écoutée est au milieu.
  dessin.fillStyle = '#fff';
  dessin.fillRect(l / 2, 0, 1, h);
}

function listeStations(stations) {
  const hote = $('stations');
  const signature = stations.map((s) => s.id + s.quand).join('|');
  if (hote.dataset.signature === signature) { return; }
  hote.dataset.signature = signature;
  hote.innerHTML = '';
  for (const station of stations) {
    const ligne = document.createElement('div');
    ligne.className = 'ligne' + (station.encours ? ' encours' : '');
    ligne.innerHTML = `<span class="id"></span><span class="nom"></span>
      <span class="freq"></span><span class="quand"></span>`;
    ligne.children[0].textContent = station.id;
    ligne.children[1].textContent = station.nom || '';
    ligne.children[2].textContent = station.frequence ? `${station.frequence} kHz` : '—';
    ligne.children[3].textContent = station.quand;
    ligne.onclick = () => fetch(`/api/ecouter/${encodeURIComponent(station.id)}`, { method: 'POST' });
    hote.appendChild(ligne);
  }
}

function listeJournal(entrees) {
  const hote = $('journal');
  const signature = entrees.map((e) => e.at + e.station + e.rating).join('|');
  if (hote.dataset.signature === signature) { return; }
  hote.dataset.signature = signature;
  hote.innerHTML = '';
  for (const entree of entrees) {
    const ligne = document.createElement('div');
    ligne.className = 'ligne';
    ligne.innerHTML = `<span class="nom"></span><span class="freq"></span><span class="quand"></span>`;
    ligne.children[0].textContent = `${entree.at}  ${entree.station}`;
    ligne.children[1].textContent = `${entree.frequency_khz} kHz`;
    ligne.children[2].textContent = '★'.repeat(entree.rating) + (entree.tags ? ` ${entree.tags}` : '');
    hote.appendChild(ligne);
  }
}

// ----- Son ------------------------------------------------------------------
// Le serveur sert des échantillons bruts à 48 kHz, précédés de l'indice du
// premier d'entre eux. Le navigateur les enchaîne sans jamais recoller deux
// morceaux au même instant, ce qui éviterait les craquements.
async function pomper() {
  if (!joue) { return; }
  try {
    const reponse = await fetch(`/api/son?depuis=${horloge}&ms=700`);
    const octets = await reponse.arrayBuffer();
    if (octets.byteLength > 8) {
      const vue = new DataView(octets);
      const depart = Number(vue.getBigUint64(0, true));
      const nombre = (octets.byteLength - 8) / 2;
      const tampon = audio.createBuffer(1, nombre, 48000);
      const canal = tampon.getChannelData(0);
      for (let n = 0; n < nombre; n++) { canal[n] = vue.getInt16(8 + n * 2, true) / 32768; }
      horloge = depart + nombre;
      const source = audio.createBufferSource();
      source.buffer = tampon;
      source.connect(audio.destination);
      const maintenant = audio.currentTime;
      if (prochain < maintenant + 0.05) { prochain = maintenant + 0.25; }
      source.start(prochain);
      prochain += tampon.duration;
    }
  } catch (e) { /* une coupure réseau ne doit pas arrêter la boucle */ }
  setTimeout(pomper, 350);
}

$('son').onclick = async () => {
  if (joue) {
    joue = false;
    $('son').textContent = '▶ Écouter';
    await commande('quitter');
    return;
  }
  audio = audio || new (window.AudioContext || window.webkitAudioContext)({ sampleRate: 48000 });
  await audio.resume();
  horloge = etat ? etat.son_prochain : 0;
  prochain = 0;
  joue = true;
  $('son').textContent = '■ Couper';
  await commande('rejoindre');
  pomper();
};

$('enregistrer').onclick = () => commande('enregistrer', etat && etat.enregistre ? 0 : 1);
$('morseactif').onchange = (e) => commande('morse', e.target.checked ? 1 : 0);
$('effacer').onclick = () => commande('morse-effacer');
window.addEventListener('beforeunload', () => { if (joue) { navigator.sendBeacon('/api/commande/quitter'); } });

rafraichir();
setInterval(rafraichir, 250);
"#
    .to_string()
}
