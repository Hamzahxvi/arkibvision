#![forbid(unsafe_code)]

//! Historical Malay & Southeast Asian writing system decoder:
//! - Jawi (Traditional Malay-Arabic script) -> delegated to `crate::jawi`
//! - Pallava Script (Ancient South Indian / Srivijaya inscription script, 4th-8th century)
//! - Kawi Script (Unicode U+11F00..U+11F5F, Old Malay & Javanese archival manuscripts)
//! - Rencong / Surat Ulu Script (Unicode Rejang U+A930..U+A95F, traditional Sumatran/Malay script)

use crate::jawi::decode_jawi_text;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AncientScript {
    Jawi,
    Pallava,
    Kawi,
    Rencong,
    LatinOrUnknown,
}

impl AncientScript {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jawi => "jawi",
            Self::Pallava => "pallava",
            Self::Kawi => "kawi",
            Self::Rencong => "rencong",
            Self::LatinOrUnknown => "unknown",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Jawi => "Jawi (Traditional Malay-Arabic Script)",
            Self::Pallava => {
                "Pallava Script (Aksara Pallava — Ancient Srivijaya / Malay Peninsula Inscriptions)"
            }
            Self::Kawi => "Kawi Script (Aksara Kawi — Classical Malay Archipelago Manuscripts)",
            Self::Rencong => {
                "Rencong / Surat Ulu Script (Aksara Rencong / Rejang — Traditional Sumatran-Malay Script)"
            }
            Self::LatinOrUnknown => "Latin / Transliterated Archival Text",
        }
    }

    pub fn historical_context(self) -> &'static str {
        match self {
            Self::Jawi => {
                "Adapted Arabic script used across the Malay world since at least the Terengganu Inscription Stone (1303 CE) for royal decrees, treaties, and classical Malay manuscripts."
            }
            Self::Pallava => {
                "Ancient South Indian Brahmic script (4th–8th century CE) used in the earliest Old Malay inscriptions such as Kedukan Bukit (683 CE), Talang Tuo (684 CE), Kota Kapur (686 CE), and Bujang Valley / Sungai Mas inscriptions in Kedah."
            }
            Self::Kawi => {
                "Southeast Asian Brahmic script derived from Pallava (8th–16th century CE), widely used across the Malay Archipelago for Old Malay and Javanese stone inscriptions, copperplates, and palm-leaf manuscripts."
            }
            Self::Rencong => {
                "Traditional indigenous Brahmic-derived script (Surat Ulu / KaGaNga family) inscribed on bamboo, tree bark, and horn across Sumatra and Malay heritage regions."
            }
            Self::LatinOrUnknown => "Archival transcription or romanized text.",
        }
    }
}

pub fn detect_script(text: &str) -> AncientScript {
    let mut arabic = 0usize;
    let mut kawi = 0usize;
    let mut rencong = 0usize;
    let mut grantha_indic = 0usize;

    for c in text.chars() {
        let cp = c as u32;
        match cp {
            0x0600..=0x06FF
            | 0x0750..=0x077F
            | 0x08A0..=0x08FF
            | 0xFB50..=0xFDFF
            | 0xFE70..=0xFEFF => {
                arabic += 1;
            }
            0x11F00..=0x11F5F => kawi += 1,
            0xA930..=0xA95F => rencong += 1,
            0x11300..=0x1137F | 0x0900..=0x097F | 0x0B80..=0x0BFF => grantha_indic += 1,
            _ => {}
        }
    }

    if kawi > 0 && kawi >= rencong && kawi >= arabic {
        return AncientScript::Kawi;
    }
    if rencong > 0 && rencong >= arabic {
        return AncientScript::Rencong;
    }
    if arabic > 0 {
        return AncientScript::Jawi;
    }
    if grantha_indic > 0 {
        return AncientScript::Pallava;
    }

    // Heuristic check for romanized Old Malay / Pallava inscription vocabulary.
    let low = text.to_ascii_lowercase();
    if low.contains("svasti")
        || low.contains("shaka")
        || low.contains("saka")
        || low.contains("sriwijaya")
        || low.contains("srivijaya")
        || low.contains("dapunta")
        || low.contains("kedukan")
        || low.contains("vanua")
        || low.contains("marvuat")
        || low.contains("niyur")
    {
        return AncientScript::Pallava;
    }

    AncientScript::LatinOrUnknown
}

/// Transliterate Unicode Kawi (U+11F00..U+11F5F) into searchable Romanized Old Malay / Malay.
pub fn transliterate_kawi(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() * 2);
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if let Some(cons) = kawi_consonant(c) {
            let mut vowel = Some("a");
            let mut coda = String::new();
            let mut j = i + 1;
            while j < chars.len() {
                match chars[j] {
                    '\u{11F41}' | '\u{11F42}' => {
                        // Kawi killer / conjoiner (virama) suppresses inherent 'a'.
                        vowel = None;
                        j += 1;
                        break;
                    }
                    '\u{11F34}' => {
                        vowel = Some("aa");
                        j += 1;
                    }
                    '\u{11F35}' => {
                        vowel = Some("aa");
                        j += 1;
                    }
                    '\u{11F36}' => {
                        vowel = Some("i");
                        j += 1;
                    }
                    '\u{11F37}' => {
                        vowel = Some("ii");
                        j += 1;
                    }
                    '\u{11F38}' => {
                        vowel = Some("u");
                        j += 1;
                    }
                    '\u{11F39}' => {
                        vowel = Some("uu");
                        j += 1;
                    }
                    '\u{11F3A}' => {
                        vowel = Some("re");
                        j += 1;
                    }
                    '\u{11F3E}' => {
                        vowel = Some("e");
                        j += 1;
                    }
                    '\u{11F3F}' => {
                        vowel = Some("ai");
                        j += 1;
                    }
                    '\u{11F00}' => {
                        coda.push_str("ng");
                        j += 1;
                    } // candrabindu
                    '\u{11F01}' => {
                        coda.push_str("ng");
                        j += 1;
                    } // anusvara
                    '\u{11F02}' => {
                        coda.push('r');
                        j += 1;
                    } // repha
                    '\u{11F03}' => {
                        coda.push('h');
                        j += 1;
                    } // visarga
                    _ => break,
                }
            }
            out.push_str(cons);
            if let Some(v) = vowel {
                out.push_str(v);
            }
            out.push_str(&coda);
            i = j;
            continue;
        }

        if let Some(indep) = kawi_other(c) {
            out.push_str(indep);
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

fn kawi_consonant(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{11F12}' => "k",
        '\u{11F13}' => "kh",
        '\u{11F14}' => "g",
        '\u{11F15}' => "gh",
        '\u{11F16}' => "ng",
        '\u{11F17}' => "c",
        '\u{11F18}' => "ch",
        '\u{11F19}' => "j",
        '\u{11F1A}' => "jh",
        '\u{11F1B}' => "ny",
        '\u{11F1C}' => "t",
        '\u{11F1D}' => "th",
        '\u{11F1E}' => "d",
        '\u{11F1F}' => "dh",
        '\u{11F20}' => "n",
        '\u{11F21}' => "t",
        '\u{11F22}' => "th",
        '\u{11F23}' => "d",
        '\u{11F24}' => "dh",
        '\u{11F25}' => "n",
        '\u{11F26}' => "p",
        '\u{11F27}' => "ph",
        '\u{11F28}' => "b",
        '\u{11F29}' => "bh",
        '\u{11F2A}' => "m",
        '\u{11F2B}' => "y",
        '\u{11F2C}' => "r",
        '\u{11F2D}' => "l",
        '\u{11F2E}' => "w",
        '\u{11F2F}' => "sh",
        '\u{11F30}' => "s",
        '\u{11F31}' => "s",
        '\u{11F32}' => "h",
        '\u{11F33}' => "jnya",
        _ => return None,
    })
}

fn kawi_other(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{11F04}' => "a",
        '\u{11F05}' => "aa",
        '\u{11F06}' => "i",
        '\u{11F07}' => "ii",
        '\u{11F08}' => "u",
        '\u{11F09}' => "uu",
        '\u{11F0A}' => "re",
        '\u{11F0B}' => "ree",
        '\u{11F0C}' => "le",
        '\u{11F0D}' => "lee",
        '\u{11F0E}' => "e",
        '\u{11F0F}' => "ai",
        '\u{11F10}' => "o",
        '\u{11F43}' => ",",
        '\u{11F44}' => ".",
        '\u{11F50}' => "0",
        '\u{11F51}' => "1",
        '\u{11F52}' => "2",
        '\u{11F53}' => "3",
        '\u{11F54}' => "4",
        '\u{11F55}' => "5",
        '\u{11F56}' => "6",
        '\u{11F57}' => "7",
        '\u{11F58}' => "8",
        '\u{11F59}' => "9",
        _ => return None,
    })
}

/// Transliterate Unicode Rejang / Rencong (U+A930..U+A95F) into searchable Romanized Malay.
pub fn transliterate_rencong(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() * 2);
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if let Some(cons) = rencong_consonant(c) {
            let mut vowel = Some("a");
            let mut coda = String::new();
            let mut j = i + 1;
            while j < chars.len() {
                match chars[j] {
                    '\u{A953}' => {
                        // Virama suppresses inherent 'a'.
                        vowel = None;
                        j += 1;
                        break;
                    }
                    '\u{A947}' => {
                        vowel = Some("i");
                        j += 1;
                    }
                    '\u{A948}' => {
                        vowel = Some("u");
                        j += 1;
                    }
                    '\u{A949}' => {
                        vowel = Some("e");
                        j += 1;
                    }
                    '\u{A94A}' => {
                        vowel = Some("ai");
                        j += 1;
                    }
                    '\u{A94B}' => {
                        vowel = Some("o");
                        j += 1;
                    }
                    '\u{A94C}' => {
                        vowel = Some("au");
                        j += 1;
                    }
                    '\u{A94D}' => {
                        vowel = Some("eu");
                        j += 1;
                    }
                    '\u{A94E}' => {
                        vowel = Some("ea");
                        j += 1;
                    }
                    '\u{A94F}' => {
                        coda.push_str("ng");
                        j += 1;
                    }
                    '\u{A950}' => {
                        coda.push('n');
                        j += 1;
                    }
                    '\u{A951}' => {
                        coda.push('r');
                        j += 1;
                    }
                    '\u{A952}' => {
                        coda.push('h');
                        j += 1;
                    }
                    _ => break,
                }
            }
            out.push_str(cons);
            if let Some(v) = vowel {
                out.push_str(v);
            }
            out.push_str(&coda);
            i = j;
            continue;
        }
        if c == '\u{A95F}' {
            out.push('.');
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

fn rencong_consonant(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{A930}' => "k",
        '\u{A931}' => "g",
        '\u{A932}' => "ng",
        '\u{A933}' => "t",
        '\u{A934}' => "d",
        '\u{A935}' => "n",
        '\u{A936}' => "p",
        '\u{A937}' => "b",
        '\u{A938}' => "m",
        '\u{A939}' => "c",
        '\u{A93A}' => "j",
        '\u{A93B}' => "ny",
        '\u{A93C}' => "s",
        '\u{A93D}' => "r",
        '\u{A93E}' => "l",
        '\u{A93F}' => "y",
        '\u{A940}' => "w",
        '\u{A941}' => "h",
        '\u{A942}' => "mb",
        '\u{A943}' => "ngg",
        '\u{A944}' => "nd",
        '\u{A945}' => "nj",
        '\u{A946}' => "", // 'a' vowel carrier
        _ => return None,
    })
}

/// Normalize Old Malay / Pallava archival transliterations and provide modern Malay equivalents.
fn gloss_old_malay_terms(text: &str) -> Vec<Value> {
    const GLOSSARY: &[(&str, &str, &str)] = &[
        (
            "svasti",
            "Selamat sejahtera / Salam pembukaan",
            "Auspicious greeting / Prosperity",
        ),
        ("sriwijaya", "Kerajaan Srivijaya", "Srivijaya Empire"),
        ("srivijaya", "Kerajaan Srivijaya", "Srivijaya Empire"),
        (
            "dapunta",
            "Baginda / Yang Dipertuan (gelaran diraja)",
            "Royal title / Lord",
        ),
        ("hyang", "Yang Maha Mulia / Suci", "Sacred / Divine"),
        (
            "vanua",
            "Benua / Negeri / Wilayah",
            "Land / Settlement / Realm",
        ),
        ("marvuat", "Membuat / Membina", "To build / To make"),
        (
            "saka",
            "Tahun Saka (kalendar sejarah +78 M)",
            "Shaka Era year (+78 CE)",
        ),
        (
            "shaka",
            "Tahun Saka (kalendar sejarah +78 M)",
            "Shaka Era year (+78 CE)",
        ),
        ("niyur", "Nyiur / Kelapa", "Coconut palm"),
        ("vulan", "Bulan", "Month / Moon"),
        (
            "siddhayatra",
            "Perjalanan suci / kejayaan",
            "Sacred journey / Spiritual success",
        ),
        (
            "datu",
            "Datuk / Pemerintah wilayah",
            "Chieftain / Regional ruler",
        ),
        ("kadatuan", "Kedatuan / Kerajaan", "Kingdom / Palace realm"),
    ];
    let low = text.to_ascii_lowercase();
    let mut found = Vec::new();
    for &(term, ms, en) in GLOSSARY {
        if low.contains(term) {
            found.push(json!({
                "term": term,
                "modern_malay": ms,
                "english": en,
            }));
        }
    }
    found
}

/// Unified decoder for Jawi, Pallava, Kawi, and Rencong scripts.
pub fn decode_ancient_script(text: &str, script_hint: &str) -> Value {
    let hint = script_hint.trim().to_ascii_lowercase();
    let script = match hint.as_str() {
        "jawi" => AncientScript::Jawi,
        "pallava" | "palava" => AncientScript::Pallava,
        "kawi" => AncientScript::Kawi,
        "rencong" | "rejang" | "surat_ulu" | "surat ulu" => AncientScript::Rencong,
        _ => detect_script(text),
    };

    if script == AncientScript::Jawi {
        let mut jawi_res =
            decode_jawi_text(text.to_string()).unwrap_or_else(|e| json!({ "error": e }));
        if let Some(obj) = jawi_res.as_object_mut() {
            obj.insert("detected_script".to_string(), json!(script.as_str()));
            obj.insert("script_name".to_string(), json!(script.display_name()));
            obj.insert(
                "historical_context".to_string(),
                json!(script.historical_context()),
            );
        }
        return jawi_res;
    }

    let lines: Vec<String> = text
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let transliterated_lines: Vec<String> = lines
        .iter()
        .map(|line| match script {
            AncientScript::Kawi => transliterate_kawi(line),
            AncientScript::Rencong => transliterate_rencong(line),
            AncientScript::Pallava => normalize_pallava_text(line),
            _ => line.clone(),
        })
        .collect();

    let combined = transliterated_lines.join("\n");
    let glossary_matches = gloss_old_malay_terms(&combined);

    json!({
        "detected_script": script.as_str(),
        "script_name": script.display_name(),
        "historical_context": script.historical_context(),
        "line_count": lines.len(),
        "original_lines": lines,
        "transliterated_rumi_lines": transliterated_lines,
        "searchable_digital_text": combined,
        "archival_glossary_matches": glossary_matches,
        "preservation_note": "Preserve original orthographic forms alongside searchable modern Bahasa Melayu and English translation without inventing illegible segments; mark damaged carvings/strokes with [?].",
    })
}

fn normalize_pallava_text(line: &str) -> String {
    // Normalize common Devanagari/Grantha/Pallava diacritic romanizations into clean searchable text.
    line.replace('ā', "a")
        .replace('ī', "i")
        .replace('ū', "u")
        .replace(['ś', 'ṣ'], "s")
        .replace('ñ', "ny")
        .replace('ṅ', "ng")
        .replace('ṭ', "t")
        .replace('ḍ', "d")
        .replace('ṇ', "n")
        .replace('ṃ', "m")
        .replace('ḥ', "h")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kawi_and_rencong_transliteration() {
        // Kawi: ka + wi (𑼒𑼮𑼶) -> "kawi"
        let kawi_sample = "\u{11F12}\u{11F2E}\u{11F36}";
        assert_eq!(detect_script(kawi_sample), AncientScript::Kawi);
        assert_eq!(transliterate_kawi(kawi_sample), "kawi");

        // Rencong/Rejang: ka + ga + nga (ꤰꤱꤲ) -> "kaganga"
        let rencong_sample = "\u{A930}\u{A931}\u{A932}";
        assert_eq!(detect_script(rencong_sample), AncientScript::Rencong);
        assert_eq!(transliterate_rencong(rencong_sample), "kaganga");

        // Pallava Old Malay inscription detection & glossary
        let pallava = "Svasti śrī śakavarṣātīta 605 dapunta hyang marvuat vanua śrīvijaya";
        let res = decode_ancient_script(pallava, "auto");
        assert_eq!(res["detected_script"], "pallava");
        assert!(res["archival_glossary_matches"].as_array().unwrap().len() >= 4);
    }
}
