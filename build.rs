#![forbid(unsafe_code)]

use fst::MapBuilder;
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use unicode_normalization::UnicodeNormalization;

fn normalize_jawi_key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.nfkc() {
        match ch {
            // Strip tatweel, Arabic harakat/vowels, zero-width & BiDi control marks.
            '\u{0640}'
            | '\u{064B}'..='\u{065F}'
            | '\u{0670}'
            | '\u{06D6}'..='\u{06ED}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}' => {}
            'ک' | 'ڪ' => out.push('ك'),
            'ی' | 'ے' => out.push('ي'),
            'گ' | 'ڬ' | 'ݣ' => out.push('ݢ'),
            'ھ' | 'ۀ' => out.push('ه'),
            'أ' | 'إ' | 'آ' | 'ٱ' => out.push('ا'),
            other => out.push(other),
        }
    }
    out
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let lexicon_path = manifest_dir
        .join("data")
        .join("lexicon")
        .join("dbp_jawi_rumi.tsv");
    println!("cargo:rerun-if-changed={}", lexicon_path.display());

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let fst_path = out_dir.join("jawi_lexicon.fst");
    let values_path = out_dir.join("jawi_values.txt");

    let mut map: BTreeMap<String, (String, String)> = BTreeMap::new();
    if let Ok(raw) = fs::read_to_string(&lexicon_path) {
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let cols: Vec<&str> = trimmed.split('\t').collect();
            let (raw_jawi, canonical_jawi, rumi) = match cols.len() {
                2 => (cols[0].trim(), cols[0].trim(), cols[1].trim()),
                n if n >= 3 => (cols[0].trim(), cols[1].trim(), cols[2].trim()),
                _ => continue,
            };
            if raw_jawi.is_empty() || rumi.is_empty() {
                continue;
            }
            let norm_key = normalize_jawi_key(raw_jawi);
            if !norm_key.is_empty() {
                map.entry(norm_key)
                    .or_insert_with(|| (canonical_jawi.to_string(), rumi.to_string()));
            }
        }
    }

    let fst_file = File::create(&fst_path).expect("create jawi_lexicon.fst");
    let mut builder = MapBuilder::new(BufWriter::new(fst_file)).expect("MapBuilder::new");
    let mut val_file = BufWriter::new(File::create(&values_path).expect("create jawi_values.txt"));

    for (idx, (key, (canonical, rumi))) in map.into_iter().enumerate() {
        builder
            .insert(key.as_bytes(), idx as u64)
            .expect("insert fst key");
        writeln!(val_file, "{canonical}\t{rumi}").expect("write jawi_values.txt");
    }
    builder.finish().expect("finish jawi_lexicon.fst");
    val_file.flush().expect("flush jawi_values.txt");
}
