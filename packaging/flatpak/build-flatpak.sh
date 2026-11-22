#!/bin/bash
# Genera dist/Slimcast-<versión>-x86_64.flatpak (bundle de un solo archivo) a partir de target/release/slimcast.
# Requiere flatpak y flatpak-builder; el runtime se instala solo desde Flathub.
# Instalar: flatpak install --user Slimcast-<versión>-x86_64.flatpak
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
work="${WORK:-$root/target/flatpak}"
id=io.github.VuloB1.Slimcast
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"
mkdir -p "$work" "$root/dist"
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak-builder --user --force-clean --install-deps-from=flathub --state-dir="$work/state" \
  --repo="$work/repo" "$work/build" "$root/packaging/flatpak/$id.yml"
out="$root/dist/Slimcast-$version-x86_64.flatpak"
flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo "$work/repo" "$out" "$id"
ls -la "$out"
