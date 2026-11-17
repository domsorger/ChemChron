#!/bin/sh
# Builds ChemChron on a Mac and wraps it as ChemChron.app (in the
# ChemChron_program folder). Run from the ChemChron_program folder:
#     sh macos/make_app.sh
# Needs Rust (https://rustup.rs) and the Xcode command line tools
# (xcode-select --install). The first build downloads libraries and takes a
# few minutes; later builds are quick.
set -e
cd "$(dirname "$0")/.."
# build inside this folder, whatever folder it sits in
export CARGO_TARGET_DIR="$(pwd)/target"
cargo build --release
APP="ChemChron.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/chemchron "$APP/Contents/MacOS/ChemChron"
# the icon: your own assets/icon.png, or the ChemChron logo
ICONSET="target/ChemChron.iconset"
rm -rf "$ICONSET" && mkdir -p "$ICONSET"
if [ -f assets/icon.png ]; then SRC=assets/icon.png; else SRC=target/icon_1024.png; target/release/chemchron --write-icon "$SRC"; fi
for s in 16 32 128 256 512; do
  sips -z $s $s "$SRC" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2)); sips -z $d $d "$SRC" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/ChemChron.icns"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>ChemChron</string>
  <key>CFBundleDisplayName</key><string>ChemChron</string>
  <key>CFBundleIdentifier</key><string>eu.chemchron.app</string>
  <key>CFBundleVersion</key><string>1.2.0</string>
  <key>CFBundleShortVersionString</key><string>1.2.0</string>
  <key>CFBundleExecutable</key><string>ChemChron</string>
  <key>CFBundleIconFile</key><string>ChemChron</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
# an unsigned app built on this Mac opens without the Gatekeeper warning;
# copied to another Mac, right-click > Open the first time.
echo "Done: $(pwd)/$APP  (double-click it, or: open $APP)"
