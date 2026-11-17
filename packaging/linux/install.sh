#!/bin/sh
# Installs the ChemChron in this folder for you (no administrator rights):
# the program in ~/.local/bin and an entry with icon in the applications menu.
#     sh install.sh
set -e
cd "$(dirname "$0")"
mkdir -p "$HOME/.local/bin" "$HOME/.local/share/applications" "$HOME/.local/share/icons/hicolor/512x512/apps"
cp chemchron "$HOME/.local/bin/chemchron"
cp chemchron.png "$HOME/.local/share/icons/hicolor/512x512/apps/chemchron.png"
sed "s|^Exec=.*|Exec=$HOME/.local/bin/chemchron|; s|^Icon=.*|Icon=$HOME/.local/share/icons/hicolor/512x512/apps/chemchron.png|" chemchron.desktop > "$HOME/.local/share/applications/chemchron.desktop"
echo "Done: ChemChron is in your applications menu."
