#!/usr/bin/env bash
# Morpho end-to-end regression suite.
# Drives the morpho CLI through every supported pipeline and checks outputs.
# Usage: scripts/e2e_test.sh [morpho-binary]
set -u
cd "$(dirname "$0")/.."
BIN="${1:-$PWD/target/release/morpho}"
if [ ! -x "$BIN" ]; then
  echo "morpho binary not found: $BIN (build with: cargo build -p morpho-cli --release)" >&2
  exit 2
fi
export MORPHO_ENGINES_DIR="${MORPHO_ENGINES_DIR:-$PWD/engines}"
if [ ! -d "$MORPHO_ENGINES_DIR" ]; then
  echo "engines directory not found: $MORPHO_ENGINES_DIR" >&2
  exit 3
fi

WORK="$(mktemp -d /tmp/morpho-e2e.XXXXXX)"
trap "rm -rf $WORK" EXIT
cd "$WORK"

PASS=0
FAIL=0
FAILED=""

t() { # t <name> <command...> — run and record pass/fail
  local name="$1"; shift
  if "$@" >/dev/null 2>&1; then
    PASS=$((PASS+1)); echo "  ok   $name"
  else
    FAIL=$((FAIL+1)); FAILED="$FAILED $name"; echo "  FAIL $name"
  fi
}

# ---------- assets ----------
FF="$MORPHO_ENGINES_DIR/ffmpeg/ffmpeg"
SOFFICE=""
for c in "$MORPHO_ENGINES_DIR/libreoffice/program/soffice" "/Applications/LibreOffice.app/Contents/MacOS/soffice"; do
  if [ -x "$c" ]; then SOFFICE="$c"; break; fi
done

"$FF" -y -loglevel error -f lavfi -i testsrc=duration=1:size=320x240:rate=10 -f lavfi -i sine=duration=1 -shortest -c:v libx264 -pix_fmt yuv420p -c:a aac clip.mp4
"$FF" -y -loglevel error -f lavfi -i testsrc=duration=0.5:size=256x256:rate=8 -frames:v 1 pic.png
"$FF" -y -loglevel error -f lavfi -i testsrc=size=640x640 -frames:v 1 big.png
"$FF" -y -loglevel error -f lavfi -i testsrc=duration=0.5:size=256x256:rate=8 clip.gif
printf "Chapter One\n\nHello paragraph.\n" > doc.md
printf "name,qty\napple,3\nbanana,7\n" > list.csv
printf "{\\rtf1\\ansi Hello RTF world.}\n" > doc.rtf
mkdir -p out1 out2 out3 ocrt

"$BIN" convert doc.md --to docx >/dev/null 2>&1
if [ ! -s doc.docx ]; then echo "asset generation failed (docx)" >&2; exit 3; fi
cp doc.md report.final.md
cp doc.docx report.final.docx

# ---------- image pipelines ----------
echo "== images =="
t png-webp    "$BIN" convert pic.png --to webp
t png-avif    "$BIN" convert pic.png --to avif
t png-jpg-q   "$BIN" convert pic.png --to jpg --quality 95
t png-ico-big "$BIN" convert big.png --to ico
t png-bmp     "$BIN" convert pic.png --to bmp

# ---------- audio / video pipelines ----------
echo "== audio/video =="
t mp4-mkv     "$BIN" convert clip.mp4 --to mkv
t mp4-mp3     "$BIN" convert clip.mp4 --to mp3
t mp4-gif2p   "$BIN" convert clip.mp4 --to gif
t mp4-poster  "$BIN" convert clip.mp4 --to png
t gif-mp4     "$BIN" convert clip.gif --to mp4
t reencode-mp4 "$BIN" convert clip.mp4 --to mp4 --preset wechat
t same-format-rejected sh -c "if $BIN convert clip.mp4 --to mp4 >/dev/null 2>&1; then exit 1; fi"
t unknown-format-rejected sh -c "if $BIN convert pic.png --to zzz >/dev/null 2>&1; then exit 1; fi"

# ---------- document pipelines ----------
echo "== documents =="
t md-pdf      "$BIN" convert doc.md --to pdf
t md-docx     "$BIN" convert report.final.md --to docx --out out1
t rtf-pdf     "$BIN" convert doc.rtf --to pdf
t multidot-docx-pdf sh -c "$BIN convert report.final.docx --to pdf --out out2 >/dev/null 2>&1 && [ -s out2/report.final.pdf ]"
t csv-xlsx    "$BIN" convert list.csv --to xlsx --out out1
t xlsx-csv    sh -c "$BIN convert out1/list.xlsx --to csv --out out3 >/dev/null 2>&1 && grep -q apple out3/list.csv"

# ---------- pdf pipelines ----------
echo "== pdf =="
t pdf-txt     sh -c "$BIN convert doc.pdf --to txt --out out1 >/dev/null 2>&1 && grep -q Hello out1/doc.txt"
t pdf-png     sh -c "$BIN convert doc.pdf --to png --out out2 >/dev/null 2>&1 && ls out2/doc*.png >/dev/null 2>&1"
t pdf-docx    sh -c "$BIN convert doc.pdf --to docx --out out3 >/dev/null 2>&1 && [ -s out3/doc.docx ]"
t pdf-md      sh -c "$BIN convert doc.pdf --to md --out out1 >/dev/null 2>&1 && grep -q Hello out1/doc.md"

# ---------- ocr pipelines ----------
echo "== ocr =="
t ocr-png-txt sh -c "cd out2 && $BIN convert doc.png --to txt --out ../ocrt >/dev/null 2>&1 && [ -s ../ocrt/doc.txt ]"
t ocr-spdf    sh -c "cd out2 && $BIN convert doc.png --to spdf --out ../ocrt >/dev/null 2>&1 && [ -s ../ocrt/doc.pdf ]"

# ---------- pdf tools ----------
echo "== pdf tools =="
t pdf-merge   "$BIN" pdf merge doc.pdf doc.pdf --out merged.pdf
t pdf-split   sh -c "$BIN pdf split merged.pdf --out pages >/dev/null 2>&1 && [ -s pages/merged-1.pdf ] && [ -s pages/merged-2.pdf ]"
t pdf-encrypt "$BIN" pdf encrypt doc.pdf --out secret.pdf -p pw123
t pdf-decrypt sh -c "$BIN pdf decrypt secret.pdf --out plain.pdf -p pw123 >/dev/null 2>&1 && $BIN convert plain.pdf --to txt --out out2 >/dev/null 2>&1 && grep -q Hello out2/plain.txt"

# ---------- summary ----------
echo
echo "passed: $PASS  failed: $FAIL"
if [ "$FAIL" -ne 0 ]; then
  echo "failed cases:$FAILED"
  exit 1
fi
echo "all pipelines passed"
