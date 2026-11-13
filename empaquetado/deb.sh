#!/bin/sh
# Crea el paquete .deb a partir de target/release/musika.
# Uso: sh empaquetado/deb.sh   (después de cargo build --release)
set -eu
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
arq=$(dpkg --print-architecture)
raiz=$(mktemp -d)
chmod 755 "$raiz"
trap 'rm -rf "$raiz"' EXIT

install -Dm755 target/release/musika "$raiz/usr/bin/musika"
install -Dm644 README.md "$raiz/usr/share/doc/musika/README.md"

# Ventana de escritorio: ejecutable, entrada del menú de aplicaciones e icono.
depende="ffmpeg, libc6"
if [ -f gui/target/release/musika-gui ]; then
    install -Dm755 gui/target/release/musika-gui "$raiz/usr/bin/musika-gui"
    install -Dm644 empaquetado/musika.desktop "$raiz/usr/share/applications/io.github.tramankulu.musika.desktop"
    mkdir -p "$raiz/usr/share/icons/hicolor/256x256/apps"
    target/release/musika --icono "$raiz/usr/share/icons/hicolor/256x256/apps/musika.png"
    depende="$depende, libgtk-4-1"
fi
mkdir -p "$raiz/DEBIAN"
cat > "$raiz/DEBIAN/control" <<CONTROL
Package: musika
Version: $version
Architecture: $arq
Maintainer: Tramankulu <tramankulu@users.noreply.github.com>
Depends: $depende
Section: sound
Priority: optional
Homepage: https://github.com/Tramankulu/musika
Description: Prepara canciones para Spotify, YouTube... y crea vídeos de un disco girando
 Ajusta automáticamente la sonoridad de una canción al nivel de cada
 plataforma de streaming y, opcionalmente, genera un vídeo de un vinilo
 girando con el nombre de la canción.
CONTROL

salida="musika_${version}_${arq}.deb"
dpkg-deb --build --root-owner-group "$raiz" "$salida" >/dev/null
echo "$salida"
