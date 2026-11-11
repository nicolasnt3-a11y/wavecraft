//! Décodage du Morse à l'oreille de la machine.
//!
//! Les stations de nombres en Morse — M12, M14, M23… — émettent lentement et
//! proprement, mais la vitesse varie d'une station à l'autre et parfois au sein
//! d'un même message. Le décodeur ne suppose donc aucune vitesse : il mesure la
//! durée du point au fur et à mesure et s'y adapte.
//!
//! La chaîne est volontairement simple : un banc de filtres de Goertzel donne
//! l'énergie autour de la note Morse, la fréquence la plus forte est suivie —
//! l'opérateur n'a pas à régler la tonalité au hertz près —, un seuil adaptatif
//! décide si le manipulateur est abaissé, et la durée des traits et des
//! silences donne les lettres.

use std::f32::consts::TAU;

use crate::dsp::{AUDIO_RATE, CW_PITCH_HZ};

/// Taille du bloc d'analyse. 256 échantillons à 48 kHz font 5,3 ms, assez fin
/// pour un point de 60 ms (environ 20 mots/minute) et assez large pour que le
/// filtre soit sélectif.
const BLOCK: usize = 256;

/// Étendue du suivi de tonalité autour de la note réglée, en hertz.
const PITCH_SPAN_HZ: f32 = 200.0;
/// Pas entre deux filtres du banc, en hertz.
const PITCH_STEP_HZ: f32 = 25.0;

/// Durée de point admise, en blocs.
const MIN_DOT_BLOCKS: f32 = 4.0; // ≈ 21 ms, soit 57 mots/minute
const MAX_DOT_BLOCKS: f32 = 94.0; // ≈ 500 ms, soit 2,4 mots/minute

/// Table du Morse international, limitée à ce qu'émettent les stations de
/// nombres : chiffres, lettres et quelques signes de service.
const TABLE: &[(&str, char)] = &[
    (".-", 'A'),
    ("-...", 'B'),
    ("-.-.", 'C'),
    ("-..", 'D'),
    (".", 'E'),
    ("..-.", 'F'),
    ("--.", 'G'),
    ("....", 'H'),
    ("..", 'I'),
    (".---", 'J'),
    ("-.-", 'K'),
    (".-..", 'L'),
    ("--", 'M'),
    ("-.", 'N'),
    ("---", 'O'),
    (".--.", 'P'),
    ("--.-", 'Q'),
    (".-.", 'R'),
    ("...", 'S'),
    ("-", 'T'),
    ("..-", 'U'),
    ("...-", 'V'),
    (".--", 'W'),
    ("-..-", 'X'),
    ("-.--", 'Y'),
    ("--..", 'Z'),
    (".----", '1'),
    ("..---", '2'),
    ("...--", '3'),
    ("....-", '4'),
    (".....", '5'),
    ("-....", '6'),
    ("--...", '7'),
    ("---..", '8'),
    ("----.", '9'),
    ("-----", '0'),
    (".-.-.-", '.'),
    ("--..--", ','),
    ("..--..", '?'),
    ("-..-.", '/'),
    ("-...-", '='), // séparateur BT, courant dans les messages chiffrés
    (".-.-.", '+'),
];

/// Traduit une suite de points et de traits en caractère.
pub fn decode_symbol(symbols: &str) -> Option<char> {
    TABLE
        .iter()
        .find(|(code, _)| *code == symbols)
        .map(|(_, c)| *c)
}

/// Décodeur de Morse à vitesse adaptative.
pub struct MorseDecoder {
    /// Banc de filtres : fréquence suivie et coefficient de Goertzel.
    bank: Vec<(f32, f32)>,
    /// Tonalité suivie, en hertz.
    pitch: f32,
    buffer: Vec<f32>,
    /// Enveloppe lissée de l'énergie reçue à la note Morse.
    level: f32,
    floor: f32,
    ceiling: f32,
    /// Manipulateur abaissé au bloc précédent.
    keyed: bool,
    /// Durée de l'état courant, en blocs.
    run: f32,
    /// Durée estimée d'un point, en blocs.
    dot: f32,
    symbols: String,
    /// Vrai quand une lettre a été close et le silence continue : évite de
    /// répéter l'espace entre mots.
    word_done: bool,
    text: String,
}

impl Default for MorseDecoder {
    fn default() -> Self {
        Self::new(CW_PITCH_HZ, AUDIO_RATE)
    }
}

impl MorseDecoder {
    pub fn new(pitch_hz: f32, sample_rate: u32) -> Self {
        // Un filtre tous les 25 Hz autour de la note réglée : de quoi suivre un
        // émetteur qui dérive ou un accord approximatif, sans aller chercher le
        // bruit à l'autre bout de la bande.
        let mut bank = Vec::new();
        let mut freq = (pitch_hz - PITCH_SPAN_HZ).max(PITCH_STEP_HZ);
        while freq <= pitch_hz + PITCH_SPAN_HZ {
            let k = (BLOCK as f32 * freq / sample_rate as f32).round();
            // La fréquence réellement analysée est celle du canal retenu.
            let exacte = k * sample_rate as f32 / BLOCK as f32;
            bank.push((exacte, 2.0 * (TAU * k / BLOCK as f32).cos()));
            freq += PITCH_STEP_HZ;
        }
        bank.dedup_by(|a, b| (a.0 - b.0).abs() < 0.5);
        Self {
            bank,
            pitch: pitch_hz,
            buffer: Vec::with_capacity(BLOCK),
            level: 0.0,
            floor: 0.0,
            ceiling: 0.0,
            keyed: false,
            run: 0.0,
            dot: 12.0, // ≈ 64 ms, environ 19 mots/minute, le temps de mesurer
            symbols: String::new(),
            word_done: true,
            text: String::new(),
        }
    }

    /// Énergie du bloc à une fréquence donnée, par le filtre de Goertzel.
    fn goertzel(block: &[f32], coefficient: f32) -> f32 {
        let mut s1 = 0.0;
        let mut s2 = 0.0;
        for &sample in block {
            let s0 = sample + coefficient * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        ((s1 * s1 + s2 * s2 - coefficient * s1 * s2) / block.len() as f32).max(0.0)
    }

    /// Fréquence la plus forte du banc, et son énergie.
    fn strongest(&self, block: &[f32]) -> (f32, f32) {
        let mut meilleure = (self.pitch, 0.0);
        for &(freq, coefficient) in &self.bank {
            let energie = Self::goertzel(block, coefficient);
            if energie > meilleure.1 {
                meilleure = (freq, energie);
            }
        }
        meilleure
    }

    /// Tonalité suivie, en hertz.
    pub fn pitch_hz(&self) -> f32 {
        self.pitch
    }

    /// Analyse un bloc d'audio et renvoie le texte nouvellement décodé.
    pub fn process(&mut self, audio: &[f32]) -> String {
        let mut produced = String::new();
        for &sample in audio {
            self.buffer.push(sample);
            if self.buffer.len() < BLOCK {
                continue;
            }
            let (freq, energie) = self.strongest(&self.buffer);
            self.buffer.clear();
            self.step(energie.sqrt(), freq, &mut produced);
        }
        produced
    }

    fn step(&mut self, energy: f32, freq: f32, produced: &mut String) {
        // Lissage court : le manipulateur ne bouge pas en quelques millisecondes.
        self.level += (energy - self.level) * 0.35;

        // Le plancher suit le bruit, le plafond suit les traits : le seuil se
        // place à mi-chemin, ce qui s'adapte à un signal qui monte et descend.
        if self.level < self.floor {
            self.floor = self.level;
        } else {
            self.floor += (self.level - self.floor) * 0.0005;
        }
        if self.level > self.ceiling {
            self.ceiling += (self.level - self.ceiling) * 0.2;
        } else {
            self.ceiling += (self.level - self.ceiling) * 0.0005;
        }

        let span = self.ceiling - self.floor;
        // Sans contraste, il n'y a pas de Morse : on ne décode pas du bruit.
        //
        // Le bruit seul donne un plafond à peine au-dessus du plancher ; un
        // manipulateur donne un rapport bien plus grand. Exiger un facteur
        // trois — environ 10 dB — évite de transcrire des parasites, au prix
        // des signaux vraiment enfouis, qui de toute façon ne se décodent pas.
        if span < 1e-4 || self.ceiling < self.floor * 3.0 {
            self.run += 1.0;
            // Ce qui était commencé sous le bruit n'a pas de valeur.
            self.symbols.clear();
            return;
        }
        let keyed = self.level > self.floor + span * 0.5;
        if keyed {
            // La tonalité ne se suit que pendant un trait : pendant un silence,
            // le plus fort du banc n'est que du bruit.
            self.pitch += (freq - self.pitch) * 0.05;
        }

        if keyed == self.keyed {
            self.run += 1.0;
            // Un silence très long clôt le mot en cours.
            if !keyed && !self.word_done && self.run > self.dot * 6.0 {
                self.flush_letter(produced);
                produced.push(' ');
                self.text.push(' ');
                self.word_done = true;
            }
            return;
        }

        let length = self.run;
        self.run = 1.0;
        self.keyed = keyed;

        if keyed {
            // Fin d'un silence : sa durée sépare les symboles, les lettres ou
            // les mots.
            if length >= self.dot * 2.0 {
                self.flush_letter(produced);
            }
        } else {
            // Fin d'un trait : sa durée distingue le point du trait.
            if length < MIN_DOT_BLOCKS {
                return; // trop court pour être un symbole : parasite
            }
            self.learn_dot(length);
            self.symbols.push(if length < self.dot * 2.0 { '.' } else { '-' });
            self.word_done = false;
        }
    }

    /// Affine la durée du point d'après le trait qui vient de passer.
    fn learn_dot(&mut self, length: f32) {
        let estimate = if length < self.dot * 2.0 {
            length // un point
        } else {
            length / 3.0 // un trait vaut trois points
        };
        if (MIN_DOT_BLOCKS..=MAX_DOT_BLOCKS).contains(&estimate) {
            self.dot += (estimate - self.dot) * 0.25;
        }
    }

    fn flush_letter(&mut self, produced: &mut String) {
        if self.symbols.is_empty() {
            return;
        }
        let letter = decode_symbol(&self.symbols).unwrap_or('?');
        produced.push(letter);
        self.text.push(letter);
        self.symbols.clear();
    }

    /// Tout le texte décodé depuis le début de la session.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Vitesse estimée, en mots par minute (référence PARIS).
    pub fn wpm(&self) -> f32 {
        let dot_seconds = self.dot * BLOCK as f32 / AUDIO_RATE as f32;
        if dot_seconds <= 0.0 {
            0.0
        } else {
            1.2 / dot_seconds
        }
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.symbols.clear();
    }
}

/// Traduit les chiffres abrégés des opérateurs en chiffres pleins.
///
/// Pour gagner du temps, les stations en Morse raccourcissent les chiffres :
/// `T` pour 0, `A` pour 1, `N` pour 9… Les groupes de cinq chiffres deviennent
/// alors des suites de lettres. La conversion ne s'applique qu'aux lettres de
/// cette table ; le reste du texte passe tel quel.
pub fn cut_numbers(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'T' => '0',
            'A' => '1',
            'U' => '2',
            'V' => '3',
            'E' => '5',
            'B' => '7',
            'D' => '8',
            'N' => '9',
            autre => autre,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fabrique l'audio d'un message en Morse, à la vitesse demandée.
    fn morse_audio(text: &str, wpm: f32, amplitude: f32) -> Vec<f32> {
        morse_audio_pitch(text, wpm, amplitude, CW_PITCH_HZ)
    }

    /// Même chose, à une tonalité choisie.
    fn morse_audio_pitch(text: &str, wpm: f32, amplitude: f32, pitch: f32) -> Vec<f32> {
        let dot = (1.2 / wpm * AUDIO_RATE as f32) as usize;
        let mut audio = Vec::new();
        let tone = |length: usize, on: bool, audio: &mut Vec<f32>| {
            let start = audio.len();
            for n in 0..length {
                let t = (start + n) as f32 / AUDIO_RATE as f32;
                audio.push(if on {
                    amplitude * (TAU * pitch * t).sin()
                } else {
                    0.0
                });
            }
        };

        tone(dot * 10, false, &mut audio); // silence initial
        for (index, c) in text.chars().enumerate() {
            if c == ' ' {
                tone(dot * 7, false, &mut audio);
                continue;
            }
            let code = TABLE
                .iter()
                .find(|(_, letter)| *letter == c)
                .map(|(code, _)| *code)
                .unwrap_or("");
            for (position, symbol) in code.chars().enumerate() {
                if position > 0 {
                    tone(dot, false, &mut audio);
                }
                tone(if symbol == '-' { dot * 3 } else { dot }, true, &mut audio);
            }
            if index + 1 < text.len() {
                tone(dot * 3, false, &mut audio);
            }
        }
        tone(dot * 12, false, &mut audio); // silence final, pour clore la lettre
        audio
    }

    #[test]
    fn la_table_couvre_chiffres_et_lettres() {
        for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".chars() {
            assert!(
                TABLE.iter().any(|(_, letter)| *letter == c),
                "{c} absent de la table"
            );
        }
        assert_eq!(decode_symbol("...-"), Some('V'));
        assert_eq!(decode_symbol("-----"), Some('0'));
        assert_eq!(decode_symbol("......."), None);
    }

    #[test]
    fn un_indicatif_est_decode() {
        let audio = morse_audio("VC", 18.0, 0.5);
        let mut decoder = MorseDecoder::default();
        decoder.process(&audio);
        assert_eq!(decoder.text().trim(), "VC", "décodé : {:?}", decoder.text());
    }

    #[test]
    fn un_groupe_de_cinq_chiffres_est_decode() {
        // C'est le cœur du métier : M12 émet des groupes de cinq chiffres.
        let audio = morse_audio("37125", 16.0, 0.4);
        let mut decoder = MorseDecoder::default();
        decoder.process(&audio);
        assert_eq!(decoder.text().trim(), "37125", "décodé : {:?}", decoder.text());
    }

    #[test]
    fn la_vitesse_est_estimee_correctement() {
        let audio = morse_audio("PARIS", 20.0, 0.5);
        let mut decoder = MorseDecoder::default();
        decoder.process(&audio);
        let wpm = decoder.wpm();
        assert!(
            (wpm - 20.0).abs() < 4.0,
            "vitesse estimée à {wpm:.1} mots/minute au lieu de 20"
        );
    }

    #[test]
    fn deux_mots_sont_separes() {
        let audio = morse_audio("VC 123", 18.0, 0.5);
        let mut decoder = MorseDecoder::default();
        decoder.process(&audio);
        assert!(
            decoder.text().contains("VC") && decoder.text().contains("123"),
            "décodé : {:?}",
            decoder.text()
        );
        assert!(decoder.text().contains(' '), "mots collés : {:?}", decoder.text());
    }

    #[test]
    fn une_vitesse_lente_est_suivie() {
        // M12 émet souvent lentement ; le décodeur ne doit pas supposer 20 wpm.
        let audio = morse_audio("8213", 8.0, 0.5);
        let mut decoder = MorseDecoder::default();
        decoder.process(&audio);
        assert_eq!(decoder.text().trim(), "8213", "décodé : {:?}", decoder.text());
    }

    #[test]
    fn les_chiffres_abreges_se_traduisent() {
        // « TANVE » lu par un opérateur, c'est « 01935 ».
        assert_eq!(cut_numbers("TANVE"), "01935");
        assert_eq!(cut_numbers("UBD"), "278");
        // Un texte sans chiffre abrégé n'est pas abîmé.
        assert_eq!(cut_numbers("37125 88888"), "37125 88888");
        assert_eq!(cut_numbers("M12 "), "M12 ");
    }

    #[test]
    fn la_tonalite_est_suivie() {
        // Un message émis 60 Hz au-dessus de la note réglée est décodé, et le
        // décodeur annonce la tonalité qu'il a trouvée.
        let mut decoder = MorseDecoder::new(CW_PITCH_HZ, AUDIO_RATE);
        let audio = morse_audio_pitch("37125", 16.0, 0.5, CW_PITCH_HZ + 60.0);
        decoder.process(&audio);
        assert_eq!(decoder.text().trim(), "37125");
        assert!(
            (decoder.pitch_hz() - (CW_PITCH_HZ + 60.0)).abs() < 30.0,
            "tonalité suivie : {} Hz",
            decoder.pitch_hz()
        );
    }

    #[test]
    fn le_bruit_seul_ne_produit_pas_de_texte() {
        let mut graine = 12_345u32;
        let bruit: Vec<f32> = (0..AUDIO_RATE as usize * 2)
            .map(|_| {
                graine ^= graine << 13;
                graine ^= graine >> 17;
                graine ^= graine << 5;
                // Du bruit fort : c'est celui-là qui piège un décodeur trop
                // confiant, pas le murmure d'une bande vide.
                (graine as f32 / u32::MAX as f32 - 0.5) * 0.6
            })
            .collect();
        let mut decoder = MorseDecoder::default();
        decoder.process(&bruit);
        assert!(
            decoder.text().trim().is_empty(),
            "du bruit a été pris pour du Morse : {:?}",
            decoder.text()
        );
    }
}
