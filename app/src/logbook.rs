//! Journal d'écoute.
//!
//! Chaque session d'écoute laisse une entrée : quand, quelle station, quelle
//! fréquence, ce qui a été entendu. C'est la tradition de l'écoute d'ondes
//! courtes, et le seul moyen de retrouver une émission des mois plus tard.
//!
//! Le fichier est au format JSON par lignes : une entrée par ligne, ajoutée à
//! la fin. Rien n'est réécrit, donc rien n'est perdu si l'application s'arrête
//! brutalement, et le fichier reste lisible dans un éditeur de texte.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Une session d'écoute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Identifiant unique : l'horodatage de début au millième de seconde.
    pub id: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub station: String,
    pub frequency_khz: u32,
    pub mode: String,
    #[serde(default)]
    pub bandwidth_hz: u32,
    /// Niveau moyen du signal, en dB pleine échelle.
    #[serde(default)]
    pub level_dbfs: f32,
    /// Texte décodé en Morse pendant la session.
    #[serde(default)]
    pub decoded: String,
    /// Note libre de l'auditeur.
    #[serde(default)]
    pub notes: String,
    /// Appréciation, de 0 à 5 étoiles ; 0 quand rien n'a été noté.
    #[serde(default)]
    pub rating: u8,
    /// Étiquettes libres : « nuit », « première écoute »…
    #[serde(default)]
    pub tags: Vec<String>,
    /// Enregistrement audio associé, s'il existe.
    #[serde(default)]
    pub recording: Option<PathBuf>,
}

impl Entry {
    pub fn new(station: &str, frequency_khz: u32, mode: &str) -> Self {
        let started_at = Utc::now();
        Self {
            id: started_at.format("%Y%m%d-%H%M%S%.3f").to_string(),
            started_at,
            ended_at: None,
            station: station.to_string(),
            frequency_khz,
            mode: mode.to_string(),
            bandwidth_hz: 0,
            level_dbfs: -120.0,
            decoded: String::new(),
            notes: String::new(),
            rating: 0,
            tags: Vec::new(),
            recording: None,
        }
    }

    /// Durée de la session, si elle est close.
    pub fn duration(&self) -> Option<chrono::TimeDelta> {
        self.ended_at.map(|end| end - self.started_at)
    }

    pub fn close(&mut self) {
        self.ended_at = Some(Utc::now());
    }
}

/// Formats d'export du journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Export {
    Csv,
    Json,
    Markdown,
}

impl Export {
    pub fn extension(self) -> &'static str {
        match self {
            Export::Csv => "csv",
            Export::Json => "json",
            Export::Markdown => "md",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Export::Csv => "CSV",
            Export::Json => "JSON",
            Export::Markdown => "Markdown",
        }
    }
}

/// Échappe un champ pour le CSV : le séparateur est le point-virgule, et les
/// commentaires peuvent contenir des guillemets ou des retours à la ligne.
fn csv(champ: &str) -> String {
    if champ.contains(';') || champ.contains('"') || champ.contains('\n') {
        format!("\"{}\"", champ.replace('"', "\"\""))
    } else {
        champ.to_string()
    }
}

/// Le journal, chargé en mémoire et ajouté au fil de l'eau sur disque.
pub struct Logbook {
    path: PathBuf,
    entries: Vec<Entry>,
}

impl Logbook {
    pub fn path() -> PathBuf {
        crate::paths::data_dir().join("logbook.jsonl")
    }

    pub fn open() -> Result<Self> {
        Self::open_at(&Self::path())
    }

    /// Ouvre le journal. Une ligne illisible est sautée plutôt que de faire
    /// échouer l'ouverture : un journal partiellement abîmé reste utile.
    pub fn open_at(path: &Path) -> Result<Self> {
        let mut entries = Vec::new();
        if path.exists() {
            let file = std::fs::File::open(path)
                .with_context(|| format!("lecture du journal ({})", path.display()))?;
            for line in BufReader::new(file).lines() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<Entry>(&line) {
                    Ok(entry) => entries.push(entry),
                    Err(err) => eprintln!("journal : ligne ignorée ({err})"),
                }
            }
        }
        Ok(Self {
            path: path.to_path_buf(),
            entries,
        })
    }

    /// Ajoute une entrée et l'écrit aussitôt.
    pub fn append(&mut self, entry: Entry) -> Result<()> {
        crate::paths::ensure_parent(&self.path)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("ouverture du journal ({})", self.path.display()))?;
        writeln!(file, "{}", serde_json::to_string(&entry)?)
            .with_context(|| format!("écriture du journal ({})", self.path.display()))?;
        self.entries.push(entry);
        Ok(())
    }

    /// Remplace une entrée existante et réécrit le fichier.
    ///
    /// Sert à clore une session ou à ajouter une note : ce sont les seuls cas
    /// où le journal est réécrit.
    pub fn update(&mut self, entry: Entry) -> Result<()> {
        match self.entries.iter_mut().find(|e| e.id == entry.id) {
            Some(existing) => *existing = entry,
            None => return self.append(entry),
        }
        self.rewrite()
    }

    fn rewrite(&self) -> Result<()> {
        crate::paths::ensure_parent(&self.path)?;
        // Écriture dans un fichier temporaire puis remplacement : une coupure
        // au mauvais moment ne laisse pas un journal à moitié écrit.
        let temporary = self.path.with_extension("jsonl.tmp");
        {
            let mut file = std::fs::File::create(&temporary)?;
            for entry in &self.entries {
                writeln!(file, "{}", serde_json::to_string(entry)?)?;
            }
        }
        std::fs::rename(&temporary, &self.path)
            .with_context(|| format!("remplacement du journal ({})", self.path.display()))
    }

    /// Entrées de la plus récente à la plus ancienne.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().rev()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Recherche libre : station, mode, note ou texte décodé.
    pub fn search(&self, needle: &str) -> Vec<&Entry> {
        let needle = needle.trim().to_lowercase();
        if needle.is_empty() {
            return self.entries().collect();
        }
        self.entries()
            .filter(|e| {
                e.station.to_lowercase().contains(&needle)
                    || e.mode.to_lowercase().contains(&needle)
                    || e.notes.to_lowercase().contains(&needle)
                    || e.tags.iter().any(|t| t.to_lowercase().contains(&needle))
                    || e.decoded.to_lowercase().contains(&needle)
                    || e.frequency_khz.to_string().contains(&needle)
            })
            .collect()
    }

    /// Toutes les étiquettes utilisées, classées par ordre alphabétique.
    pub fn tags(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .entries
            .iter()
            .flat_map(|e| e.tags.iter().cloned())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Exporte le journal au format demandé.
    pub fn export(&self, format: Export) -> String {
        let entries: Vec<&Entry> = self.entries().collect();
        match format {
            Export::Csv => {
                let mut out = String::from(
                    "debut_utc;fin_utc;station;frequence_khz;mode;note;\
                     etiquettes;enregistrement;decode;commentaire\n",
                );
                for e in entries {
                    out.push_str(&format!(
                        "{};{};{};{};{};{};{};{};{};{}\n",
                        e.started_at.format("%Y-%m-%d %H:%M:%S"),
                        e.ended_at
                            .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
                            .unwrap_or_default(),
                        csv(&e.station),
                        e.frequency_khz,
                        csv(&e.mode),
                        e.rating,
                        csv(&e.tags.join(",")),
                        csv(&e
                            .recording
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default()),
                        csv(&e.decoded),
                        csv(&e.notes),
                    ));
                }
                out
            }
            Export::Json => serde_json::to_string_pretty(&entries).unwrap_or_default(),
            Export::Markdown => {
                let mut out = String::from("# Journal d'écoute Wavecraft\n\n");
                out.push_str("| Date (UTC) | Station | Fréq. | Mode | Note | Étiquettes |\n");
                out.push_str("|---|---|---|---|---|---|\n");
                for e in entries {
                    out.push_str(&format!(
                        "| {} | {} | {} kHz | {} | {} | {} |\n",
                        e.started_at.format("%Y-%m-%d %H:%M"),
                        e.station,
                        e.frequency_khz,
                        e.mode,
                        "★".repeat(e.rating.min(5) as usize),
                        e.tags.join(", "),
                    ));
                }
                out
            }
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn fichier(nom: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("wavecraft-test-log-{nom}.jsonl"));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn une_entree_s_ecrit_et_se_relit() {
        let path = fichier("aller-retour");
        let mut journal = Logbook::open_at(&path).unwrap();
        assert!(journal.is_empty());

        let mut entree = Entry::new("S28", 4625, "USB");
        entree.notes = "porteuse seule, pas de voix".into();
        journal.append(entree.clone()).unwrap();

        let relu = Logbook::open_at(&path).unwrap();
        assert_eq!(relu.len(), 1);
        assert_eq!(relu.entries().next().unwrap(), &entree);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn les_entrees_sortent_de_la_plus_recente_a_la_plus_ancienne() {
        let path = fichier("ordre");
        let mut journal = Logbook::open_at(&path).unwrap();
        journal.append(Entry::new("S28", 4625, "USB")).unwrap();
        journal.append(Entry::new("M12", 6917, "CW")).unwrap();
        let ids: Vec<&str> = journal.entries().map(|e| e.station.as_str()).collect();
        assert_eq!(ids, vec!["M12", "S28"]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn une_session_se_clot_et_la_duree_est_connue() {
        let path = fichier("cloture");
        let mut journal = Logbook::open_at(&path).unwrap();
        let mut entree = Entry::new("E11", 6804, "USB");
        journal.append(entree.clone()).unwrap();
        assert!(entree.duration().is_none());

        entree.close();
        entree.decoded = "37125".into();
        journal.update(entree.clone()).unwrap();

        let relu = Logbook::open_at(&path).unwrap();
        assert_eq!(relu.len(), 1, "la mise à jour a dupliqué l'entrée");
        let lu = relu.entries().next().unwrap();
        assert!(lu.ended_at.is_some());
        assert_eq!(lu.decoded, "37125");
        assert!(lu.duration().unwrap().num_milliseconds() >= 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn une_ligne_abimee_ne_perd_pas_le_reste() {
        let path = fichier("abime");
        let mut journal = Logbook::open_at(&path).unwrap();
        journal.append(Entry::new("S30", 5448, "USB")).unwrap();
        // Une ligne tronquée, comme après une coupure de courant.
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{{\"id\":\"casse\"").unwrap();
        drop(file);

        let relu = Logbook::open_at(&path).unwrap();
        assert_eq!(relu.len(), 1, "l'entrée valide doit survivre");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn la_recherche_porte_sur_tous_les_champs() {
        let path = fichier("recherche");
        let mut journal = Logbook::open_at(&path).unwrap();
        let mut a = Entry::new("S28", 4625, "USB");
        a.notes = "le bourdon, comme toujours".into();
        let mut b = Entry::new("M12", 6917, "CW");
        b.decoded = "37125 88888".into();
        journal.append(a).unwrap();
        journal.append(b).unwrap();

        assert_eq!(journal.search("bourdon").len(), 1);
        assert_eq!(journal.search("37125").len(), 1);
        assert_eq!(journal.search("4625").len(), 1);
        assert_eq!(journal.search("cw").len(), 1);
        assert_eq!(journal.search("").len(), 2);
        assert!(journal.search("introuvable").is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn le_journal_s_exporte_dans_les_trois_formats() {
        let path = fichier("export");
        let mut journal = Logbook::open_at(&path).unwrap();
        let mut entree = Entry::new("S28", 4625, "USB");
        entree.rating = 4;
        entree.tags = vec!["nuit".into(), "première écoute".into()];
        entree.notes = "porteuse ; puis la voix".into();
        journal.append(entree).unwrap();

        let csv = journal.export(Export::Csv);
        let lignes: Vec<&str> = csv.lines().collect();
        assert_eq!(lignes.len(), 2, "en-tête et une entrée attendues");
        assert!(lignes[0].starts_with("debut_utc;"));
        // Le point-virgule du commentaire est protégé par des guillemets : les
        // neuf premières colonnes restent lisibles telles quelles.
        let commentaire = "\"porteuse ; puis la voix\"";
        let colonnes = lignes[1]
            .strip_suffix(commentaire)
            .expect("le commentaire doit être en dernière colonne, entre guillemets")
            .strip_suffix(';')
            .expect("le commentaire est séparé des autres colonnes");
        assert_eq!(colonnes.split(';').count(), 9, "{colonnes}");
        assert!(colonnes.contains(";S28;4625;USB;4;"), "{colonnes}");

        let json = journal.export(Export::Json);
        assert!(json.contains("\"station\": \"S28\""), "{json}");

        let md = journal.export(Export::Markdown);
        assert!(md.contains("★★★★"), "{md}");
        assert!(md.contains("4625 kHz"), "{md}");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn les_etiquettes_se_retrouvent() {
        let path = fichier("etiquettes");
        let mut journal = Logbook::open_at(&path).unwrap();
        let mut a = Entry::new("S28", 4625, "USB");
        a.tags = vec!["nuit".into()];
        let mut b = Entry::new("M12", 6917, "CW");
        b.tags = vec!["morse".into(), "nuit".into()];
        journal.append(a).unwrap();
        journal.append(b).unwrap();
        assert_eq!(journal.tags(), vec!["morse".to_string(), "nuit".to_string()]);
        assert_eq!(journal.search("morse").len(), 1);
        let _ = std::fs::remove_file(&path);
    }

}
