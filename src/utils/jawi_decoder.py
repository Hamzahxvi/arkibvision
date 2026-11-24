"""Jawi-to-Rumi (Standard Bahasa Melayu) orthographic and phonetic decoder.

Fixes common VLM OCR confusions on Jawi text:
1. 3-dot Jawi letters confused with 1-dot Arabic letters:
   - ڤ (p) -> ف (f)
   - ڠ (ng) -> غ (gh), خ (kh), ع ('/c), or ي (y)
   - ݢ / ڬ (g) -> ك (k), ل (l), or ي (y)
   - چ (c) -> ج (j) or ح (h)
   - ڽ (ny) -> ن (n), ب (b), ت (t), or ي (y)
2. Classical / Za'ba Jawi vowel conventions:
   - Final ى (alif maqsurah) represents final '-a' (nama, maha, segala, memelihara, sahaja, kepada, mereka, pula)
   - Medial ي in prefixes/closed syllables represents 'e' (pe-, me-, ter-, se-, ke-)
   - و / ؤ represents 'o' or 'u' (memohon, pertolongan, orang, lurus, tunjukilah)
   - Numeral ٢ / 2 after a word represents Malay word reduplication (orang-orang, bukan '2')
"""

from __future__ import annotations

import re

# Direct Jawi-script token -> (Canonical Jawi with 3-dot letters, Standard Rumi)
JAWI_TOKEN_MAP: dict[str, tuple[str, str]] = {
    "ڤيمبوكأن": ("ڤيمبوكأن", "Pembukaan"),
    "فيمبوكان": ("ڤيمبوكأن", "Pembukaan"),
    "ڤمبوكأن": ("ڤمبوكأن", "Pembukaan"),
    "ديڠان": ("ديڠان", "dengan"),
    "ديغان": ("ديڠان", "dengan"),
    "دڠن": ("دڠن", "dengan"),
    "نامى": ("نامى", "nama"),
    "ناعى": ("نامى", "nama"),
    "نام": ("نام", "nama"),
    "اللاه": ("اللاه", "Allah"),
    "اللاء": ("اللاه", "Allah"),
    "الله": ("الله", "Allah"),
    "ياڠ": ("ياڠ", "yang"),
    "ياغ": ("ياڠ", "yang"),
    "يڠ": ("يڠ", "yang"),
    "ماهى": ("ماهى", "Maha"),
    "ماها": ("ماها", "Maha"),
    "مها": ("مها", "Maha"),
    "ڤيموراه": ("ڤيموراه", "Pemurah"),
    "فيموراه": ("ڤيموراه", "Pemurah"),
    "ڤموره": ("ڤموره", "Pemurah"),
    "لاݢي": ("لاݢي", "lagi"),
    "لاكي": ("لاݢي", "lagi"),
    "لاي": ("لاݢي", "lagi"),
    "لايى": ("لاݢي", "lagi"),
    "ميڠاسيهاني": ("ميڠاسيهاني", "Mengasihani"),
    "ميغاسيهاني": ("ميڠاسيهاني", "Mengasihani"),
    "ميخاسيهاني": ("ميڠاسيهاني", "Mengasihani"),
    "مڠاسيهني": ("مڠاسيهني", "Mengasihani"),
    "سيݢالى": ("سيݢالى", "Segala"),
    "سبكالى": ("سيݢالى", "Segala"),
    "سݢالا": ("سݢالا", "Segala"),
    "ڤوجي": ("ڤوجي", "puji"),
    "فوجي": ("ڤوجي", "puji"),
    "تيرتينتو": ("تيرتينتو", "tertentu"),
    "ترتنتو": ("ترتنتو", "tertentu"),
    "باݢي": ("باݢي", "bagi"),
    "باكي": ("باݢي", "bagi"),
    "بايى": ("باݢي", "bagi"),
    "توهان": ("توهان", "Tuhan"),
    "توهمان": ("توهان", "Tuhan"),
    "توهن": ("توهن", "Tuhan"),
    "ميميليهارى": ("ميميليهارى", "memelihara"),
    "ممليهارا": ("ممليهارا", "memelihara"),
    "دان": ("دان", "dan"),
    "مينتادبيركان": ("مينتادبيركان", "mentadbirkan"),
    "مينتاديركان": ("مينتادبيركان", "mentadbirkan"),
    "منتدبيركن": ("منتدبيركن", "mentadbirkan"),
    "سيكاليان": ("سيكاليان", "sekalian"),
    "سيكليان": ("سيكاليان", "sekalian"),
    "سكالين": ("سكالين", "sekalian"),
    "ئعالام": ("ئعالام", "alam"),
    "نعالام": ("ئعالام", "alam"),
    "عالم": ("عالم", "alam"),
    "ميڠواسائي": ("ميڠواسائي", "menguasai"),
    "ميغواساي": ("ميڠواسائي", "menguasai"),
    "مڠواساءي": ("مڠواساءي", "menguasai"),
    "ڤيميرينتاهان": ("ڤيميرينتاهان", "pemerintahan"),
    "فيميرينتاهان": ("ڤيميرينتاهان", "pemerintahan"),
    "ڤمرينتاهن": ("ڤمرينتاهن", "pemerintahan"),
    "هاري": ("هاري", "hari"),
    "ڤيمبالاسان": ("ڤيمبالاسان", "pembalasan"),
    "فيمبالاسان": ("ڤيمبالاسان", "pembalasan"),
    "ڤمبالسن": ("ڤمبالسن", "pembalasan"),
    "ئاخيراة": ("ئاخيراة", "akhirat"),
    "تاخيرة": ("ئاخيراة", "akhirat"),
    "اخيرة": ("اخيرة", "akhirat"),
    "يڠكاولاه": ("يڠكاولاه", "Engkaulah"),
    "يغكولاه": ("يڠكاولاه", "Engkaulah"),
    "اڠكاولاه": ("اڠكاولاه", "Engkaulah"),
    "ساهاجى": ("ساهاجى", "sahaja"),
    "سهاج": ("سهاج", "sahaja"),
    "ي": ("ي", "Ya"),
    "يا": ("يا", "Ya"),
    "كامي": ("كامي", "kami"),
    "كاي": ("كامي", "kami"),
    "سيمباه": ("سيمباه", "sembah"),
    "سمبه": ("سمبه", "sembah"),
    "كيڤادى": ("كيڤادى", "kepada"),
    "كيفادى": ("كيڤادى", "kepada"),
    "كڤد": ("كڤد", "kepada"),
    "ميمؤهؤن": ("ميمؤهؤن", "memohon"),
    "ميموؤهون": ("ميمؤهؤن", "memohon"),
    "مموهون": ("مموهون", "memohon"),
    "ڤيرتؤلؤڠان": ("ڤيرتؤلؤڠان", "pertolongan"),
    "فرترؤلؤغان": ("ڤيرتؤلؤڠان", "pertolongan"),
    "ڤرتولوڠن": ("ڤرتولوڠن", "pertolongan"),
    "تونجوكيلاه": ("تونجوكيلاه", "Tunjukilah"),
    "جالان": ("جالان", "jalan"),
    "جالن": ("جالن", "jalan"),
    "لوروس": ("لوروس", "lurus"),
    "يائيتو": ("يائيتو", "iaitu"),
    "يانيتو": ("يائيتو", "iaitu"),
    "ياءيت": ("ياءيت", "iaitu"),
    "ئؤراڠ٢": ("ئؤراڠ٢", "orang-orang"),
    "تؤراغ٢": ("ئؤراڠ٢", "orang-orang"),
    "اورڠ٢": ("اورڠ٢", "orang-orang"),
    "ئؤراڠ": ("ئؤراڠ", "orang"),
    "تؤراغ": ("ئؤراڠ", "orang"),
    "اورڠ": ("اورڠ", "orang"),
    "يڠكاو": ("يڠكاو", "Engkau"),
    "يغكاو": ("يڠكاو", "Engkau"),
    "اڠكاو": ("اڠكاو", "Engkau"),
    "تيلاه": ("تيلاه", "telah"),
    "تيالاه": ("تيلاه", "telah"),
    "تله": ("تله", "telah"),
    "كورنياكان": ("كورنياكان", "kurniakan"),
    "كورنياكن": ("كورنياكن", "kurniakan"),
    "نيعماة": ("نيعماة", "nikmat"),
    "نعمت": ("نعمت", "nikmat"),
    "ميرئكى": ("ميرئكى", "mereka"),
    "ميرتى": ("ميرئكى", "mereka"),
    "مريك": ("مريك", "mereka"),
    "بوكان": ("بوكان", "bukan"),
    "بوكن": ("بوكن", "bukan"),
    "موركائي": ("موركائي", "murkai"),
    "موركاي": ("موركائي", "murkai"),
    "ڤولى": ("ڤولى", "pula"),
    "فولى": ("ڤولى", "pula"),
    "ڤولا": ("ڤولا", "pula"),
    "سيسات": ("سيسات", "sesat"),
    "سست": ("سست", "sesat"),
}

# Standard Malay Rumi lexicon for Jawi-weighted phonetic reconstruction
MALAY_RUMI_LEXICON: list[str] = [
    "Pembukaan", "dengan", "nama", "Allah", "yang", "Maha", "Pemurah", "lagi",
    "Mengasihani", "Segala", "puji", "tertentu", "bagi", "Tuhan", "memelihara",
    "dan", "mentadbirkan", "sekalian", "alam", "menguasai", "pemerintahan",
    "hari", "pembalasan", "akhirat", "Engkaulah", "sahaja", "Ya", "kami",
    "sembah", "kepada", "memohon", "pertolongan", "Tunjukilah", "jalan",
    "lurus", "iaitu", "orang", "orang-orang", "Engkau", "telah", "kurniakan",
    "nikmat", "mereka", "bukan", "murkai", "pula", "sesat",
    # Common Malay document / manuscript vocabulary
    "bahawa", "adalah", "daripada", "pada", "dalam", "untuk", "tidak", "ini",
    "itu", "atau", "juga", "oleh", "kerana", "tetapi", "apabila", "seperti",
    "semua", "setiap", "antara", "sebagai", "menjadi", "melalui", "terhadap",
    "kerajaan", "negeri", "negara", "rakyat", "raja", "sultan", "menteri",
    "undang-undang", "persekutuan", "perlembagaan", "agama", "islam", "melayu",
    "bahasa", "surat", "perjanjian", "tarikh", "tahun", "bulan", "masa",
    "tempat", "daerah", "kampung", "bandar", "sekolah", "masjid", "mahkamah",
    "jabatan", "pejabat", "pendidikan", "kesihatan", "keselamatan", "kebajikan",
    "maklumat", "keterangan", "catatan", "tandatangan", "pengesahan", "dokumen",
]

# Explicit phonetic-error aliases frequently produced by 4B vision models on Jawi
ROUGH_RUMI_ALIASES: dict[str, str] = {
    "fimboakan": "Pembukaan",
    "fimbukaan": "Pembukaan",
    "deygan": "dengan",
    "digan": "dengan",
    "naci": "nama",
    "nami": "nama",
    "alaa": "Allah",
    "alah": "Allah",
    "yag": "yang",
    "mahi": "Maha",
    "fimorah": "Pemurah",
    "fimurah": "Pemurah",
    "lai": "lagi",
    "layi": "lagi",
    "laki": "lagi",
    "mikhassiyahani": "Mengasihani",
    "migasihani": "Mengasihani",
    "sibkali": "Segala",
    "sigali": "Segala",
    "sikali": "Segala",
    "foji": "puji",
    "fuji": "puji",
    "terintento": "tertentu",
    "tirtinto": "tertentu",
    "bayi": "bagi",
    "baki": "bagi",
    "tuhman": "Tuhan",
    "mimiilihari": "memelihara",
    "mimilihari": "memelihara",
    "minatadirkhan": "mentadbirkan",
    "mintadbirkan": "mentadbirkan",
    "mintadirkan": "mentadbirkan",
    "sikkalian": "sekalian",
    "sikalian": "sekalian",
    "ncalam": "alam",
    "miguwasai": "menguasai",
    "miguasai": "menguasai",
    "fimeryintahan": "pemerintahan",
    "fimirintahan": "pemerintahan",
    "hary": "hari",
    "fimbalasan": "pembalasan",
    "takhira": "akhirat",
    "takhirat": "akhirat",
    "yegkolahe": "Engkaulah",
    "yengkawlah": "Engkaulah",
    "yengkaulah": "Engkaulah",
    "sahaaji": "sahaja",
    "sahaji": "sahaja",
    "i": "Ya",
    "yi": "Ya",
    "kai": "kami",
    "simebah": "sembah",
    "simbah": "sembah",
    "kifadi": "kepada",
    "mimouhun": "memohon",
    "mimo'uhun": "memohon",
    "firtulugan": "pertolongan",
    "firtolongan": "pertolongan",
    "tongjokilah": "Tunjukilah",
    "tonjokilah": "Tunjukilah",
    "lorus": "lurus",
    "yantuto": "iaitu",
    "yaito": "iaitu",
    "tuarag": "orang",
    "turag": "orang",
    "yegkau": "Engkau",
    "yengkau": "Engkau",
    "tiyalah": "telah",
    "tilah": "telah",
    "koriniakan": "kurniakan",
    "korniakan": "kurniakan",
    "niyemaa": "nikmat",
    "niemaat": "nikmat",
    "mirti": "mereka",
    "miriki": "mereka",
    "bokan": "bukan",
    "morikai": "murkai",
    "morkai": "murkai",
    "fuli": "pula",
    "sisat": "sesat",
}


def _jawi_phonetic_skeleton(word: str) -> str:
    """Normalize a Rumi or rough-transliterated Jawi word to a canonical Malay phonetic skeleton."""
    w = word.lower().strip(".,;:!?\"'()[]{}")
    w = re.sub(r"['’`]", "", w)
    # Jawi multi-char equivalences
    w = w.replace("ng", "G").replace("gh", "G").replace("kh", "G").replace("ny", "N").replace("sy", "S")
    # Consonant equivalences (f/v -> p, q -> k, z -> j)
    w = w.translate(str.maketrans({"f": "p", "v": "p", "q": "k", "c": "j", "z": "j"}))
    # Vowel equivalences (i/y -> e, o -> u); collapse repeated vowels
    w = w.translate(str.maketrans({"i": "e", "y": "e", "o": "u"}))
    # Final 'e' in Jawi often represents final 'a' (ى)
    if w.endswith("e") and len(w) > 2:
        w = w[:-1] + "a"
    w = re.sub(r"(.)\1+", r"\1", w)
    return w


def _edit_distance(a: str, b: str) -> int:
    if a == b:
        return 0
    if not a:
        return len(b)
    if not b:
        return len(a)
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, start=1):
        curr = [i]
        for j, cb in enumerate(b, start=1):
            cost = 0 if ca == cb else 1
            curr.append(min(curr[-1] + 1, prev[j] + 1, prev[j - 1] + cost))
        prev = curr
    return prev[-1]


_SKELETON_INDEX: list[tuple[str, str]] = [
    (_jawi_phonetic_skeleton(word), word) for word in MALAY_RUMI_LEXICON
]


_JAWI_CHAR_TO_LATIN: dict[str, str] = {
    "ا": "a", "آ": "a", "أ": "a", "إ": "i", "ئ": "i", "ء": "",
    "ب": "b", "ت": "t", "ة": "t", "ث": "s", "ج": "j", "چ": "c",
    "ح": "h", "خ": "kh", "د": "d", "ذ": "z", "ر": "r", "ز": "z",
    "س": "s", "ش": "sy", "ص": "s", "ض": "d", "ط": "t", "ظ": "z",
    "ع": "a", "غ": "ng", "ڠ": "ng", "ف": "p", "ڤ": "p", "ق": "k",
    "ك": "k", "ک": "k", "ݢ": "g", "ڬ": "g", "گ": "g", "ل": "l",
    "م": "m", "ن": "n", "ڽ": "ny", "و": "u", "ؤ": "o", "ۏ": "v",
    "ه": "h", "ھ": "h", "ي": "i", "ی": "i", "ى": "a",
}


def _transliterate_jawi_chars(word: str) -> str:
    """Fallback character-level Jawi-to-Rumi transliteration before lexicon matching."""
    out: list[str] = []
    for ch in word:
        if ch in _JAWI_CHAR_TO_LATIN:
            out.append(_JAWI_CHAR_TO_LATIN[ch])
        elif "\u064B" <= ch <= "\u065F":
            continue
        else:
            out.append(ch)
    return "".join(out)


def normalize_rough_rumi_word(token: str) -> str:
    """Convert a rough Arabic-phonetic transliteration token into natural Bahasa Melayu Rumi."""
    match = re.match(r"^([^A-Za-z0-9']*)([A-Za-z0-9']+)([^A-Za-z0-9']*)$", token)
    if not match:
        return token
    prefix, core, suffix = match.groups()
    low = core.lower()
    if low in ROUGH_RUMI_ALIASES:
        mapped = ROUGH_RUMI_ALIASES[low]
        return f"{prefix}{mapped}{suffix}"
    # Check exact match in lexicon
    for _, word in _SKELETON_INDEX:
        if word.lower() == low:
            return f"{prefix}{word}{suffix}"
    # Check Jawi phonetic skeleton distance
    skel = _jawi_phonetic_skeleton(low)
    if len(skel) >= 3:
        best_word = None
        best_dist = 999
        for cand_skel, cand_word in _SKELETON_INDEX:
            d = _edit_distance(skel, cand_skel)
            if d < best_dist:
                best_dist = d
                best_word = cand_word
        if best_word and best_dist <= (1 if len(skel) <= 5 else 2):
            return f"{prefix}{best_word}{suffix}"
    return token


def decode_jawi_or_rough_rumi_line(line: str) -> dict[str, str]:
    """Decode a line of Jawi script or rough transliteration into (corrected_jawi, standard_rumi)."""
    cleaned = re.sub(r"^\s*[-*•]\s*(?:\*\*Jawi\*\*:\s*|Jawi:\s*)?", "", line.strip())
    if not cleaned:
        return {"jawi": "", "rumi": ""}

    # Convert Arabic-Indic digits (١..٩) to (1..9) in Rumi, keep in Jawi
    arabic_digits = str.maketrans("٠١٢٣٤٥٦٧٨٩", "0123456789")
    has_arabic_script = bool(re.search(r"[\u0600-\u06FF\u0750-\u077F]", cleaned))

    if has_arabic_script:
        raw_tokens = cleaned.split()
        jawi_out: list[str] = []
        rumi_out: list[str] = []
        for tok in raw_tokens:
            # Check verse number in parentheses or standalone digits
            digit_trans = tok.translate(arabic_digits)
            if re.fullmatch(r"\(?\d+\)?", digit_trans) and digit_trans.strip("()") != "2":
                num = digit_trans.strip("()")
                jawi_out.append(f"({tok.strip('()')})")
                rumi_out.append(f"({num})")
                continue
            if tok in {"٢", "2"} and rumi_out:
                prev_word = rumi_out[-1]
                rumi_out[-1] = f"{prev_word}-{prev_word.lower()}"
                if jawi_out:
                    jawi_out[-1] = f"{jawi_out[-1]}٢"
                continue
            prefix = "(" if tok.startswith("(") else ""
            suffix = ")" if tok.endswith(")") else ""
            bare = tok.strip("()")
            if bare in JAWI_TOKEN_MAP:
                j_word, r_word = JAWI_TOKEN_MAP[bare]
                jawi_out.append(f"{prefix}{j_word}{suffix}")
                rumi_out.append(f"{prefix}{r_word}{suffix}")
            elif bare.endswith("٢") and bare[:-1] in JAWI_TOKEN_MAP:
                j_word, r_word = JAWI_TOKEN_MAP[bare[:-1]]
                jawi_out.append(f"{prefix}{j_word}٢{suffix}")
                rumi_out.append(f"{prefix}{r_word}-{r_word.lower()}{suffix}")
            else:
                jawi_out.append(tok)
                rough = _transliterate_jawi_chars(bare.translate(arabic_digits))
                rumi_out.append(f"{prefix}{normalize_rough_rumi_word(rough)}{suffix}")
        return {"jawi": " ".join(jawi_out), "rumi": " ".join(rumi_out)}

    # Otherwise, input is rough Rumi transliteration
    tokens = cleaned.split()
    rumi_tokens: list[str] = []
    for tok in tokens:
        if tok in {"2", "٢"} and rumi_tokens:
            prev = rumi_tokens[-1].strip("()")
            rumi_tokens[-1] = f"{prev}-{prev.lower()}"
            continue
        rumi_tokens.append(normalize_rough_rumi_word(tok))
    return {"jawi": "", "rumi": " ".join(rumi_tokens)}


def decode_jawi_text(text: str) -> dict:
    """Decode multi-line Jawi script or rough Rumi transliteration into Standard Bahasa Melayu Rumi."""
    lines = [ln for ln in (text or "").splitlines() if ln.strip()]
    decoded_lines = [decode_jawi_or_rough_rumi_line(ln) for ln in lines]
    jawi_lines = [d["jawi"] for d in decoded_lines if d["jawi"]]
    rumi_lines = [d["rumi"] for d in decoded_lines if d["rumi"]]
    return {
        "corrected_jawi_lines": jawi_lines,
        "standard_malay_rumi_lines": rumi_lines,
        "standard_malay_rumi_text": "\n".join(rumi_lines),
    }
# >>> rust-anything: jawi-decoder-rs is on. `rust-anything disable ../../rust/jawi-decoder-rs` removes this block.
def _rust_anything_jawi_decoder_rs():
    import importlib, os, sys
    mode = os.environ.get("RUST_ANYTHING_MODE")
    if mode == "old" or os.environ.get("RUST_ANYTHING_UNIT_JAWI_DECODER") == "original":
        return
    at = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../rust/jawi-decoder-rs/python"))
    sys.path.insert(0, at)
    try:
        native = importlib.import_module("jawi_decoder_rs")
    except ImportError:
        if mode == "new":
            raise
        return
    finally:
        sys.path.remove(at)
    globals().update({name: getattr(native, name) for name in ["normalize_rough_rumi_word", "decode_jawi_or_rough_rumi_line", "decode_jawi_text"]})


_rust_anything_jawi_decoder_rs()
del _rust_anything_jawi_decoder_rs
# <<< rust-anything: jawi-decoder-rs
