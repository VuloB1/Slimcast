#!/bin/bash
# Genera dist/Slimcast-<versión>-x86_64.AppImage con ffmpeg incluido.
# Uso: packaging/linux/build-appimage.sh [binario]   (por defecto target/release/slimcast)
# Compilar en Ubuntu 22.04 o anterior: el AppImage funciona en distros con glibc igual o más nueva.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
bin="${1:-$root/target/release/slimcast}"
work="${WORK:-$root/target/appimage}"
id=io.github.VuloB1.Slimcast
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"
source "$root/packaging/linux/ffmpeg.env"
mkdir -p "$work" "$root/dist"

# ffmpeg estático fijado y comprobado (rama 8.0: NVENC sigue funcionando con drivers de NVIDIA antiguos)
ff="$work/$(basename "$FFMPEG_URL")"
[ -f "$ff" ] || curl -fL --retry 3 -o "$ff" "$FFMPEG_URL"
echo "$FFMPEG_SHA256  $ff" | sha256sum -c -
tool="$work/appimagetool-1.9.1-x86_64.AppImage"
[ -f "$tool" ] || curl -fL --retry 3 -o "$tool" https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
chmod +x "$tool"

app="$work/AppDir"
rm -rf "$app" "$work/ffmpeg"
mkdir -p "$app/usr/bin" "$app/usr/lib" "$app/usr/share/applications" "$app/usr/share/metainfo" \
  "$app/usr/share/icons/hicolor/256x256/apps" "$app/usr/share/licenses/slimcast" "$work/ffmpeg"
tar -xJf "$ff" -C "$work/ffmpeg" --strip-components=1

install -m755 "$bin" "$app/usr/bin/slimcast"
install -m755 "$work/ffmpeg/bin/ffmpeg" "$work/ffmpeg/bin/ffprobe" "$app/usr/bin/"
# su RPATH viene mal formado de fábrica: las encuentran por el LD_LIBRARY_PATH que pone AppRun
cp -a "$work/ffmpeg/lib/"*.so.* "$app/usr/lib/"
install -m755 "$root/packaging/linux/AppRun" "$app/AppRun"
# xkbcommon, xcb, Wayland y Vulkan se usan las del sistema: un xkbcommon incluido más viejo que la configuración
# de teclado de la distro no entiende sus teclas muertas (probado en Arch) y Vulkan depende del driver
install -m644 "$root/packaging/linux/$id.desktop" "$app/usr/share/applications/$id.desktop"
install -m644 "$root/packaging/linux/$id.metainfo.xml" "$app/usr/share/metainfo/$id.metainfo.xml"
install -m644 "$root/assets/icon-256.png" "$app/usr/share/icons/hicolor/256x256/apps/$id.png"
install -m644 "$root/LICENSE" "$app/usr/share/licenses/slimcast/LICENSE"
install -m644 "$work/ffmpeg/LICENSE.txt" "$app/usr/share/licenses/slimcast/ffmpeg-LICENSE.txt"
ln -s "usr/share/applications/$id.desktop" "$app/$id.desktop"
ln -s "usr/share/icons/hicolor/256x256/apps/$id.png" "$app/$id.png"

out="$root/dist/Slimcast-$version-x86_64.AppImage"
# zstd al máximo con bloques de 1 MB (el mksquashfs de appimagetool solo trae zstd): varios MB menos que el nivel por defecto
ARCH=x86_64 "$tool" --appimage-extract-and-run --no-appstream --comp zstd --mksquashfs-opt -Xcompression-level --mksquashfs-opt 22 --mksquashfs-opt -b --mksquashfs-opt 1M "$app" "$out"
ls -la "$out"
