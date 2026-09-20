#!/usr/bin/env python3
"""Download and lay out conversion engines for Morpho.

Produces engines/ with:
  ffmpeg/ffmpeg.exe, ffprobe.exe
  poppler/          (pdftoppm, pdftotext, pdfinfo, pdfseparate, pdfunite + DLLs)
  tesseract/        (tesseract.exe + tessdata/)
  pandoc/pandoc.exe
  libreoffice/program/soffice.exe (+ full portable tree)

Engines are never committed to git; CI runs this script too.
Usage: python scripts/fetch_engines.py [--only ffmpeg,pandoc]
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ENGINES = ROOT / "engines"
CACHE = ROOT / ".engine-cache"

FFMPEG_URL = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"
POPPLER_URL = ("https://github.com/oschwartz10612/poppler-windows/releases/"
               "download/v26.07.0-0/Release-26.07.0-0.zip")
PANDOC_VER = "3.11"
PANDOC_URLS = [
    f"https://github.com/jgm/pandoc/releases/download/{PANDOC_VER}/pandoc-{PANDOC_VER}-windows-x86_64.zip",
    f"https://github.com/jgm/pandoc/releases/download/{PANDOC_VER}/pandoc-{PANDOC_VER}-windows-amd64.zip",
]
TESSERACT_URL = "https://digi.bib.uni-mannheim.de/tesseract/tesseract-ocr-w64-setup-5.4.0.20240606.exe"
LO_STILL = "26.2.6"
LO_URLS = [
    f"https://download.documentfoundation.org/libreoffice/stable/{LO_STILL}/win/x86_64/LibreOffice_{LO_STILL}_Win_x86-64.msi",
    f"https://downloadarchive.documentfoundation.org/libreoffice/old/{LO_STILL}/win/x86_64/LibreOffice_{LO_STILL}_Win_x86-64.msi",
]


def _curl_download(url: str, dest: Path) -> bool:
    """Fallback: curl uses the Windows cert store, which tolerates proxy MITM."""
    try:
        subprocess.run(["curl", "-L", "--fail", "--retry", "3", "-o", str(dest), url],
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return dest.exists() and dest.stat().st_size > 0
    except Exception:
        return False


def download(url: str, dest: Path) -> Path:
    dest.parent.mkdir(parents=True, exist_ok=True)
    part = dest.with_suffix(dest.suffix + ".part")
    if dest.exists():
        print(f"[skip] {dest.name} already cached")
        return dest
    print(f"[get ] {url}")
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "morpho-fetch/0.1"})
        with urllib.request.urlopen(req) as resp, open(part, "wb") as fh:
            total = int(resp.headers.get("Content-Length", 0) or 0)
            done = 0
            while True:
                chunk = resp.read(1 << 20)
                if not chunk:
                    break
                fh.write(chunk)
                done += len(chunk)
                if total:
                    print(f"\r       {done/1e6:7.1f}/{total/1e6:.1f} MB", end="", flush=True)
        print()
        # a silent connection drop looks like a clean EOF: refuse to cache
        # a truncated download (Content-Length missing only on chunked repos)
        if total and done != total:
            part.unlink(missing_ok=True)
            raise IOError(f"truncated download: {done}/{total} bytes")
        part.rename(dest)
        return dest
    except Exception as e:
        part.unlink(missing_ok=True)
        print(f"[warn] urllib failed ({e}); retrying with curl")
        if _curl_download(url, dest):
            return dest
        raise


def unzip(src: Path, dest: Path):
    with zipfile.ZipFile(src) as zf:
        zf.extractall(dest)


SEVENZIP_MSI_URL = "https://github.com/ip7z/7zip/releases/download/26.03/7z2603-x64.msi"


def ensure_7z() -> Path:
    """Full 7-Zip console (7z.exe + 7z.dll) laid out via `msiexec /a`.

    7z.dll decodes Inno Setup archives (needed for tesseract); the 7zr.exe /
    extra pack from 7-zip.org does not. msiexec /a only lays files on disk —
    no install, no elevation, so it works on unattended CI runners.
    """
    root = CACHE / "7zip"
    hits = list(root.rglob("7z.exe")) if root.exists() else []
    if hits:
        return hits[0]
    msi = download(SEVENZIP_MSI_URL, CACHE / "7zip.msi")
    root.mkdir(parents=True, exist_ok=True)
    subprocess.run(["msiexec", "/a", str(msi), "/qn", f"TARGETDIR={root}"], check=True)
    hits = list(root.rglob("7z.exe"))
    if not hits:
        raise FileNotFoundError("7z.exe not produced by msiexec /a")
    return hits[0]


def fetch_ffmpeg():
    out = ENGINES / "ffmpeg"
    if (out / "ffmpeg.exe").exists():
        return print("[skip] ffmpeg")
    zipf = download(FFMPEG_URL, CACHE / "ffmpeg.zip")
    with tempfile.TemporaryDirectory() as tmp:
        unzip(zipf, Path(tmp))
        bin_ = next(Path(tmp).glob("*/bin"))
        out.mkdir(parents=True, exist_ok=True)
        for name in ("ffmpeg.exe", "ffprobe.exe"):
            shutil.copy2(bin_ / name, out / name)
    print("[done] ffmpeg")


def fetch_poppler():
    out = ENGINES / "poppler"
    if (out / "pdftoppm.exe").exists():
        return print("[skip] poppler")
    zipf = download(POPPLER_URL, CACHE / "poppler.zip")
    with tempfile.TemporaryDirectory() as tmp:
        unzip(zipf, Path(tmp))
        bin_ = next(Path(tmp).glob("*/poppler-*/Library/bin") for _ in [0]) if False else next(
            p for p in Path(tmp).rglob("pdftoppm.exe")).parent
        out.mkdir(parents=True, exist_ok=True)
        for f in bin_.iterdir():
            if f.suffix.lower() in (".exe", ".dll"):
                shutil.copy2(f, out / f.name)
    print("[done] poppler")


def fetch_pandoc():
    out = ENGINES / "pandoc"
    if (out / "pandoc.exe").exists():
        return print("[skip] pandoc")
    zipf = None
    for url in PANDOC_URLS:
        try:
            zipf = download(url, CACHE / "pandoc.zip")
            break
        except Exception as e:
            print(f"[warn] {url}: {e}")
    if zipf is None:
        raise RuntimeError("pandoc download failed")
    with tempfile.TemporaryDirectory() as tmp:
        unzip(zipf, Path(tmp))
        exe = next(p for p in Path(tmp).rglob("pandoc.exe"))
        out.mkdir(parents=True, exist_ok=True)
        shutil.copy2(exe, out / "pandoc.exe")
    print("[done] pandoc")


def fetch_tesseract():
    out = ENGINES / "tesseract"
    if (out / "tesseract.exe").exists():
        return print("[skip] tesseract")
    installer = download(TESSERACT_URL, CACHE / "tesseract-installer.exe")
    out.mkdir(parents=True, exist_ok=True)
    # Extract, never install: the UB-Mannheim build is Inno Setup whose
    # silent install hangs on unattended CI (UAC elevation waits forever),
    # so we unpack the installer's payload with 7-Zip instead of running it.
    sevenz = ensure_7z()
    subprocess.run([str(sevenz), "x", str(installer), f"-o{out}", "-y"],
                   check=True, stdout=subprocess.DEVNULL)
    shutil.rmtree(out / "$PLUGINSDIR", ignore_errors=True)
    if not (out / "tesseract.exe").exists():
        raise RuntimeError("tesseract.exe missing after extraction")
    print("[done] tesseract")
    # Chinese OCR data (UB-Mannheim default install ships eng+osd only)
    tessdata = out / "tessdata"
    if not (tessdata / "chi_sim.traineddata").exists():
        download("https://github.com/tesseract-ocr/tessdata_fast/raw/main/chi_sim.traineddata",
                 tessdata / "chi_sim.traineddata")
    # trim training tools, uninstaller & docs we never use (keeps the bundle lean)
    for f in out.iterdir():
        if f.is_file() and f.suffix.lower() == ".exe" and f.stem != "tesseract":
            f.unlink()
    for f in out.glob("*.html"):
        f.unlink()
    print("[done] tesseract trimmed")


def fetch_libreoffice():
    out = ENGINES / "libreoffice"
    if (out / "program" / "soffice.exe").exists():
        return print("[skip] libreoffice")
    msi = None
    for url in LO_URLS:
        try:
            msi = download(url, CACHE / f"libreoffice-{LO_STILL}.msi")
            break
        except Exception as e:
            print(f"[warn] {url}: {e}")
    if msi is None:
        raise RuntimeError("libreoffice download failed")
    # Administrative extract: no install, just files laid out on disk.
    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run(["msiexec", "/a", str(msi), "/qn", f"TARGETDIR={Path(tmp)}"],
                       check=True)
        soffice = next(p for p in Path(tmp).rglob("soffice.exe"))
        product_root = soffice.parent if soffice.parent.name != "program" \
            else soffice.parent.parent
        out.mkdir(parents=True, exist_ok=True)
        shutil.copytree(product_root, out, dirs_exist_ok=True)
    # A tree without the launcher is worse than no tree: engines_status would
    # ship "libreoffice: missing" inside a release installer.
    if not (out / "program" / "soffice.exe").exists():
        raise RuntimeError(
            "libreoffice extract did not produce program/soffice.exe; "
            f"got {[p.name for p in out.iterdir()]}")
    print("[done] libreoffice")


def fetch_qpdf():
    out = ENGINES / "qpdf"
    if (out / "qpdf.exe").exists():
        return print("[skip] qpdf")
    headers = {"User-Agent": "morpho-fetch/0.1"}
    # GitHub's unauthenticated API rate limit is tiny; CI/local often has a token
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    if token:
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request("https://api.github.com/repos/qpdf/qpdf/releases/latest",
                                 headers=headers)
    with urllib.request.urlopen(req) as resp:
        release = json.load(resp)
    assets = [a["browser_download_url"] for a in release["assets"]
              if a["name"].endswith(("mingw64.zip", "msvc64.zip"))]
    assets.sort(key=lambda u: 0 if u.endswith("mingw64.zip") else 1)
    if not assets:
        raise RuntimeError("no windows qpdf asset found")
    zipf = download(assets[0], CACHE / "qpdf.zip")
    with tempfile.TemporaryDirectory() as tmp:
        unzip(zipf, Path(tmp))
        bin_ = next(p for p in Path(tmp).rglob("qpdf.exe")).parent
        out.mkdir(parents=True, exist_ok=True)
        for f in bin_.iterdir():
            if f.suffix.lower() in (".exe", ".dll"):
                shutil.copy2(f, out / f.name)
    print("[done] qpdf")


FETCHERS = {"ffmpeg": fetch_ffmpeg, "poppler": fetch_poppler, "pandoc": fetch_pandoc,
            "tesseract": fetch_tesseract, "libreoffice": fetch_libreoffice,
            "qpdf": fetch_qpdf}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", default="")
    args = ap.parse_args()
    CACHE.mkdir(parents=True, exist_ok=True)
    wanted = args.only.split(",") if args.only else list(FETCHERS)
    failed = []
    for name in wanted:
        try:
            FETCHERS[name]()
        except Exception as e:
            print(f"[FAIL] {name}: {e}")
            failed.append(name)
    manifest = {n: (n not in failed) for n in wanted}
    (ENGINES / "manifest.json").write_text(json.dumps(manifest, indent=2))
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
