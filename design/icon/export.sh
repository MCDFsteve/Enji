#!/bin/sh
# Render the icon sources of build.py into the files Bern.toml declares:
# assets/icon/Enji.icon (macOS and iOS, which Bern compiles with actool),
# enji.ico (Windows), the hicolor theme (Linux), android/res (Android) and
# enji.png (the rest). Needs rsvg-convert and ImageMagick.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
out="$root/assets/icon"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
python3 -I "$here/build.py" >/dev/null
rm -rf "$out"
mkdir -p "$out"

# macOS and iOS: the Liquid Glass document as it is.
cp -R "$here/apple/Enji.icon" "$out/Enji.icon"

# Windows: the bare glyph at the sizes Windows asks for.
pngs=""
for side in 16 20 24 32 40 48 64 128 256; do
  rsvg-convert -w $side -h $side "$here/enji.svg" -o "$work/win-$side.png"
  pngs="$pngs $work/win-$side.png"
done
magick $pngs "$out/enji.ico"

# Linux: an hicolor theme of the bare glyph.
for side in 16 22 24 32 48 64 128 256 512; do
  mkdir -p "$out/hicolor/${side}x${side}/apps"
  rsvg-convert -w $side -h $side "$here/enji.svg" -o "$out/hicolor/${side}x${side}/apps/enji.png"
done
mkdir -p "$out/hicolor/scalable/apps"
cp "$here/enji.svg" "$out/hicolor/scalable/apps/enji.svg"

# Android: the adaptive icon as it is.
mkdir -p "$out/android"
cp -R "$here/android/res" "$out/android/res"

# Any other platform: the glyph on its plate, 1024 pixels.
rsvg-convert -w 1024 -h 1024 "$here/plate.svg" -o "$out/enji.png"
echo "$out"
