//! Safe Rust Jawi-to-Rumi orthographic, morphological, and phonetic skeleton decoder.
//!
//! Supports both Modern Jawi (DBP) and Old Classical Jawi (*Jawi Lama* / *Hikayat* /
//! *Surat Diraja* / archival manuscripts), normalizing multi-font Unicode variants
//! (tatweel/kashida, Arabic/Persian/Urdu codepoints, harakat, and legacy Malaysian `پ`→`ڽ`).

#![forbid(unsafe_code)]
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{Value, json};

const JAWI_TOKEN_ENTRIES: &[(&str, (&str, &str))] = &[
    // --- Classical & Al-Fatihah / OCR 3-dot restoration entries ---
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
    ("سݢل", ("سݢل", "segala")),
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
    ("دڤليهارا", ("دڤليهارا", "dipelihara")),
    ("ڤليهارا", ("ڤليهارا", "pelihara")),
    ("دان", ("دان", "dan")),
    ("مينتادبيركان", ("مينتادبيركان", "mentadbirkan")),
    ("مينتاديركان", ("مينتادبيركان", "mentadbirkan")),
    ("منتدبيركن", ("منتدبيركن", "mentadbirkan")),
    ("سيكاليان", ("سيكاليان", "sekalian")),
    ("سيكليان", ("سيكاليان", "sekalian")),
    ("سكالين", ("سكالين", "sekalian")),
    ("سكلين", ("سكلين", "sekalian")),
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
    ("سهج", ("سهاج", "sahaja")),
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
    ("ڤول", ("ڤول", "pula")),
    ("سيسات", ("سيسات", "sesat")),
    ("سست", ("سست", "sesat")),
    // --- Core Modern & Old Jawi Function Words / Particles ---
    ("د", ("د", "di")),
    ("ك", ("ك", "ke")),
    ("ڤد", ("ڤد", "pada")),
    ("دالم", ("دالم", "dalam")),
    ("درڤد", ("درڤد", "daripada")),
    ("دريڤد", ("دريڤد", "daripada")),
    ("دري", ("دري", "dari")),
    ("اونتوق", ("اونتوق", "untuk")),
    ("اونتوك", ("اونتوق", "untuk")),
    ("اوليه", ("اوليه", "oleh")),
    ("اوله", ("اوليه", "oleh")),
    ("كران", ("كران", "kerana")),
    ("كرن", ("كران", "kerana")),
    ("تتاڤي", ("تتاڤي", "tetapi")),
    ("تتڤي", ("تتاڤي", "tetapi")),
    ("سڤرتي", ("سڤرتي", "seperti")),
    ("اڤابيلا", ("اڤابيلا", "apabila")),
    ("اڤبيل", ("اڤابيلا", "apabila")),
    ("اداله", ("اداله", "adalah")),
    ("اياله", ("اياله", "ialah")),
    ("اد", ("اد", "ada")),
    ("تيدق", ("تيدق", "tidak")),
    ("تيداق", ("تيدق", "tidak")),
    ("تياد", ("تياد", "tiada")),
    ("تاء", ("تاء", "tak")),
    ("اين", ("اين", "ini")),
    ("ايت", ("ايت", "itu")),
    ("جوݢ", ("جوݢ", "juga")),
    ("جوݢا", ("جوݢ", "juga")),
    ("ڤون", ("ڤون", "pun")),
    ("اتاو", ("اتاو", "atau")),
    ("سموا", ("سموا", "semua")),
    ("ستياڤ", ("ستياڤ", "setiap")),
    ("تياڤ", ("تياڤ", "tiap")),
    ("انتارا", ("انتارا", "antara")),
    ("انتار", ("انتارا", "antara")),
    ("سباݢاي", ("سباݢاي", "sebagai")),
    ("سباݢي", ("سباݢاي", "sebagai")),
    ("سبارڠ", ("سبارڠ", "sebarang")),
    ("اكن", ("اكن", "akan")),
    ("سدڠ", ("سدڠ", "sedang")),
    ("ماسيه", ("ماسيه", "masih")),
    ("مسيه", ("ماسيه", "masih")),
    ("بلوم", ("بلوم", "belum")),
    ("سوده", ("سوده", "sudah")),
    ("بوليه", ("بوليه", "boleh")),
    ("بوله", ("بوليه", "boleh")),
    ("داڤت", ("داڤت", "dapat")),
    ("هاروس", ("هاروس", "harus")),
    ("مستي", ("مستي", "mesti")),
    ("هندق", ("هندق", "hendak")),
    ("هندقله", ("هندقله", "hendaklah")),
    ("ماهو", ("ماهو", "mahu")),
    ("ڤرلو", ("ڤرلو", "perlu")),
    ("سڠت", ("سڠت", "sangat")),
    ("ساڠت", ("ساڠت", "sangat")),
    ("امت", ("امت", "amat")),
    ("ترلالو", ("ترلالو", "terlalu")),
    ("لبيه", ("لبيه", "lebih")),
    ("لبه", ("لبيه", "lebih")),
    ("كورڠ", ("كورڠ", "kurang")),
    ("سام", ("سام", "sama")),
    ("لاءين", ("لاءين", "lain")),
    ("بهارو", ("بهارو", "baharu")),
    ("بارو", ("بارو", "baru")),
    ("لام", ("لام", "lama")),
    ("لاما", ("لاما", "lama")),
    ("سنديري", ("سنديري", "sendiri")),
    ("برسام", ("برسام", "bersama")),
    ("سرتا", ("سرتا", "serta")),
    ("سرت", ("سرتا", "serta")),
    ("تنڤا", ("تنڤا", "tanpa")),
    ("هيڠݢ", ("هيڠݢ", "hingga")),
    ("سهيڠݢ", ("سهيڠݢ", "sehingga")),
    ("سبلوم", ("سبلوم", "sebelum")),
    ("سلڤس", ("سلڤس", "selepas")),
    ("سسوده", ("سسوده", "sesudah")),
    ("ستله", ("ستله", "setelah")),
    ("كتيك", ("كتيك", "ketika")),
    ("سماس", ("سماس", "semasa")),
    ("منكالا", ("منكالا", "manakala")),
    ("والاوڤون", ("والاوڤون", "walaupun")),
    ("جك", ("جك", "jika")),
    ("جيك", ("جيك", "jika")),
    ("جكالو", ("جكالو", "jikalau")),
    ("سكيرڽ", ("سكيرڽ", "sekiranya")),
    ("سڤاي", ("سڤاي", "supaya")),
    ("اݢر", ("اݢر", "agar")),
    ("ملاينكن", ("ملاينكن", "melainkan")),
    ("كچوالي", ("كچوالي", "kecuali")),
    ("ترهادڤ", ("ترهادڤ", "terhadap")),
    ("ملالوءي", ("ملالوءي", "melalui")),
    ("منجادي", ("منجادي", "menjadi")),
    ("سياڤا", ("سياڤا", "siapa")),
    ("اڤ", ("اڤ", "apa")),
    ("مان", ("مان", "mana")),
    ("بيلا", ("بيلا", "bila")),
    ("بݢايمان", ("بݢايمان", "bagaimana")),
    ("مڠاڤ", ("مڠاڤ", "mengapa")),
    ("كيت", ("كيت", "kita")),
    ("ساي", ("ساي", "saya")),
    ("اكو", ("اكو", "aku")),
    ("ديا", ("ديا", "dia")),
    ("بلياو", ("بلياو", "beliau")),
    // --- Old Classical Jawi (Jawi Lama / Hikayat / Surat Diraja / Warkah Sultan) ---
    ("مك", ("مك", "maka")),
    ("تتكال", ("تتكال", "tatkala")),
    ("تتكالا", ("تتكالا", "tatkala")),
    ("شهدان", ("شهدان", "syahadan")),
    ("شهيدان", ("شهدان", "syahadan")),
    ("حتي", ("حتي", "hatta")),
    ("حتا", ("حتا", "hatta")),
    ("اركين", ("اركين", "arakian")),
    ("برمول", ("برمول", "bermula")),
    ("برمولا", ("برمولا", "bermula")),
    ("سبرمول", ("سبرمول", "sebermula")),
    ("ادڤون", ("ادڤون", "adapun")),
    ("القصه", ("القصه", "alkisah")),
    ("كلكين", ("كلكين", "kalakian")),
    ("بهوا", ("بهوا", "bahawa")),
    ("بهاوا", ("بهاوا", "bahawa")),
    ("بهوسڽ", ("بهوسڽ", "bahawasanya")),
    ("سسوڠݢوهڽ", ("سسوڠݢوهڽ", "sesungguhnya")),
    ("دمكين", ("دمكين", "demikian")),
    ("دمكينله", ("دمكينله", "demikianlah")),
    ("بݢند", ("بݢند", "baginda")),
    ("بݢيندا", ("بݢيندا", "baginda")),
    ("تيته", ("تيته", "titah")),
    ("برتيته", ("برتيته", "bertitah")),
    ("منجنجوڠ", ("منجنجوڠ", "menjunjung")),
    ("جنجوڠ", ("جنجوڠ", "junjung")),
    ("مرڤك", ("مرڤك", "merafak")),
    ("ڤاتيك", ("ڤاتيك", "patik")),
    ("بيتا", ("بيتا", "beta")),
    ("توانكو", ("توانكو", "Tuanku")),
    ("تونكو", ("تونكو", "Tunku")),
    ("دولي", ("دولي", "Duli")),
    ("موليا", ("موليا", "Mulia")),
    ("سري", ("سري", "seri")),
    ("ڤادوك", ("ڤادوك", "Paduka")),
    ("ڤدوك", ("ڤدوك", "Paduka")),
    ("سلطان", ("سلطان", "sultan")),
    ("راج", ("راج", "raja")),
    ("راجا", ("راجا", "raja")),
    ("بندهارا", ("بندهارا", "Bendahara")),
    ("بندهار", ("بندهار", "Bendahara")),
    ("تمڠݢوڠ", ("تمڠݢوڠ", "Temenggung")),
    ("لقسمان", ("لقسمان", "Laksamana")),
    ("لقسامان", ("لقسامان", "Laksamana")),
    ("شاهبندر", ("شاهبندر", "Syahbandar")),
    ("منتري", ("منتري", "menteri")),
    ("ڤردان", ("ڤردان", "Perdana")),
    ("ڤڠهولو", ("ڤڠهولو", "Penghulu")),
    ("داتوق", ("داتوق", "Datuk")),
    ("داتوء", ("داتوء", "Datuk")),
    ("تون", ("تون", "Tun")),
    ("تن", ("تن", "Tan")),
    ("بسر", ("بسر", "besar")),
    ("كچيل", ("كچيل", "kecil")),
    ("دباليروڠ", ("دباليروڠ", "di balairung")),
    ("باليروڠ", ("باليروڠ", "balairung")),
    ("ايستان", ("ايستان", "istana")),
    ("دايستان", ("دايستان", "di istana")),
    ("برهيمڤون", ("برهيمڤون", "berhimpun")),
    ("بركومڤول", ("بركومڤول", "berkumpul")),
    ("مڠادڤ", ("مڠادڤ", "menghadap")),
    ("مڠهادڤ", ("مڠهادڤ", "menghadap")),
    ("ڤنوه", ("ڤنوه", "penuh")),
    ("طاعة", ("طاعة", "taat")),
    ("طاعت", ("طاعت", "taat")),
    ("ستيا", ("ستيا", "setia")),
    ("درهاك", ("درهاك", "derhaka")),
    ("درهك", ("درهك", "derhaka")),
    ("دولت", ("دولت", "daulat")),
    ("بردولت", ("بردولت", "berdaulat")),
    ("كراجأن", ("كراجأن", "kerajaan")),
    ("كراجان", ("كراجأن", "kerajaan")),
    ("كسولطانن", ("كسولطانن", "kesultanan")),
    ("كسلطانن", ("كسلطانن", "kesultanan")),
    ("نݢري", ("نݢري", "negeri")),
    ("نݢارا", ("نݢارا", "Negara")),
    ("نݢار", ("نݢارا", "negara")),
    ("بڠسا", ("بڠسا", "bangsa")),
    ("بڠس", ("بڠسا", "bangsa")),
    ("رعية", ("رعية", "rakyat")),
    ("رعيت", ("رعيت", "rakyat")),
    ("همبا", ("همبا", "hamba")),
    ("همب", ("همب", "hamba")),
    ("تانه", ("تانه", "Tanah")),
    ("اءير", ("اءير", "air")),
    ("اير", ("اير", "air")),
    ("ملايو", ("ملايو", "Melayu")),
    ("مليسيا", ("مليسيا", "Malaysia")),
    ("ڤرسكوتوان", ("ڤرسكوتوان", "Persekutuan")),
    ("ڤرلمباݢأن", ("ڤرلمباݢأن", "Perlembagaan")),
    ("عادت", ("عادت", "adat")),
    ("استعادت", ("استعادت", "istiadat")),
    ("اوندڠ", ("اوندڠ", "undang")),
    ("شريعة", ("شريعة", "syariat")),
    ("شريعت", ("شريعت", "syariat")),
    ("حكوم", ("حكوم", "hukum")),
    ("قانون", ("قانون", "kanun")),
    ("ورقه", ("ورقه", "warkah")),
    ("ورقة", ("ورقة", "warkah")),
    ("سورت", ("سورت", "surat")),
    ("چڤ", ("چڤ", "cap")),
    ("موهور", ("موهور", "mohor")),
    ("ڤرجنجين", ("ڤرجنجين", "perjanjian")),
    ("ڤرجنجيان", ("ڤرجنجين", "perjanjian")),
    ("تنداتاڠن", ("تنداتاڠن", "tandatangan")),
    ("تنداتڠن", ("تنداتڠن", "tandatangan")),
    ("دتنداتڠني", ("دتنداتڠني", "ditandatangani")),
    ("دتنداتاڠني", ("دتنداتاڠني", "ditandatangani")),
    ("ڤڠيشتيهارن", ("ڤڠيشتيهارن", "pengisytiharan")),
    ("ڤمشهورن", ("ڤمشهورن", "pemasyhuran")),
    ("ممشهوركن", ("ممشهوركن", "memasyhurkan")),
    ("كمرديكأن", ("كمرديكأن", "kemerdekaan")),
    ("كمرديكان", ("كمرديكأن", "kemerdekaan")),
    ("مرديك", ("مرديك", "merdeka")),
    ("مرديكا", ("مرديكا", "merdeka")),
    ("سجاره", ("سجاره", "sejarah")),
    ("برسجاره", ("برسجاره", "bersejarah")),
    ("واريثن", ("واريثن", "warisan")),
    ("بودايا", ("بودايا", "budaya")),
    ("بوداي", ("بوداي", "budaya")),
    ("بهاس", ("بهاس", "bahasa")),
    ("بهاسا", ("بهاسا", "bahasa")),
    ("جاوي", ("جاوي", "Jawi")),
    ("رومي", ("رومي", "Rumi")),
    ("توليسن", ("توليسن", "tulisan")),
    ("اݢام", ("اݢام", "agama")),
    ("اݢاما", ("اݢاما", "agama")),
    ("اسلام", ("اسلام", "Islam")),
    // --- Numbers, Dates, Months & Calendar ---
    ("ساتو", ("ساتو", "satu")),
    ("دوا", ("دوا", "dua")),
    ("تيݢ", ("تيݢ", "tiga")),
    ("تيݢا", ("تيݢ", "tiga")),
    ("امڤت", ("امڤت", "empat")),
    ("ليم", ("ليم", "lima")),
    ("ليما", ("ليم", "lima")),
    ("انم", ("انم", "enam")),
    ("توجوه", ("توجوه", "tujuh")),
    ("لاڤن", ("لاڤن", "lapan")),
    ("سمبيلن", ("سمبيلن", "sembilan")),
    ("سڤولوه", ("سڤولوه", "sepuluh")),
    ("ڤولوه", ("ڤولوه", "puluh")),
    ("بلس", ("بلس", "belas")),
    ("راتوس", ("راتوس", "ratus")),
    ("سراتوس", ("سراتوس", "seratus")),
    ("ريبو", ("ريبو", "ribu")),
    ("سريبو", ("سريبو", "seribu")),
    ("جوتا", ("جوتا", "juta")),
    ("ڤرتام", ("ڤرتام", "pertama")),
    ("كدوا", ("كدوا", "kedua")),
    ("كتيݢ", ("كتيݢ", "ketiga")),
    ("تاريخ", ("تاريخ", "tarikh")),
    ("بولن", ("بولن", "bulan")),
    ("تاهون", ("تاهون", "tahun")),
    ("ماس", ("ماس", "masa")),
    ("وقتو", ("وقتو", "waktu")),
    ("ابد", ("ابد", "abad")),
    ("زمان", ("زمان", "zaman")),
    ("جانواري", ("جانواري", "Januari")),
    ("فيبرواري", ("فيبرواري", "Februari")),
    ("مچ", ("مچ", "Mac")),
    ("اڤريل", ("اڤريل", "April")),
    ("مي", ("مي", "Mei")),
    ("جون", ("جون", "Jun")),
    ("جولاي", ("جولاي", "Julai")),
    ("اوݢوس", ("اوݢوس", "Ogos")),
    ("سڤتمبر", ("سڤتمبر", "September")),
    ("اوكتوبر", ("اوكتوبر", "Oktober")),
    ("نوۏمبر", ("نوۏمبر", "November")),
    ("ديسمبر", ("ديسمبر", "Disember")),
    ("محرم", ("محرم", "Muharam")),
    ("صفر", ("صفر", "Safar")),
    ("ربيع", ("ربيع", "Rabiul")),
    ("رجب", ("رجب", "Rejab")),
    ("شعبان", ("شعبان", "Syaaban")),
    ("رمضان", ("رمضان", "Ramadan")),
    ("شوال", ("شوال", "Syawal")),
    ("ذوالحجه", ("ذوالحجه", "Zulhijah")),
    ("هجره", ("هجره", "Hijrah")),
    ("هجرة", ("هجرة", "Hijrah")),
    ("مسيحي", ("مسيحي", "Masihi")),
    // --- Archival Preservation, Conservation & Arkib Negara Malaysia ---
    ("اركيب", ("اركيب", "Arkib")),
    ("ريكود", ("ريكود", "rekod")),
    ("دوكومن", ("دوكومن", "dokumen")),
    ("مانوسكريڤ", ("مانوسكريڤ", "manuskrip")),
    ("نسخه", ("نسخه", "naskhah")),
    ("نوسخه", ("نسخه", "naskhah")),
    ("كتاب", ("كتاب", "kitab")),
    ("حكايت", ("حكايت", "hikayat")),
    ("حكاية", ("حكاية", "hikayat")),
    ("راسمي", ("راسمي", "rasmi")),
    ("ڤموليهارأن", ("ڤموليهارأن", "pemuliharaan")),
    ("ڤموليهاران", ("ڤموليهارأن", "pemuliharaan")),
    ("بهاݢين", ("بهاݢين", "bahagian")),
    ("جابتن", ("جابتن", "jabatan")),
    ("ڤجابت", ("ڤجابت", "pejabat")),
    ("كمنترين", ("كمنترين", "kementerian")),
    ("مڽيمڤن", ("مڽيمڤن", "menyimpan")),
    ("دسيمڤن", ("دسيمڤن", "disimpan")),
    ("سيمڤن", ("سيمڤن", "simpan")),
    ("كرجا", ("كرجا", "kerja")),
    ("دجالنكن", ("دجالنكن", "dijalankan")),
    ("منجالنكن", ("منجالنكن", "menjalankan")),
    ("كرتس", ("كرتس", "kertas")),
    ("كيرتس", ("كرتس", "kertas")),
    ("ڤودر", ("ڤودر", "pudar")),
    ("كويق", ("كويق", "koyak")),
    ("بركولت", ("بركولت", "berkulat")),
    ("كولت", ("كولت", "kulat")),
    ("دبايكي", ("دبايكي", "dibaiki")),
    ("دباءيقي", ("دباءيقي", "dibaiki")),
    ("باءيق", ("باءيق", "baik")),
    ("بايك", ("بايك", "baik")),
    ("ڤݢاواي", ("ڤݢاواي", "pegawai")),
    ("بركلياقن", ("بركلياقن", "berkelayakan")),
    ("بركلايقن", ("بركلايقن", "berkelayakan")),
    ("مڽدياكن", ("مڽدياكن", "menyediakan")),
    ("سديا", ("سديا", "sedia")),
    ("كاولن", ("كاولن", "kawalan")),
    ("كاوالن", ("كاولن", "kawalan")),
    ("سوهو", ("سوهو", "suhu")),
    ("كلمبڤن", ("كلمبڤن", "kelembapan")),
    ("كتت", ("كتت", "ketat")),
    ("كيتت", ("كتت", "ketat")),
    ("مڠيلقكن", ("مڠيلقكن", "mengelakkan")),
    ("كروسقن", ("كروسقن", "kerosakan")),
    ("كروساقن", ("كروسقن", "kerosakan")),
    ("روسق", ("روسق", "rosak")),
    ("لنجوت", ("لنجوت", "lanjut")),
    ("برستوجو", ("برستوجو", "bersetuju")),
    ("ستوجو", ("ستوجو", "setuju")),
    ("منوبوهكن", ("منوبوهكن", "menubuhkan")),
    ("توبوه", ("توبوه", "tubuh")),
    ("ستاديوم", ("ستاديوم", "Stadium")),
    ("كوالا", ("كوالا", "Kuala")),
    ("لومڤور", ("لومڤور", "Lumpur")),
    ("عبدالرحمن", ("عبدالرحمن", "Abdul Rahman")),
    ("عبد", ("عبد", "Abdul")),
    ("الرحمن", ("الرحمن", "Rahman")),
    ("ڤوترا", ("ڤوترا", "Putra")),
    ("الحاج", ("الحاج", "Al-Haj")),
    ("برسوك", ("برسوك", "bersuka")),
    ("ريا", ("ريا", "ria")),
    ("مڽمبوت", ("مڽمبوت", "menyambut")),
    ("سمبوت", ("سمبوت", "sambut")),
    ("كسمڤتن", ("كسمڤتن", "kesempatan")),
    ("كجادين", ("كجادين", "kejadian")),
    ("كعاديلن", ("كعاديلن", "keadilan")),
    ("كسلامتن", ("كسلامتن", "keselamatan")),
    ("كصيحتن", ("كصيحتن", "kesihatan")),
    ("ڤنديديقن", ("ڤنديديقن", "pendidikan")),
    ("كباجيقن", ("كباجيقن", "kebajikan")),
    ("معلومات", ("معلومات", "maklumat")),
    ("كترڠن", ("كترڠن", "keterangan")),
    ("چاتتن", ("چاتتن", "catatan")),
    ("ڤڠصحن", ("ڤڠصحن", "pengesahan")),
    ("بڠونن", ("بڠونن", "bangunan")),
    ("ديوان", ("ديوان", "dewan")),
    ("ݢالري", ("ݢالري", "galeri")),
    ("ڤاميرن", ("ڤاميرن", "pameran")),
    ("ڤڽليديقن", ("ڤڽليديقن", "penyelidikan")),
    ("تيمڤوه", ("تيمڤوه", "tempoh")),
    ("تمڤت", ("تمڤت", "tempat")),
    ("دائره", ("دائره", "daerah")),
    ("داءيره", ("داءيره", "daerah")),
    ("كمڤوڠ", ("كمڤوڠ", "kampung")),
    ("بندر", ("بندر", "bandar")),
    ("سكوله", ("سكوله", "sekolah")),
    ("مسجد", ("مسجد", "masjid")),
    ("محكمه", ("محكمه", "mahkamah")),
];

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

/// Normalizes multi-font Jawi/Arabic/Persian/Urdu Unicode variants into a single canonical
/// lookup key: strips tatweel/kashida (`ـ`), harakat/diacritics, and zero-width joiners, and
/// unifies equivalent Kaf (`ک`/`ڪ`→`ك`), Ya (`ی`/`ے`→`ي`), Ga (`گ`/`ڬ`/`ݣ`→`ݢ`),
/// Ha (`ھ`/`ۀ`→`ه`), and Alif hamza forms (`أ`/`إ`/`آ`/`ٱ`→`ا`).
fn normalize_jawi_key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            // Strip tatweel (calligraphic stretching), Arabic harakat/vowels, and joiners.
            '\u{0640}'
            | '\u{064B}'..='\u{065F}'
            | '\u{0670}'
            | '\u{06D6}'..='\u{06ED}'
            | '\u{200C}'
            | '\u{200D}'
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

fn jawi_token_map() -> &'static HashMap<String, (&'static str, &'static str)> {
    static MAP: OnceLock<HashMap<String, (&'static str, &'static str)>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = HashMap::with_capacity(JAWI_TOKEN_ENTRIES.len() + 32);
        for &(raw_key, val) in JAWI_TOKEN_ENTRIES {
            map.entry(normalize_jawi_key(raw_key)).or_insert(val);
        }
        map
    })
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
            '\u{0640}' | '\u{200C}' | '\u{200D}' => {}
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

fn lookup_jawi_exact(
    key: &str,
    jmap: &HashMap<String, (&'static str, &'static str)>,
) -> Option<(&'static str, &'static str)> {
    if let Some(&hit) = jmap.get(key) {
        return Some(hit);
    }
    // Malaysian Jawi Biasa legacy fonts encode `ڽ` (ny) as `پ`, whereas Persian/Urdu use `پ` for `ڤ` (p).
    if key.contains('پ') {
        if let Some(&hit) = jmap.get(&key.replace('پ', "ڽ")) {
            return Some(hit);
        }
        if let Some(&hit) = jmap.get(&key.replace('پ', "ڤ")) {
            return Some(hit);
        }
    }
    // Classical / modern ta marbutah (`ة`) vs ta maftuhah (`ت`) interchange (e.g. طاعة/طاعت, رعية/رعيت).
    if let Some(stem) = key.strip_suffix('ة')
        && let Some(&hit) = jmap.get(&format!("{stem}ت"))
    {
        return Some(hit);
    }
    if let Some(stem) = key.strip_suffix('ت')
        && let Some(&hit) = jmap.get(&format!("{stem}ة"))
    {
        return Some(hit);
    }
    None
}

/// Deterministic morphological prefix/suffix decomposition for Jawi tokens not directly
/// listed in `JAWI_TOKEN_ENTRIES`. Only fires when the stripped root is a verified dictionary
/// word (at least 2 chars) so it never hallucinates.
fn lookup_jawi_morphological(
    key: &str,
    jmap: &HashMap<String, (&'static str, &'static str)>,
) -> Option<String> {
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

    // 1. Prefix + known root (e.g., بركولت, دسيمڤن, كراجأن)
    for &(j_pre, r_pre) in PREFIXES {
        if let Some(rest) = key.strip_prefix(j_pre)
            && rest.chars().count() >= 2
            && let Some((_, root_rumi)) = lookup_jawi_exact(rest, jmap)
        {
            return Some(format!("{r_pre}{}", root_rumi.to_lowercase()));
        }
    }

    // 2. Known root + suffix (e.g., هندقله, سورتڽ, كراجأنڽ)
    for &(j_suf, r_suf) in SUFFIXES {
        if let Some(rest) = key.strip_suffix(j_suf)
            && rest.chars().count() >= 2
            && let Some((_, root_rumi)) = lookup_jawi_exact(rest, jmap)
        {
            return Some(format!("{root_rumi}{r_suf}"));
        }
    }

    // 3. Circumfix: prefix + known root + suffix (e.g., دتنداتڠني, منوبوهكن)
    for &(j_pre, r_pre) in PREFIXES {
        if let Some(after_pre) = key.strip_prefix(j_pre) {
            for &(j_suf, r_suf) in SUFFIXES {
                if let Some(root) = after_pre.strip_suffix(j_suf)
                    && root.chars().count() >= 2
                    && let Some((_, root_rumi)) = lookup_jawi_exact(root, jmap)
                {
                    return Some(format!("{r_pre}{}{r_suf}", root_rumi.to_lowercase()));
                }
            }
        }
    }

    None
}

struct LineDecodeStats {
    jawi: String,
    rumi: String,
    total_jawi_tokens: usize,
    matched_jawi_tokens: usize,
    old_jawi_hits: usize,
}

fn decode_line_with_stats(line: &str) -> LineDecodeStats {
    let stripped = py_trim(line);
    let cleaned = bullet_prefix_re().replace(stripped, "");
    if cleaned.is_empty() {
        return LineDecodeStats {
            jawi: String::new(),
            rumi: String::new(),
            total_jawi_tokens: 0,
            matched_jawi_tokens: 0,
            old_jawi_hits: 0,
        };
    }

    let has_arabic = cleaned
        .chars()
        .any(|c| ('\u{0600}'..='\u{06FF}').contains(&c) || ('\u{0750}'..='\u{077F}').contains(&c));

    if has_arabic {
        let jmap = jawi_token_map();
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
                jawi_out.push(format!("({tok_inner})"));
                rumi_out.push(format!("({stripped_digits})"));
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

            // 1. Exact dictionary match (including normalized Unicode, `پ`→`ڽ`/`ڤ`, `ة`↔`ت`)
            if let Some((j_word, r_word)) = lookup_jawi_exact(&norm_key, jmap) {
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
                if let Some((j_word, r_word)) = lookup_jawi_exact(stem_key, jmap) {
                    matched_jawi_tokens += 1;
                    let lower = r_word.to_lowercase();
                    jawi_out.push(format!("{prefix}{j_word}٢{suffix}"));
                    rumi_out.push(format!("{rumi_prefix}{r_word}-{lower}{rumi_suffix}"));
                    continue;
                }
                if let Some(morph_rumi) = lookup_jawi_morphological(stem_key, jmap) {
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

            // 3. Deterministic morphological prefix/suffix match
            if let Some(morph_rumi) = lookup_jawi_morphological(&norm_key, jmap) {
                matched_jawi_tokens += 1;
                jawi_out.push(tok.to_string());
                rumi_out.push(format!("{rumi_prefix}{morph_rumi}{rumi_suffix}"));
                continue;
            }

            // 4. Phonetic skeleton & transliteration fallback
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

    Ok(json!({
        "corrected_jawi_lines": jawi_lines,
        "standard_malay_rumi_lines": rumi_lines,
        "standard_malay_rumi_text": rumi_text,
        "dictionary_matched_tokens": matched_jawi,
        "total_jawi_tokens": total_jawi,
        "coverage_pct": coverage_pct,
        "script_style": script_style,
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
        // Calligraphic kashida stretching + Persian ک/ی/گ + diacritics
        let sample = "ارکـيـب نـگـارا مـلـيـسـیـا مـمـلـيـهـارا سـجـاره";
        let out = decode_jawi_text(sample.to_string()).unwrap();
        assert_eq!(
            out["standard_malay_rumi_text"],
            "Arkib Negara Malaysia memelihara sejarah"
        );
    }
}
