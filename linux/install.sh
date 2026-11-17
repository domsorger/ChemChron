#!/bin/sh
# Builds ChemChron on Linux and adds it to your applications menu (for you
# only, no administrator rights needed). Run from the ChemChron_program folder:
#     sh linux/install.sh
# Needs Rust (https://rustup.rs) and a C compiler
# (Ubuntu/Debian: sudo apt install build-essential pkg-config).
set -e
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR="$(pwd)/target"
cargo build --release
BIN="$HOME/.local/bin"
APPS="$HOME/.local/share/applications"
ICONS="$HOME/.local/share/icons/hicolor/512x512/apps"
mkdir -p "$BIN" "$APPS" "$ICONS"
cp target/release/chemchron "$BIN/chemchron"
# the icon: your own assets/icon.png, or the ChemChron logo
if [ -f assets/icon.png ]; then cp assets/icon.png "$ICONS/chemchron.png"; else "$BIN/chemchron" --write-icon "$ICONS/chemchron.png"; fi
cat > "$APPS/chemchron.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=ChemChron
Comment=LA-ICP-MS, EPMA and combined geochronology
Exec=$BIN/chemchron
Icon=$ICONS/chemchron.png
Terminal=false
Categories=Science;Education;
DESKTOP
echo "Done: ChemChron is in your applications menu (or run $BIN/chemchron)."
echo "A copy that runs anywhere on this kind of Linux: target/release/chemchron"
