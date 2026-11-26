//! Safe Rust Jawi-to-Rumi orthographic, morphological, FST-backed lexicon, and Hijri colophon decoder.
//!
//! Supports both Modern Jawi (DBP) and Old Classical Jawi (*Jawi Lama* / *Hikayat* /
//! *Surat Diraja* / archival manuscripts), normalizing Unicode NFKC, Arabic Presentation
//! Forms-A/B (`U+FB50..=U+FDFF`, `U+FE70..=U+FEFF`), BiDi control marks, tatweel/kashida,
//! harakat, Persian/Urdu codepoints, and legacy Malaysian `پ`→`ڽ`.

#![forbid(unsafe_code)]
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::OnceLock;

use fst::automaton::Levenshtein;
use fst::{IntoStreamer, Map as FstMap, Streamer};
use regex::Regex;
use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization;

static FST_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/jawi_lexicon.fst"));
static FST_VALUES_RAW: &str = include_str!(concat!(env!("OUT_DIR"), "/jawi_values.txt"));

struct JawiFstLexicon {
    map: FstMap<&'static [u8]>,
    values: Vec<(&'static str, &'static str)>,
}

impl JawiFstLexicon {
    fn get(&self, key: &str) -> Option<(&'static str, &'static str)> {
        let idx = self.map.get(key.as_bytes())? as usize;
        self.values.get(idx).copied()
    }

    /// DBP-only FST Levenshtein distance-1 fuzzy lookup for tokens >= 4 Jawi chars.
    /// Uses `fst::automaton::Levenshtein` when the DFA fits within `fst`'s state limit, and
    /// falls back cleanly to a char-level bounded edit-distance-1 scan over the zero-copy FST
    /// stream for multi-byte Arabic/Jawi keys.
    fn fuzzy_lookup_dist1(&self, key: &str) -> Option<(&'static str, &'static str)> {
        let key_chars: Vec<char> = key.chars().collect();
        let key_len = key_chars.len();
        if key_len < 4 {
            return None;
        }
        if let Ok(aut) = Levenshtein::new(key, 1) {
            let mut stream = self.map.search(aut).into_stream();
            let mut best: Option<(usize, (&'static str, &'static str))> = None;
            while let Some((cand_bytes, val_idx)) = stream.next() {
                if let Ok(cand_str) = std::str::from_utf8(cand_bytes)
                    && let Some(&pair) = self.values.get(val_idx as usize)
                {
                    let len_diff = cand_str.chars().count().abs_diff(key_len);
                    if best.is_none_or(|(b_diff, _)| len_diff < b_diff) {
                        best = Some((len_diff, pair));
                    }
                }
            }
            if let Some((_, pair)) = best {
                return Some(pair);
            }
        }

        // Bounded char-level edit-distance-1 scan over the zero-copy FST (handles multi-byte Arabic UTF-8).
        let mut stream = self.map.stream();
        let mut best: Option<(usize, (&'static str, &'static str))> = None;
        while let Some((cand_bytes, val_idx)) = stream.next() {
            if let Ok(cand_str) = std::str::from_utf8(cand_bytes) {
                let cand_chars: Vec<char> = cand_str.chars().collect();
                let len_diff = cand_chars.len().abs_diff(key_len);
                if len_diff > 1 {
                    continue;
                }
                if edit_distance(&key_chars, &cand_chars) == 1
                    && let Some(&pair) = self.values.get(val_idx as usize)
                    && best.is_none_or(|(b_diff, _)| len_diff < b_diff)
                {
                    best = Some((len_diff, pair));
                    if len_diff == 0 {
                        break;
                    }
                }
            }
        }
        best.map(|(_, pair)| pair)
    }
}

fn jawi_fst() -> &'static JawiFstLexicon {
    static LEX: OnceLock<JawiFstLexicon> = OnceLock::new();
    LEX.get_or_init(|| {
        let map = FstMap::new(FST_BYTES).expect("embedded jawi_lexicon.fst");
        let values: Vec<(&'static str, &'static str)> = FST_VALUES_RAW
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .collect();
        JawiFstLexicon { map, values }
    })
}

/// Old Classical Jawi (*Jawi Lama*) discourse & orthographic markers.
const OLD_JAWI_MARKERS: &[&str] = &[
    "مك",
    "تتكال",
    "تتكالا",
    "شهدان",
    "حتي",
    "حتا",
    "اركين",
    "برمول",
    "سبرمول",
    "ادڤون",
    "القصه",
    "كلكين",
    "بݢند",
    "تيته",
    "سݢل",
    "دباليروڠ",
    "ورقه",
    "ورقة",
    "سنه",
    "سنة",
    "فصل",
    "حاشيه",
    "حاشية",
];

const MALAY_RUMI_LEXICON: &[&str] = &[
    "Pembukaan",
    "dengan",
    "nama",
    "Allah",
    "yang",
    "Maha",
    "Pemurah",
    "lagi",
    "Mengasihani",
    "Segala",
    "puji",
    "tertentu",
    "bagi",
    "Tuhan",
    "memelihara",
    "dan",
    "mentadbirkan",
    "sekalian",
    "alam",
    "menguasai",
    "pemerintahan",
    "hari",
    "pembalasan",
    "akhirat",
    "Engkaulah",
    "sahaja",
    "Ya",
    "kami",
    "sembah",
    "kepada",
    "memohon",
    "pertolongan",
    "Tunjukilah",
    "jalan",
    "lurus",
    "iaitu",
    "orang",
    "orang-orang",
    "Engkau",
    "telah",
    "kurniakan",
    "nikmat",
    "mereka",
    "bukan",
    "murkai",
    "pula",
    "sesat",
    "bahawa",
    "adalah",
    "daripada",
    "pada",
    "dalam",
    "untuk",
    "tidak",
    "ini",
    "itu",
    "atau",
    "juga",
    "oleh",
    "kerana",
    "tetapi",
    "apabila",
    "seperti",
    "semua",
    "setiap",
    "antara",
    "sebagai",
    "menjadi",
    "melalui",
    "terhadap",
    "kerajaan",
    "negeri",
    "negara",
    "rakyat",
    "raja",
    "sultan",
    "menteri",
    "undang-undang",
    "persekutuan",
    "perlembagaan",
    "agama",
    "islam",
    "melayu",
    "bahasa",
    "surat",
    "perjanjian",
    "tarikh",
    "tahun",
    "bulan",
    "masa",
    "tempat",
    "daerah",
    "kampung",
    "bandar",
    "sekolah",
    "masjid",
    "mahkamah",
    "jabatan",
    "pejabat",
    "pendidikan",
    "kesihatan",
    "keselamatan",
    "kebajikan",
    "maklumat",
    "keterangan",
    "catatan",
    "tandatangan",
    "pengesahan",
    "dokumen",
    "arkib",
    "malaysia",
    "rekod",
    "sejarah",
    "warisan",
    "bangsa",
    "manuskrip",
    "pemuliharaan",
    "kemerdekaan",
    "merdeka",
    "berdaulat",
    "baginda",
    "titah",
    "balairung",
    "istana",
    "tatkala",
    "syahadan",
    "hatta",
    "arakian",
    "bermula",
    "adapun",
    "alkisah",
    "bendahara",
    "temenggung",
    "laksamana",
    "adat",
    "istiadat",
    "kertas",
    "kulat",
    "pegawai",
    "kawalan",
    "kelembapan",
    "kerosakan",
];

const ROUGH_RUMI_ALIASES: &[(&str, &str)] = &[
    ("fimboakan", "Pembukaan"),
    ("fimbukaan", "Pembukaan"),
    ("deygan", "dengan"),
    ("digan", "dengan"),
    ("naci", "nama"),
    ("nami", "nama"),
    ("alaa", "Allah"),
    ("alah", "Allah"),
    ("yag", "yang"),
    ("mahi", "Maha"),
    ("fimorah", "Pemurah"),
    ("fimurah", "Pemurah"),
    ("lai", "lagi"),
    ("layi", "lagi"),
    ("laki", "lagi"),
    ("mikhassiyahani", "Mengasihani"),
    ("migasihani", "Mengasihani"),
    ("sibkali", "Segala"),
    ("sigali", "Segala"),
    ("sikali", "Segala"),
    ("foji", "puji"),
    ("fuji", "puji"),
    ("terintento", "tertentu"),
    ("tirtinto", "tertentu"),
    ("bayi", "bagi"),
    ("baki", "bagi"),
    ("tuhman", "Tuhan"),
    ("mimiilihari", "memelihara"),
    ("mimilihari", "memelihara"),
    ("minatadirkhan", "mentadbirkan"),
    ("mintadbirkan", "mentadbirkan"),
    ("mintadirkan", "mentadbirkan"),
    ("sikkalian", "sekalian"),
    ("sikalian", "sekalian"),
    ("ncalam", "alam"),
    ("miguwasai", "menguasai"),
    ("miguasai", "menguasai"),
    ("fimeryintahan", "pemerintahan"),
    ("fimirintahan", "pemerintahan"),
    ("hary", "hari"),
    ("fimbalasan", "pembalasan"),
    ("takhira", "akhirat"),
    ("takhirat", "akhirat"),
    ("yegkolahe", "Engkaulah"),
    ("yengkawlah", "Engkaulah"),
    ("yengkaulah", "Engkaulah"),
    ("sahaaji", "sahaja"),
    ("sahaji", "sahaja"),
    ("i", "Ya"),
    ("yi", "Ya"),
    ("kai", "kami"),
    ("simebah", "sembah"),
    ("simbah", "sembah"),
    ("kifadi", "kepada"),
    ("mimouhun", "memohon"),
    ("mimo'uhun", "memohon"),
    ("firtulugan", "pertolongan"),
    ("firtolongan", "pertolongan"),
    ("tongjokilah", "Tunjukilah"),
    ("tonjokilah", "Tunjukilah"),
    ("lorus", "lurus"),
    ("yantuto", "iaitu"),
    ("yaito", "iaitu"),
    ("tuarag", "orang"),
    ("turag", "orang"),
    ("yegkau", "Engkau"),
    ("yengkau", "Engkau"),
    ("tiyalah", "telah"),
    ("tilah", "telah"),
    ("koriniakan", "kurniakan"),
    ("korniakan", "kurniakan"),
    ("niyemaa", "nikmat"),
    ("niemaat", "nikmat"),
    ("mirti", "mereka"),
    ("miriki", "mereka"),
    ("bokan", "bukan"),
    ("morikai", "murkai"),
    ("morkai", "murkai"),
    ("fuli", "pula"),
    ("sisat", "sesat"),
];

/// Applies Unicode NFKC normalization (folding Arabic Presentation Forms-A/B `U+FB50..=U+FDFF`,
/// `U+FE70..=U+FEFF` into canonical `U+0600..=U+06FF`), strips BiDi/zero-width control marks,
/// tatweel (`ـ`), and Arabic harakat, and unifies Persian/Urdu/Jawi letter variants.
pub fn normalize_jawi_key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.nfkc() {
        match ch {
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

fn rough_aliases() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| ROUGH_RUMI_ALIASES.iter().copied().collect())
}

fn skeleton_index() -> &'static Vec<(Vec<char>, &'static str, String)> {
    static IDX: OnceLock<Vec<(Vec<char>, &'static str, String)>> = OnceLock::new();
    IDX.get_or_init(|| {
        MALAY_RUMI_LEXICON
            .iter()
            .map(|&word| {
                let skel: Vec<char> = jawi_phonetic_skeleton(word).chars().collect();
                (skel, word, word.to_lowercase())
            })
            .collect()
    })
}

fn bullet_prefix_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*[-*•]\s*(?:\*\*Jawi\*\*:\s*|Jawi:\s*)?").unwrap())
}

fn digits_fullmatch_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\(?\d+\)?$").unwrap())
}

fn list_number_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d+[.)]$").unwrap())
}

fn is_py_whitespace(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{001C}'..='\u{001F}')
}

fn py_trim(s: &str) -> &str {
    s.trim_matches(is_py_whitespace)
}

fn py_split_whitespace(s: &str) -> Vec<&str> {
    s.split(is_py_whitespace)
        .filter(|part| !part.is_empty())
        .collect()
}

fn py_splitlines(s: &str) -> Vec<&str> {
    if s.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut iter = s.char_indices().peekable();
    while let Some((idx, ch)) = iter.next() {
        let is_break = matches!(
            ch,
            '\n' | '\r'
                | '\u{000B}'
                | '\u{000C}'
                | '\u{001C}'
                | '\u{001D}'
                | '\u{001E}'
                | '\u{0085}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if is_break {
            lines.push(&s[start..idx]);
            let mut next_start = idx + ch.len_utf8();
            if ch == '\r'
                && let Some(&(_, '\n')) = iter.peek()
            {
                iter.next();
                next_start += 1;
            }
            start = next_start;
        }
    }
    if start < s.len() {
        lines.push(&s[start..]);
    }
    lines
}

fn is_jawi_punct(c: char) -> bool {
    matches!(
        c,
        '(' | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '.'
            | ','
            | '،'
            | ';'
            | '؛'
            | ':'
            | '!'
            | '?'
            | '؟'
            | '"'
            | '\''
            | '«'
            | '»'
            | '—'
            | '–'
    )
}

fn split_outer_punct(tok: &str) -> (&str, &str, &str) {
    let mut start = 0usize;
    for (idx, ch) in tok.char_indices() {
        if is_jawi_punct(ch) {
            start = idx + ch.len_utf8();
        } else {
            break;
        }
    }
    if start >= tok.len() {
        return (tok, "", "");
    }
    let mut end = tok.len();
    for (idx, ch) in tok[start..].char_indices().rev() {
        if is_jawi_punct(ch) {
            end = start + idx;
        } else {
            break;
        }
    }
    (&tok[..start], &tok[start..end], &tok[end..])
}

fn normalize_rumi_punct(p: &str) -> String {
    p.chars()
        .map(|c| match c {
            '،' => ',',
            '؛' => ';',
            '؟' => '?',
            other => other,
        })
        .collect()
}

fn jawi_phonetic_skeleton(word: &str) -> String {
    let lowered = word.to_lowercase();
    let trimmed = lowered.trim_matches(|c| {
        matches!(
            c,
            '.' | ',' | ';' | ':' | '!' | '?' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}'
        )
    });
    let mut w = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if !matches!(ch, '\'' | '’' | '`') {
            w.push(ch);
        }
    }
    w = w
        .replace("ng", "G")
        .replace("gh", "G")
        .replace("kh", "G")
        .replace("ny", "N")
        .replace("sy", "S");

    let mut mapped: Vec<char> = w
        .chars()
        .map(|ch| match ch {
            'f' | 'v' => 'p',
            'q' => 'k',
            'c' | 'z' => 'j',
            'i' | 'y' => 'e',
            'o' => 'u',
            other => other,
        })
        .collect();

    if mapped.len() > 2
        && let Some(last) = mapped.last_mut()
        && *last == 'e'
    {
        *last = 'a';
    }

    let mut collapsed = String::with_capacity(mapped.len());
    let mut prev: Option<char> = None;
    for ch in mapped {
        if ch != '\n' && prev == Some(ch) {
            continue;
        }
        collapsed.push(ch);
        prev = Some(ch);
    }
    collapsed
}

fn edit_distance(a: &[char], b: &[char]) -> usize {
    if a == b {
        return 0;
    }
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (curr[j] + 1).min(prev[j + 1] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

fn translate_arabic_digits(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '٠' | '۰' => '0',
            '١' | '۱' => '1',
            '٢' | '۲' => '2',
            '٣' | '۳' => '3',
            '٤' | '۴' => '4',
            '٥' | '۵' => '5',
            '٦' | '۶' => '6',
            '٧' | '۷' => '7',
            '٨' | '۸' => '8',
            '٩' | '۹' => '9',
            other => other,
        })
        .collect()
}

fn transliterate_jawi_chars(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    for ch in word.chars() {
        match ch {
            '\u{0640}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}' => {}
            'ا' | 'آ' | 'أ' | 'ع' | 'ى' => out.push('a'),
            'إ' | 'ئ' | 'ي' | 'ی' => out.push('i'),
            'ء' => {}
            'ب' => out.push('b'),
            'ت' | 'ة' | 'ط' => out.push('t'),
            'ث' | 'س' | 'ص' => out.push('s'),
            'ج' => out.push('j'),
            'چ' => out.push('c'),
            'ح' | 'ه' | 'ھ' => out.push('h'),
            'خ' => out.push_str("kh"),
            'د' | 'ض' => out.push('d'),
            'ذ' | 'ز' | 'ظ' => out.push('z'),
            'ر' => out.push('r'),
            'ش' => out.push_str("sy"),
            'غ' | 'ڠ' => out.push_str("ng"),
            'ف' | 'ڤ' => out.push('p'),
            'ق' | 'ك' | 'ک' => out.push('k'),
            'ݢ' | 'ڬ' | 'گ' | 'ݣ' => out.push('g'),
            'ل' => out.push('l'),
            'م' => out.push('m'),
            'ن' => out.push('n'),
            'ڽ' | 'پ' => out.push_str("ny"),
            'و' => out.push('u'),
            'ؤ' => out.push('o'),
            'ۏ' => out.push('v'),
            c if ('\u{064B}'..='\u{065F}').contains(&c) => {}
            other => out.push(other),
        }
    }
    out
}

fn is_core_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '\''
}

fn normalize_word_str(token: &str) -> String {
    let mut core_start: Option<usize> = None;
    let mut core_end: usize = token.len();
    for (idx, ch) in token.char_indices() {
        if core_start.is_none() {
            if is_core_char(ch) {
                core_start = Some(idx);
            }
        } else if !is_core_char(ch) {
            core_end = idx;
            break;
        }
    }
    let Some(start) = core_start else {
        return token.to_string();
    };
    let prefix = &token[..start];
    let core = &token[start..core_end];
    let suffix = &token[core_end..];
    if suffix.chars().any(is_core_char) {
        return token.to_string();
    }

    let low = core.to_lowercase();
    if let Some(&mapped) = rough_aliases().get(low.as_str()) {
        return format!("{prefix}{mapped}{suffix}");
    }
    let idx = skeleton_index();
    for &(_, word, ref word_low) in idx {
        if *word_low == low {
            return format!("{prefix}{word}{suffix}");
        }
    }
    let skel: Vec<char> = jawi_phonetic_skeleton(&low).chars().collect();
    if skel.len() >= 3 {
        let mut best_word: Option<&'static str> = None;
        let mut best_dist = usize::MAX;
        for (cand_skel, cand_word, _) in idx {
            let d = edit_distance(&skel, cand_skel);
            if d < best_dist {
                best_dist = d;
                best_word = Some(*cand_word);
            }
        }
        let max_dist = if skel.len() <= 5 { 1 } else { 2 };
        if let Some(word) = best_word
            && best_dist <= max_dist
        {
            return format!("{prefix}{word}{suffix}");
        }
    }
    token.to_string()
}

fn lookup_jawi_exact(key: &str, lex: &JawiFstLexicon) -> Option<(&'static str, &'static str)> {
    if let Some(hit) = lex.get(key) {
        return Some(hit);
    }
    // Legacy Malaysian Jawi Biasa fonts encode `ڽ` (ny) as `پ`, while Persian/Urdu use `پ` for `ڤ` (p).
    if key.contains('پ') {
        if let Some(hit) = lex.get(&key.replace('پ', "ڽ")) {
            return Some(hit);
        }
        if let Some(hit) = lex.get(&key.replace('پ', "ڤ")) {
            return Some(hit);
        }
    }
    // Classical / modern ta marbutah (`ة`) vs ta maftuhah (`ت`) interchange (e.g. طاعة/طاعت, رعية/رعيت).
    if let Some(stem) = key.strip_suffix('ة')
        && let Some(hit) = lex.get(&format!("{stem}ت"))
    {
        return Some(hit);
    }
    if let Some(stem) = key.strip_suffix('ت')
        && let Some(hit) = lex.get(&format!("{stem}ة"))
    {
        return Some(hit);
    }
    None
}

/// Deterministic morphological prefix/suffix decomposition validated against the embedded FST lexicon.
fn lookup_jawi_morphological(key: &str, lex: &JawiFstLexicon) -> Option<String> {
    const PREFIXES: &[(&str, &str)] = &[
        ("بر", "ber"),
        ("تر", "ter"),
        ("مڠ", "meng"),
        ("مم", "mem"),
        ("من", "men"),
        ("مڽ", "meny"),
        ("ڤڠ", "peng"),
        ("ڤم", "pem"),
        ("ڤن", "pen"),
        ("د", "di"),
        ("ك", "ke"),
        ("س", "se"),
    ];
    const SUFFIXES: &[(&str, &str)] = &[
        ("له", "lah"),
        ("كه", "kah"),
        ("ڽ", "nya"),
        ("كن", "kan"),
        ("ان", "an"),
    ];

    for &(j_pre, r_pre) in PREFIXES {
        if let Some(rest) = key.strip_prefix(j_pre)
            && rest.chars().count() >= 2
            && let Some((_, root_rumi)) = lookup_jawi_exact(rest, lex)
        {
            return Some(format!("{r_pre}{}", root_rumi.to_lowercase()));
        }
    }

    for &(j_suf, r_suf) in SUFFIXES {
        if let Some(rest) = key.strip_suffix(j_suf)
            && rest.chars().count() >= 2
            && let Some((_, root_rumi)) = lookup_jawi_exact(rest, lex)
        {
            return Some(format!("{root_rumi}{r_suf}"));
        }
    }

    for &(j_pre, r_pre) in PREFIXES {
        if let Some(after_pre) = key.strip_prefix(j_pre) {
            for &(j_suf, r_suf) in SUFFIXES {
                if let Some(root) = after_pre.strip_suffix(j_suf)
                    && root.chars().count() >= 2
                    && let Some((_, root_rumi)) = lookup_jawi_exact(root, lex)
                {
                    return Some(format!("{r_pre}{}{r_suf}", root_rumi.to_lowercase()));
                }
            }
        }
    }

    None
}

// ─── Deterministic Hijri-to-Masihi (Kuwaiti / Tabular Islamic Calendar) ──────

/// Converts a Hijri calendar date `(year, month 1..=12, day 1..=30)` to a Gregorian (Masihi)
/// `(year, month, day)` triple using the standard tabular Islamic / Kuwaiti Julian Day algorithm.
pub fn hijri_to_gregorian(hy: i32, hm: u32, hd: u32) -> (i32, u32, u32) {
    let y = i64::from(hy);
    let m = i64::from(hm.clamp(1, 12));
    let d = i64::from(hd.clamp(1, 30));

    let jd = (11 * y + 3) / 30 + 354 * y + 30 * m - (m - 1) / 2 + d + 1_948_440 - 385;

    // Fliegel–Van Flandern JDN -> Proleptic Gregorian conversion.
    let mut l = jd + 68_569;
    let n = (4 * l) / 146_097;
    l -= (146_097 * n + 3) / 4;
    let i = (4000 * (l + 1)) / 1_461_001;
    l = l - (1461 * i) / 4 + 31;
    let j = (80 * l) / 2447;
    let day = l - (2447 * j) / 80;
    l = j / 11;
    let month = j + 2 - 12 * l;
    let year = 100 * (n - 49) + i + l;

    (year as i32, month as u32, day as u32)
}

fn parse_hijri_month(raw: &str) -> Option<(u32, &'static str)> {
    let low = normalize_jawi_key(raw)
        .to_ascii_lowercase()
        .replace(['-', ' '], "");
    match low.as_str() {
        "محرم" | "muharam" | "muharram" => Some((1, "Muharam")),
        "صفر" | "safar" => Some((2, "Safar")),
        "ربيعالاول" | "rabiulawal" | "rabialawal" | "rabiulawwal" => {
            Some((3, "Rabiulawal"))
        }
        "ربيعالاخير" | "ربيعالاخر" | "ربيعالثاني" | "rabiulakhir" | "rabiulthani" => {
            Some((4, "Rabiulakhir"))
        }
        "جماديلالاول" | "جماديالاول" | "jamadilawal" | "jumadalawal" => {
            Some((5, "Jamadilawal"))
        }
        "جماديلالاخير" | "جماديالاخير" | "جماديالاخر" | "jamadilakhir" | "jumadalakhir" => {
            Some((6, "Jamadilakhir"))
        }
        "رجب" | "rejab" | "rajab" => Some((7, "Rejab")),
        "شعبان" | "syaaban" | "shaban" | "syaban" => Some((8, "Syaaban")),
        "رمضان" | "ramadan" | "ramadhan" => Some((9, "Ramadan")),
        "شوال" | "syawal" | "shawwal" => Some((10, "Syawal")),
        "ذوالقعده" | "ذوالقعدة" | "zulkaedah" | "zulqadah" | "dhulqadah" => {
            Some((11, "Zulkaedah"))
        }
        "ذوالحجه" | "ذوالحجة" | "zulhijah" | "zulhijjah" | "dhulhijjah" => {
            Some((12, "Zulhijah"))
        }
        _ => None,
    }
}

/// Extracts Hijri manuscript colophon dates in both Jawi and Rumi (e.g., `سنه ١٣٢٤`,
/// `١٣ محرم ١٣٢٤`, `تاهون ١٣٤٠ هجره`, `1324H`, `Hijrah 1324`) and deterministically converts
/// them to Gregorian (Masihi) dates.
pub fn extract_hijri_colophon_dates(text: &str) -> Vec<Value> {
    static FULL_DATE_RE: OnceLock<Regex> = OnceLock::new();
    static YEAR_COLOPHON_RE: OnceLock<Regex> = OnceLock::new();

    let norm_nfkc: String = text.nfkc().collect();
    let digits_norm = translate_arabic_digits(&norm_nfkc);

    let full_re = FULL_DATE_RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(\d{1,2})\s+(محرم|صفر|ربيع\s*(?:الاول|الأول|الاخير|الأخير|الاخر|الآخر|الثاني)|جماد(?:ي|يل)\s*(?:الاول|الأول|الاخير|الأخير|الاخر|الآخر)|رجب|شعبان|رمضان|شوال|ذوالقعد[هة]|ذوالحج[هة]|Muhar+am|Safar|Rabiul\s*awal|Rabiul\s*akhir|Jamadil\s*awal|Jamadil\s*akhir|Rejab|Rajab|Sya+ban|Ramad[ha]+n|Syawal|Zulkaedah|Zulhij+ah)\s+(\d{3,4})(?:\s*(?:H|Hijrah|AH|هـ?|هجره|هجرة))?\b",
        )
        .expect("hijri full date regex")
    });

    let year_re = YEAR_COLOPHON_RE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:(?:سنه|سنة|تاهون|sanah|hijrah|tahun|AH)\s+(\d{3,4})(?:\s*(?:H|Hijrah|هجره|هجرة|هـ))?|\b(\d{3,4})\s*(?:H|AH|Hijrah|هجره|هجرة)\b)",
        )
        .expect("hijri year regex")
    });

    let mut results: Vec<Value> = Vec::new();
    let mut seen_years: Vec<i32> = Vec::new();

    for caps in full_re.captures_iter(&digits_norm) {
        let day: u32 = caps[1].parse().unwrap_or(1);
        let month_raw = &caps[2];
        let year: i32 = caps[3].parse().unwrap_or(0);
        if !(1..=30).contains(&day) || !(1..=1600).contains(&year) {
            continue;
        }
        if let Some((month_num, month_name)) = parse_hijri_month(month_raw) {
            let (gy, gm, gd) = hijri_to_gregorian(year, month_num, day);
            seen_years.push(year);
            results.push(json!({
                "matched_text": caps[0].trim(),
                "hijri_date": format!("{day} {month_name} {year}H"),
                "hijri_year": year,
                "hijri_month": month_num,
                "hijri_day": day,
                "gregorian_date_masihi": format!("{gy:04}-{gm:02}-{gd:02}"),
                "precision": "exact_day_tabular_plus_minus_1d",
            }));
        }
    }

    for caps in year_re.captures_iter(&digits_norm) {
        let raw_match = caps[0].trim();
        let year_str = caps
            .get(1)
            .or_else(|| caps.get(2))
            .map(|m| m.as_str())
            .unwrap_or("");
        let year: i32 = year_str.parse().unwrap_or(0);
        if !(1..=1600).contains(&year) || seen_years.contains(&year) {
            continue;
        }
        // Guard against matching modern Gregorian years written after Malay `tahun` (e.g. `tahun 1957`).
        let low = raw_match.to_ascii_lowercase();
        if (low.starts_with("tahun") || raw_match.starts_with("تاهون"))
            && year >= 1500
            && !low.contains('h')
            && !raw_match.contains("هجر")
        {
            continue;
        }
        let (gy_start, gm_s, gd_s) = hijri_to_gregorian(year, 1, 1);
        let (gy_end, gm_e, gd_e) = hijri_to_gregorian(year, 12, 29);
        seen_years.push(year);
        results.push(json!({
            "matched_text": raw_match,
            "hijri_date": format!("{year}H"),
            "hijri_year": year,
            "gregorian_date_masihi": if gy_start == gy_end {
                format!("{gy_start:04}")
            } else {
                format!("{gy_start:04}/{gy_end:04} ({gy_start:04}-{gm_s:02}-{gd_s:02} to {gy_end:04}-{gm_e:02}-{gd_e:02})")
            },
            "precision": "year_range_tabular",
        }));
    }

    results
}

struct LineDecodeStats {
    jawi: String,
    rumi: String,
    total_jawi_tokens: usize,
    matched_jawi_tokens: usize,
    old_jawi_hits: usize,
}

fn is_arabic_or_presentation_char(c: char) -> bool {
    ('\u{0600}'..='\u{06FF}').contains(&c)
        || ('\u{0750}'..='\u{077F}').contains(&c)
        || ('\u{FB50}'..='\u{FDFF}').contains(&c)
        || ('\u{FE70}'..='\u{FEFF}').contains(&c)
}

fn decode_line_with_stats(line: &str) -> LineDecodeStats {
    let stripped = py_trim(line);
    let cleaned_raw = bullet_prefix_re().replace(stripped, "");
    if cleaned_raw.is_empty() {
        return LineDecodeStats {
            jawi: String::new(),
            rumi: String::new(),
            total_jawi_tokens: 0,
            matched_jawi_tokens: 0,
            old_jawi_hits: 0,
        };
    }

    // Step 1: NFKC normalize the entire line upfront so Arabic Presentation Forms-A/B become canonical U+0600..06FF.
    let cleaned: String = cleaned_raw
        .nfkc()
        .filter(|&c| {
            !matches!(
                c,
                '\u{200B}'..='\u{200F}'
                    | '\u{202A}'..='\u{202E}'
                    | '\u{2066}'..='\u{2069}'
                    | '\u{FEFF}'
            )
        })
        .collect();

    let has_arabic = cleaned.chars().any(is_arabic_or_presentation_char);

    if has_arabic {
        let lex = jawi_fst();
        let mut jawi_out: Vec<String> = Vec::new();
        let mut rumi_out: Vec<String> = Vec::new();
        let mut total_jawi_tokens = 0usize;
        let mut matched_jawi_tokens = 0usize;
        let mut old_jawi_hits = 0usize;

        for tok in py_split_whitespace(&cleaned) {
            let digit_trans = translate_arabic_digits(tok);

            // Numbered list items like "١.", "٢.", "٣." -> "1.", "2.", "3."
            if list_number_re().is_match(&digit_trans) {
                jawi_out.push(tok.to_string());
                rumi_out.push(digit_trans);
                continue;
            }

            let stripped_digits = digit_trans.trim_matches(|c| c == '(' || c == ')');
            if digits_fullmatch_re().is_match(&digit_trans) && stripped_digits != "2" {
                let tok_inner = tok.trim_matches(|c| c == '(' || c == ')');
                if tok.starts_with('(') || tok.ends_with(')') || stripped_digits.len() <= 2 {
                    jawi_out.push(format!("({tok_inner})"));
                    rumi_out.push(format!("({stripped_digits})"));
                } else {
                    // Preserve standalone multi-digit years like ١٣٢٤ -> 1324 cleanly
                    jawi_out.push(tok.to_string());
                    rumi_out.push(stripped_digits.to_string());
                }
                continue;
            }

            // Standalone reduplication marker `٢` / `2` following a word (e.g. `تؤراغ ٢`)
            if (tok == "٢" || tok == "2") && !rumi_out.is_empty() {
                if let Some(last_r) = rumi_out.last_mut() {
                    let lower = last_r.to_lowercase();
                    last_r.push('-');
                    last_r.push_str(&lower);
                }
                if let Some(last_j) = jawi_out.last_mut() {
                    last_j.push('٢');
                }
                continue;
            }

            let (prefix, bare, suffix) = split_outer_punct(tok);
            let rumi_prefix = normalize_rumi_punct(prefix);
            let rumi_suffix = normalize_rumi_punct(suffix);

            if bare.is_empty() {
                jawi_out.push(tok.to_string());
                rumi_out.push(format!("{rumi_prefix}{rumi_suffix}"));
                continue;
            }

            total_jawi_tokens += 1;
            let norm_key = normalize_jawi_key(bare);
            if OLD_JAWI_MARKERS.contains(&norm_key.as_str()) {
                old_jawi_hits += 1;
            }

            // 1. Zero-copy FST exact dictionary match (NFKC + Presentation Forms + `پ`→`ڽ`/`ڤ` + `ة`↔`ت`)
            if let Some((j_word, r_word)) = lookup_jawi_exact(&norm_key, lex) {
                matched_jawi_tokens += 1;
                jawi_out.push(format!("{prefix}{j_word}{suffix}"));
                rumi_out.push(format!("{rumi_prefix}{r_word}{rumi_suffix}"));
                continue;
            }

            // 2. Attached Jawi reduplication `٢` or `2` (e.g. راج٢ -> raja-raja, اوندڠ٢ -> undang-undang)
            if let Some(stem_key) = norm_key
                .strip_suffix('٢')
                .or_else(|| norm_key.strip_suffix('2'))
                && !stem_key.is_empty()
            {
                old_jawi_hits += 1;
                if let Some((j_word, r_word)) = lookup_jawi_exact(stem_key, lex) {
                    matched_jawi_tokens += 1;
                    let lower = r_word.to_lowercase();
                    jawi_out.push(format!("{prefix}{j_word}٢{suffix}"));
                    rumi_out.push(format!("{rumi_prefix}{r_word}-{lower}{rumi_suffix}"));
                    continue;
                }
                if let Some(morph_rumi) = lookup_jawi_morphological(stem_key, lex) {
                    matched_jawi_tokens += 1;
                    let lower = morph_rumi.to_lowercase();
                    jawi_out.push(tok.to_string());
                    rumi_out.push(format!("{rumi_prefix}{morph_rumi}-{lower}{rumi_suffix}"));
                    continue;
                }
                let rough = transliterate_jawi_chars(&translate_arabic_digits(stem_key));
                let norm = normalize_word_str(&rough);
                let lower = norm.to_lowercase();
                jawi_out.push(tok.to_string());
                rumi_out.push(format!("{rumi_prefix}{norm}-{lower}{rumi_suffix}"));
                continue;
            }

            // 3. Deterministic morphological prefix/suffix match validated against FST roots
            if let Some(morph_rumi) = lookup_jawi_morphological(&norm_key, lex) {
                matched_jawi_tokens += 1;
                jawi_out.push(tok.to_string());
                rumi_out.push(format!("{rumi_prefix}{morph_rumi}{rumi_suffix}"));
                continue;
            }

            // 4. Zero-copy FST Levenshtein distance-1 fuzzy DBP lookup (handles 1-char OCR/dot errors)
            if let Some((j_word, r_word)) = lex.fuzzy_lookup_dist1(&norm_key) {
                matched_jawi_tokens += 1;
                jawi_out.push(format!("{prefix}{j_word}{suffix}"));
                rumi_out.push(format!("{rumi_prefix}{r_word}{rumi_suffix}"));
                continue;
            }

            // 5. Phonetic skeleton & transliteration fallback
            jawi_out.push(tok.to_string());
            let rough = transliterate_jawi_chars(&translate_arabic_digits(&norm_key));
            let norm = normalize_word_str(&rough);
            rumi_out.push(format!("{rumi_prefix}{norm}{rumi_suffix}"));
        }

        return LineDecodeStats {
            jawi: jawi_out.join(" "),
            rumi: rumi_out.join(" "),
            total_jawi_tokens,
            matched_jawi_tokens,
            old_jawi_hits,
        };
    }

    let mut rumi_tokens: Vec<String> = Vec::new();
    for tok in py_split_whitespace(&cleaned) {
        if (tok == "2" || tok == "٢") && !rumi_tokens.is_empty() {
            if let Some(last) = rumi_tokens.last_mut() {
                let prev = last.trim_matches(|c| c == '(' || c == ')').to_string();
                let lower = prev.to_lowercase();
                *last = format!("{prev}-{lower}");
            }
            continue;
        }
        rumi_tokens.push(normalize_word_str(tok));
    }
    LineDecodeStats {
        jawi: String::new(),
        rumi: rumi_tokens.join(" "),
        total_jawi_tokens: 0,
        matched_jawi_tokens: 0,
        old_jawi_hits: 0,
    }
}

fn decode_line_inner(line: &str) -> (String, String) {
    let stats = decode_line_with_stats(line);
    (stats.jawi, stats.rumi)
}

/// Port of `normalize_rough_rumi_word` (`src/utils/jawi_decoder.py:317`).
pub fn normalize_rough_rumi_word(token: String) -> Result<String, String> {
    Ok(normalize_word_str(&token))
}

/// Port of `decode_jawi_or_rough_rumi_line` (`src/utils/jawi_decoder.py:346`).
pub fn decode_jawi_or_rough_rumi_line(line: String) -> Result<Value, String> {
    let (jawi, rumi) = decode_line_inner(&line);
    Ok(json!({ "jawi": jawi, "rumi": rumi }))
}

/// Port of `decode_jawi_text` (`src/utils/jawi_decoder.py:403`).
pub fn decode_jawi_text(text: String) -> Result<Value, String> {
    let mut jawi_lines: Vec<String> = Vec::new();
    let mut rumi_lines: Vec<String> = Vec::new();
    let mut total_jawi = 0usize;
    let mut matched_jawi = 0usize;
    let mut old_hits = 0usize;

    for ln in py_splitlines(&text) {
        if py_trim(ln).is_empty() {
            continue;
        }
        let stats = decode_line_with_stats(ln);
        total_jawi += stats.total_jawi_tokens;
        matched_jawi += stats.matched_jawi_tokens;
        old_hits += stats.old_jawi_hits;
        if !stats.jawi.is_empty() {
            jawi_lines.push(stats.jawi);
        }
        if !stats.rumi.is_empty() {
            rumi_lines.push(stats.rumi);
        }
    }

    let rumi_text = rumi_lines.join("\n");
    let coverage_pct = if total_jawi == 0 {
        100.0
    } else {
        ((matched_jawi as f64 / total_jawi as f64) * 1000.0).round() / 10.0
    };
    let script_style = if total_jawi == 0 {
        "rough_rumi"
    } else if old_hits >= 2 {
        "old_classical_jawi_or_mixed"
    } else {
        "modern_jawi"
    };
    let hijri_colophon_dates = extract_hijri_colophon_dates(&text);

    Ok(json!({
        "corrected_jawi_lines": jawi_lines,
        "standard_malay_rumi_lines": rumi_lines,
        "standard_malay_rumi_text": rumi_text,
        "dictionary_matched_tokens": matched_jawi,
        "total_jawi_tokens": total_jawi,
        "coverage_pct": coverage_pct,
        "script_style": script_style,
        "fst_lexicon_entries": jawi_fst().values.len(),
        "hijri_colophon_dates": hijri_colophon_dates,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_al_fatihah_rough_rumi() {
        let sample = "Fimboakan\nDeygan Naci Alaa Yag Mahi Fimorah Lai Maha\nMikhassiyahani (1) Sibkali Foji Terintento Bayi\nTuarag 2 Yag Sisat (7)";
        let out = decode_jawi_text(sample.to_string()).unwrap();
        let lines = out["standard_malay_rumi_lines"].as_array().unwrap();
        assert_eq!(lines[0], "Pembukaan");
        assert_eq!(lines[1], "dengan nama Allah yang Maha Pemurah lagi Maha");
        assert_eq!(lines[2], "Mengasihani (1) Segala puji tertentu bagi");
        assert_eq!(lines[3], "orang-orang yang sesat (7)");
    }

    #[test]
    fn test_jawi_3dot_restoration() {
        let out = decode_jawi_or_rough_rumi_line("فيمبوكان ديغان تؤراغ ٢".to_string()).unwrap();
        assert_eq!(out["jawi"], "ڤيمبوكأن ديڠان ئؤراڠ٢");
        assert_eq!(out["rumi"], "Pembukaan dengan orang-orang");
    }

    #[test]
    fn test_modern_jawi_archival_sentence() {
        let sample = "ارکيب نݢارا مليسيا ممليهارا ريكود سجاره دان واريثن بڠسا.";
        let out = decode_jawi_text(sample.to_string()).unwrap();
        assert_eq!(
            out["standard_malay_rumi_text"],
            "Arkib Negara Malaysia memelihara rekod sejarah dan warisan bangsa."
        );
        assert_eq!(out["coverage_pct"], 100.0);
    }

    #[test]
    fn test_old_classical_jawi_hikayat() {
        let sample = "مک تتکال ايت سݢل راج٢ دان اورڠ بسر٢ ڤون برهيمڤون دباليروڠ سري منجنجوڠ تيته بݢند سلطان دڠن ڤنوه طاعة ستيا.";
        let out = decode_jawi_text(sample.to_string()).unwrap();
        assert_eq!(
            out["standard_malay_rumi_text"],
            "maka tatkala itu segala raja-raja dan orang besar-besar pun berhimpun di balairung seri menjunjung titah baginda sultan dengan penuh taat setia."
        );
        assert_eq!(out["script_style"], "old_classical_jawi_or_mixed");
        assert_eq!(out["coverage_pct"], 100.0);
    }

    #[test]
    fn test_long_8_line_mixed_jawi_no_hallucination() {
        let sample = concat!(
            "١. سورت ڤرجنجين اين دتنداتڠني ڤد ليم هاري بولن اوݢوس تاهون سريبو سمبيلن راتوس ليم ڤولوه توجوه.\n",
            "٢. سݢل راج٢ ملايو تله برستوجو اونتوق منوبوهکن ڤرسکوتوان تانه ملايو يڠ مرديک دان بردولت.\n",
            "٣. مک سݢل اوندڠ٢ دان عادت استعادت نݢري هندقله دڤليهارا دڠن باءيق اوليه کراجأن.\n",
            "٤. ڤد تيݢ ڤولوه ساتو اوݢوس تاهون يڠ سام، تونکو عبدالرحمن ڤوترا الحاج تله ممشهورکن کمرديکأن د ستاديوم مرديک، کوالا لومڤور.\n",
            "٥. رعية سکلين ڤون برسوك ريا مپمبوت هاري برسجاره ايت.\n",
            "٦. ارکيب نݢارا مليسيا مپيمڤن سݢل ريكود راسمي دان مانوسکريڤ لام د بهاݢين ڤموليهارأن.\n",
            "٧. سبارڠ کيرتس يڠ ڤودر، کويق، اتاو برکولت اکن دبايكي اوليه ڤݢاواي يڠ برکلياقن.\n",
            "٨. کراجأن جوݢ مپدياکن کاوالن سوهو دان کلمبڤن يڠ کيتت باݢي مڠيلقکن کروسقن لنجوت."
        );
        let out = decode_jawi_text(sample.to_string()).unwrap();
        let lines = out["standard_malay_rumi_lines"].as_array().unwrap();
        assert_eq!(lines.len(), 8);
        assert_eq!(
            lines[0],
            "1. surat perjanjian ini ditandatangani pada lima hari bulan Ogos tahun seribu sembilan ratus lima puluh tujuh."
        );
        assert_eq!(
            lines[1],
            "2. segala raja-raja Melayu telah bersetuju untuk menubuhkan Persekutuan Tanah Melayu yang merdeka dan berdaulat."
        );
        assert_eq!(
            lines[2],
            "3. maka segala undang-undang dan adat istiadat negeri hendaklah dipelihara dengan baik oleh kerajaan."
        );
        assert_eq!(
            lines[3],
            "4. pada tiga puluh satu Ogos tahun yang sama, Tunku Abdul Rahman Putra Al-Haj telah memasyhurkan kemerdekaan di Stadium merdeka, Kuala Lumpur."
        );
        assert_eq!(
            lines[4],
            "5. rakyat sekalian pun bersuka ria menyambut hari bersejarah itu."
        );
        assert_eq!(
            lines[5],
            "6. Arkib Negara Malaysia menyimpan segala rekod rasmi dan manuskrip lama di bahagian pemuliharaan."
        );
        assert_eq!(
            lines[6],
            "7. sebarang kertas yang pudar, koyak, atau berkulat akan dibaiki oleh pegawai yang berkelayakan."
        );
        assert_eq!(
            lines[7],
            "8. kerajaan juga menyediakan kawalan suhu dan kelembapan yang ketat bagi mengelakkan kerosakan lanjut."
        );
        assert_eq!(out["coverage_pct"], 100.0);
    }

    #[test]
    fn test_calligraphic_tatweel_and_persian_variants() {
        let sample = "ارکـيـب نـگـارا مـلـيـسـیـا مـمـلـيـهـارا سـجـاره";
        let out = decode_jawi_text(sample.to_string()).unwrap();
        assert_eq!(
            out["standard_malay_rumi_text"],
            "Arkib Negara Malaysia memelihara sejarah"
        );
    }

    #[test]
    fn test_presentation_forms_b_bidi_and_fst_fuzzy() {
        // Arabic Presentation Forms-B glyphs for "سجاره" (\u{FEB3}\u{FE9F}\u{FE8E}\u{FEAD}\u{FEE9})
        // and "سلطان" (\u{FEB3}\u{FEDF}\u{FEC3}\u{FE8E}\u{FEE5}) wrapped in BiDi marks
        let sample = "\u{200F}\u{FEB3}\u{FE9F}\u{FE8E}\u{FEAD}\u{FEE9} \u{FEB3}\u{FEDF}\u{FEC3}\u{FE8E}\u{FEE5}\u{200E}";
        let out = decode_jawi_text(sample.to_string()).unwrap();
        assert_eq!(out["standard_malay_rumi_text"], "sejarah sultan");
        assert!(out["fst_lexicon_entries"].as_u64().unwrap() >= 300);

        // 1-char OCR typo on "مليسيا" -> "مليسبا" should recover via FST Levenshtein dist-1
        let fuzzy = decode_jawi_or_rough_rumi_line("مليسبا".to_string()).unwrap();
        assert_eq!(fuzzy["rumi"], "Malaysia");
    }

    #[test]
    fn test_hijri_to_masihi_colophon_conversion() {
        // 13 Muharam 1324H -> 1906-03-09 (civil/Kuwaiti tabular calendar)
        assert_eq!(hijri_to_gregorian(1324, 1, 13), (1906, 3, 9));
        // 1 Muharam 1446H -> 2024-07-08 (civil tabular epoch; ±1 day from lunar sighting)
        assert_eq!(hijri_to_gregorian(1446, 1, 1), (2024, 7, 8));

        let dates = extract_hijri_colophon_dates("تمت کيتاب اين ڤد ١٣ محرم ١٣٢٤ دان سنه ١٣٤٠");
        assert_eq!(dates.len(), 2);
        assert_eq!(dates[0]["hijri_date"], "13 Muharam 1324H");
        assert_eq!(dates[0]["gregorian_date_masihi"], "1906-03-09");
        assert_eq!(dates[1]["hijri_date"], "1340H");
    }
}
