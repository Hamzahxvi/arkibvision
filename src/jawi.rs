//! Safe Rust Jawi-to-Rumi orthographic and phonetic skeleton decoder.

#![forbid(unsafe_code)]
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{Value, json};

const JAWI_TOKEN_ENTRIES: &[(&str, (&str, &str))] = &[
    ("ڤيمبوكأن", ("ڤيمبوكأن", "Pembukaan")),
    ("فيمبوكان", ("ڤيمبوكأن", "Pembukaan")),
    ("ڤمبوكأن", ("ڤمبوكأن", "Pembukaan")),
    ("ديڠان", ("ديڠان", "dengan")),
    ("ديغان", ("ديڠان", "dengan")),
    ("دڠن", ("دڠن", "dengan")),
    ("نامى", ("نامى", "nama")),
    ("ناعى", ("نامى", "nama")),
    ("نام", ("نام", "nama")),
    ("اللاه", ("اللاه", "Allah")),
    ("اللاء", ("اللاه", "Allah")),
    ("الله", ("الله", "Allah")),
    ("ياڠ", ("ياڠ", "yang")),
    ("ياغ", ("ياڠ", "yang")),
    ("يڠ", ("يڠ", "yang")),
    ("ماهى", ("ماهى", "Maha")),
    ("ماها", ("ماها", "Maha")),
    ("مها", ("مها", "Maha")),
    ("ڤيموراه", ("ڤيموراه", "Pemurah")),
    ("فيموراه", ("ڤيموراه", "Pemurah")),
    ("ڤموره", ("ڤموره", "Pemurah")),
    ("لاݢي", ("لاݢي", "lagi")),
    ("لاكي", ("لاݢي", "lagi")),
    ("لاي", ("لاݢي", "lagi")),
    ("لايى", ("لاݢي", "lagi")),
    ("ميڠاسيهاني", ("ميڠاسيهاني", "Mengasihani")),
    ("ميغاسيهاني", ("ميڠاسيهاني", "Mengasihani")),
    ("ميخاسيهاني", ("ميڠاسيهاني", "Mengasihani")),
    ("مڠاسيهني", ("مڠاسيهني", "Mengasihani")),
    ("سيݢالى", ("سيݢالى", "Segala")),
    ("سبكالى", ("سيݢالى", "Segala")),
    ("سݢالا", ("سݢالا", "Segala")),
    ("ڤوجي", ("ڤوجي", "puji")),
    ("فوجي", ("ڤوجي", "puji")),
    ("تيرتينتو", ("تيرتينتو", "tertentu")),
    ("ترتنتو", ("ترتنتو", "tertentu")),
    ("باݢي", ("باݢي", "bagi")),
    ("باكي", ("باݢي", "bagi")),
    ("بايى", ("باݢي", "bagi")),
    ("توهان", ("توهان", "Tuhan")),
    ("توهمان", ("توهان", "Tuhan")),
    ("توهن", ("توهن", "Tuhan")),
    ("ميميليهارى", ("ميميليهارى", "memelihara")),
    ("ممليهارا", ("ممليهارا", "memelihara")),
    ("دان", ("دان", "dan")),
    ("مينتادبيركان", ("مينتادبيركان", "mentadbirkan")),
    ("مينتاديركان", ("مينتادبيركان", "mentadbirkan")),
    ("منتدبيركن", ("منتدبيركن", "mentadbirkan")),
    ("سيكاليان", ("سيكاليان", "sekalian")),
    ("سيكليان", ("سيكاليان", "sekalian")),
    ("سكالين", ("سكالين", "sekalian")),
    ("ئعالام", ("ئعالام", "alam")),
    ("نعالام", ("ئعالام", "alam")),
    ("عالم", ("عالم", "alam")),
    ("ميڠواسائي", ("ميڠواسائي", "menguasai")),
    ("ميغواساي", ("ميڠواسائي", "menguasai")),
    ("مڠواساءي", ("مڠواساءي", "menguasai")),
    ("ڤيميرينتاهان", ("ڤيميرينتاهان", "pemerintahan")),
    ("فيميرينتاهان", ("ڤيميرينتاهان", "pemerintahan")),
    ("ڤمرينتاهن", ("ڤمرينتاهن", "pemerintahan")),
    ("هاري", ("هاري", "hari")),
    ("ڤيمبالاسان", ("ڤيمبالاسان", "pembalasan")),
    ("فيمبالاسان", ("ڤيمبالاسان", "pembalasan")),
    ("ڤمبالسن", ("ڤمبالسن", "pembalasan")),
    ("ئاخيراة", ("ئاخيراة", "akhirat")),
    ("تاخيرة", ("ئاخيراة", "akhirat")),
    ("اخيرة", ("اخيرة", "akhirat")),
    ("يڠكاولاه", ("يڠكاولاه", "Engkaulah")),
    ("يغكولاه", ("يڠكاولاه", "Engkaulah")),
    ("اڠكاولاه", ("اڠكاولاه", "Engkaulah")),
    ("ساهاجى", ("ساهاجى", "sahaja")),
    ("سهاج", ("سهاج", "sahaja")),
    ("ي", ("ي", "Ya")),
    ("يا", ("يا", "Ya")),
    ("كامي", ("كامي", "kami")),
    ("كاي", ("كامي", "kami")),
    ("سيمباه", ("سيمباه", "sembah")),
    ("سمبه", ("سمبه", "sembah")),
    ("كيڤادى", ("كيڤادى", "kepada")),
    ("كيفادى", ("كيڤادى", "kepada")),
    ("كڤد", ("كڤد", "kepada")),
    ("ميمؤهؤن", ("ميمؤهؤن", "memohon")),
    ("ميموؤهون", ("ميمؤهؤن", "memohon")),
    ("مموهون", ("مموهون", "memohon")),
    ("ڤيرتؤلؤڠان", ("ڤيرتؤلؤڠان", "pertolongan")),
    ("فرترؤلؤغان", ("ڤيرتؤلؤڠان", "pertolongan")),
    ("ڤرتولوڠن", ("ڤرتولوڠن", "pertolongan")),
    ("تونجوكيلاه", ("تونجوكيلاه", "Tunjukilah")),
    ("جالان", ("جالان", "jalan")),
    ("جالن", ("جالن", "jalan")),
    ("لوروس", ("لوروس", "lurus")),
    ("يائيتو", ("يائيتو", "iaitu")),
    ("يانيتو", ("يائيتو", "iaitu")),
    ("ياءيت", ("ياءيت", "iaitu")),
    ("ئؤراڠ٢", ("ئؤراڠ٢", "orang-orang")),
    ("تؤراغ٢", ("ئؤراڠ٢", "orang-orang")),
    ("اورڠ٢", ("اورڠ٢", "orang-orang")),
    ("ئؤراڠ", ("ئؤراڠ", "orang")),
    ("تؤراغ", ("ئؤراڠ", "orang")),
    ("اورڠ", ("اورڠ", "orang")),
    ("يڠكاو", ("يڠكاو", "Engkau")),
    ("يغكاو", ("يڠكاو", "Engkau")),
    ("اڠكاو", ("اڠكاو", "Engkau")),
    ("تيلاه", ("تيلاه", "telah")),
    ("تيالاه", ("تيلاه", "telah")),
    ("تله", ("تله", "telah")),
    ("كورنياكان", ("كورنياكان", "kurniakan")),
    ("كورنياكن", ("كورنياكن", "kurniakan")),
    ("نيعماة", ("نيعماة", "nikmat")),
    ("نعمت", ("نعمت", "nikmat")),
    ("ميرئكى", ("ميرئكى", "mereka")),
    ("ميرتى", ("ميرئكى", "mereka")),
    ("مريك", ("مريك", "mereka")),
    ("بوكان", ("بوكان", "bukan")),
    ("بوكن", ("بوكن", "bukan")),
    ("موركائي", ("موركائي", "murkai")),
    ("موركاي", ("موركائي", "murkai")),
    ("ڤولى", ("ڤولى", "pula")),
    ("فولى", ("ڤولى", "pula")),
    ("ڤولا", ("ڤولا", "pula")),
    ("سيسات", ("سيسات", "sesat")),
    ("سست", ("سست", "sesat")),
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

fn jawi_token_map() -> &'static HashMap<&'static str, (&'static str, &'static str)> {
    static MAP: OnceLock<HashMap<&'static str, (&'static str, &'static str)>> = OnceLock::new();
    MAP.get_or_init(|| JAWI_TOKEN_ENTRIES.iter().copied().collect())
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
            '٠' => '0',
            '١' => '1',
            '٢' => '2',
            '٣' => '3',
            '٤' => '4',
            '٥' => '5',
            '٦' => '6',
            '٧' => '7',
            '٨' => '8',
            '٩' => '9',
            other => other,
        })
        .collect()
}

fn transliterate_jawi_chars(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    for ch in word.chars() {
        match ch {
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
            'ݢ' | 'ڬ' | 'گ' => out.push('g'),
            'ل' => out.push('l'),
            'م' => out.push('m'),
            'ن' => out.push('n'),
            'ڽ' => out.push_str("ny"),
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

fn decode_line_inner(line: &str) -> (String, String) {
    let stripped = py_trim(line);
    let cleaned = bullet_prefix_re().replace(stripped, "");
    if cleaned.is_empty() {
        return (String::new(), String::new());
    }

    let has_arabic = cleaned
        .chars()
        .any(|c| ('\u{0600}'..='\u{06FF}').contains(&c) || ('\u{0750}'..='\u{077F}').contains(&c));

    if has_arabic {
        let jmap = jawi_token_map();
        let mut jawi_out: Vec<String> = Vec::new();
        let mut rumi_out: Vec<String> = Vec::new();
        for tok in py_split_whitespace(&cleaned) {
            let digit_trans = translate_arabic_digits(tok);
            let stripped_digits = digit_trans.trim_matches(|c| c == '(' || c == ')');
            if digits_fullmatch_re().is_match(&digit_trans) && stripped_digits != "2" {
                let tok_inner = tok.trim_matches(|c| c == '(' || c == ')');
                jawi_out.push(format!("({tok_inner})"));
                rumi_out.push(format!("({stripped_digits})"));
                continue;
            }
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
            let prefix = if tok.starts_with('(') { "(" } else { "" };
            let suffix = if tok.ends_with(')') { ")" } else { "" };
            let bare = tok.trim_matches(|c| c == '(' || c == ')');
            if let Some(&(j_word, r_word)) = jmap.get(bare) {
                jawi_out.push(format!("{prefix}{j_word}{suffix}"));
                rumi_out.push(format!("{prefix}{r_word}{suffix}"));
            } else if let Some(stem) = bare.strip_suffix('٢')
                && let Some(&(j_word, r_word)) = jmap.get(stem)
            {
                jawi_out.push(format!("{prefix}{j_word}٢{suffix}"));
                let lower = r_word.to_lowercase();
                rumi_out.push(format!("{prefix}{r_word}-{lower}{suffix}"));
            } else {
                jawi_out.push(tok.to_string());
                let rough = transliterate_jawi_chars(&translate_arabic_digits(bare));
                let norm = normalize_word_str(&rough);
                rumi_out.push(format!("{prefix}{norm}{suffix}"));
            }
        }
        return (jawi_out.join(" "), rumi_out.join(" "));
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
    (String::new(), rumi_tokens.join(" "))
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
    for ln in py_splitlines(&text) {
        if py_trim(ln).is_empty() {
            continue;
        }
        let (jawi, rumi) = decode_line_inner(ln);
        if !jawi.is_empty() {
            jawi_lines.push(jawi);
        }
        if !rumi.is_empty() {
            rumi_lines.push(rumi);
        }
    }
    let rumi_text = rumi_lines.join("\n");
    Ok(json!({
        "corrected_jawi_lines": jawi_lines,
        "standard_malay_rumi_lines": rumi_lines,
        "standard_malay_rumi_text": rumi_text,
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
}
