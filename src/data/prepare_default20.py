from pathlib import Path
import sys
from urllib.parse import quote
from urllib.request import Request, urlopen
from urllib.error import HTTPError, URLError
import json

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from src.utils.paths import RAW_DIR

ROOT = RAW_DIR / "staff_enroll"
ROOT.mkdir(parents=True, exist_ok=True)

PEOPLE = [
    ("Anwar Ibrahim", "Anwar Ibrahim Malaysia"),
    ("Ahmad Zahid Hamidi", "Ahmad Zahid Hamidi Malaysia"),
    ("Fadillah Yusof", "Fadillah Yusof Malaysia"),
    ("Loke Siew Fook", "Loke Siew Fook Malaysia"),
    ("Mohamad Sabu", "Mohamad Sabu Malaysia"),
    ("Nga Kor Ming", "Nga Kor Ming Malaysia"),
    ("Mohamad Hasan", "Mohamad Hasan (politician)"),
    ("Alexander Nanta Linggi", "Alexander Nanta Linggi Malaysia"),
    ("Saifuddin Nasution Ismail", "Saifuddin Nasution Ismail Malaysia"),
    ("Mohamed Khaled Nordin", "Mohamed Khaled Nordin Malaysia"),
    ("Chang Lih Kang", "Chang Lih Kang Malaysia"),
    ("Azalina Othman Said", "Azalina Othman Said Malaysia"),
    ("Sultan Ibrahim", "Sultan Ibrahim Ismail"),
    ("Sultan Sallehuddin", "Sultan Sallehuddin"),
    ("Sultan Nazrin Muizzuddin Shah", "Sultan Nazrin Shah"),
    ("Sultan Abdullah Riayatuddin", "Abdullah of Pahang"),
    ("Sultan Mizan Zainal Abidin", "Mizan Zainal Abidin of Terengganu"),
    ("Sultan Sharafuddin Idris Shah", "Sharafuddin of Selangor"),
    ("Sultan Muhammad V", "Muhammad V of Kelantan"),
    ("Tuanku Muhriz", "Muhriz of Negeri Sembilan"),
]

SPECIAL_URLS = {
    "Anwar Ibrahim": "https://www.pmo.gov.my/assets/images/1.png",
    "Ahmad Zahid Hamidi": "https://www.pmo.gov.my/assets/images/3.jpg",
    "Fadillah Yusof": "https://www.pmo.gov.my/assets/images/5.jpg",
    "Loke Siew Fook": "https://www.pmo.gov.my/assets/images/8.png",
    "Mohamad Sabu": "https://www.pmo.gov.my/assets/images/9.jpg",
    "Nga Kor Ming": "https://www.pmo.gov.my/assets/images/10.jpg",
    "Mohamad Hasan": "https://www.pmo.gov.my/assets/images/12.png",
    "Alexander Nanta Linggi": "https://www.pmo.gov.my/assets/images/13.jpg",
    "Saifuddin Nasution Ismail": "https://www.pmo.gov.my/assets/images/14.jpg",
    "Mohamed Khaled Nordin": "https://www.pmo.gov.my/assets/images/15.jpg",
    "Chang Lih Kang": "https://www.pmo.gov.my/assets/images/18.png",
    "Azalina Othman Said": "https://www.pmo.gov.my/assets/images/20.png",
    "Sultan Sallehuddin": "https://media.buletintv3.my/2023/06/SULTAN-KEDAH-1024x538.jpg",
    "Sultan Nazrin Muizzuddin Shah": "https://dewanmasyarakat.jendeladbp.my/app/uploads/sites/4/2021/10/Untitled-design-3-3-1024x724.jpg",
    "Sultan Abdullah Riayatuddin": "https://www.majlisraja-raja.gov.my/images/2025/01/08/sultan_pahang.jpg",
    "Sultan Mizan Zainal Abidin": "https://kesultanantrg.unisza.edu.my/assets/image/sultan%20mizan/sultan-mizan.png",
    "Sultan Sharafuddin Idris Shah": "https://assets.theedgemarkets.com/selangor-sultan_21_0.jpg",
    "Sultan Muhammad V": "https://media.astroawani.com/awani/media/article/2022/Mar/28/81588064902_SultanKelantanSul.jpg",
}


def search_commons(query: str) -> tuple[str, str]:
    title = query.replace(" Malaysia", "").replace(" King", "")
    endpoint = "https://en.wikipedia.org/api/rest_v1/page/summary/" + quote(title.replace(" ", "_"))
    request = Request(
        endpoint,
        headers={"User-Agent": "EnterpriseVisionLocal/1.0"},
    )
    data = None
    try:
        with urlopen(request, timeout=20) as response:
            data = json.load(response)
    except (HTTPError, URLError, TimeoutError):
        search_endpoint = (
            "https://en.wikipedia.org/w/api.php?action=opensearch&limit=5&format=json&search="
            + quote(query)
        )
        search_request = Request(search_endpoint, headers={"User-Agent": "EnterpriseVisionLocal/1.0"})
        try:
            with urlopen(search_request, timeout=20) as response:
                search_data = json.load(response)
            titles = search_data[1] if len(search_data) > 1 else []
            for candidate_title in titles:
                retry_endpoint = "https://en.wikipedia.org/api/rest_v1/page/summary/" + quote(candidate_title.replace(" ", "_"))
                retry_request = Request(retry_endpoint, headers={"User-Agent": "EnterpriseVisionLocal/1.0"})
                try:
                    with urlopen(retry_request, timeout=20) as response:
                        candidate_data = json.load(response)
                    if candidate_data.get("thumbnail", {}).get("source"):
                        data = candidate_data
                        break
                except (HTTPError, URLError, TimeoutError):
                    continue
        except (HTTPError, URLError, TimeoutError):
            pass

    if not data or not data.get("thumbnail", {}).get("source"):
        raise RuntimeError(f"No usable Wikipedia portrait found for {query}")
    thumbnail = data.get("thumbnail", {})
    return thumbnail.get("source", ""), data.get("title", title)


if __name__ == "__main__":
    for name, query in PEOPLE:
        folder = ROOT / name
        folder.mkdir(exist_ok=True)
        target = folder / "01.jpg"
        if target.exists() and target.stat().st_size > 10_000 and name not in {"Sultan Sallehuddin", *SPECIAL_URLS.keys()}:
            print(f"exists: {name}")
            continue
        if name == "Sultan Ibrahim" and target.exists() and target.stat().st_size > 10_000:
            print(f"exists: {name}")
            continue

        try:
            if name in SPECIAL_URLS:
                url, title = SPECIAL_URLS[name], "verified ruler portrait source"
            else:
                url, title = search_commons(query)
            request = Request(url, headers={"User-Agent": "EnterpriseVisionLocal/1.0"})
            with urlopen(request, timeout=30) as response:
                data = response.read()
            target.write_bytes(data)
            print(f"downloaded: {name} <- {title} ({len(data)} bytes)")
        except Exception as exc:
            print(f"warning: could not download {name}: {exc}")
