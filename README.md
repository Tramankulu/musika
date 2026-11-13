# musika

Programa de línea de comandos en Rust, para **Windows y Ubuntu**, que coge una canción y:

1. **Ajusta el audio automáticamente** al nivel de cada plataforma (Spotify, YouTube, Apple Music, Tidal, Amazon Music, Deezer, SoundCloud).
2. Saca, a elegir, **la canción ya ajustada**, **un vídeo de un disco girando** con el nombre de la canción, o las dos cosas.

## Requisitos

Hace falta [FFmpeg](https://ffmpeg.org) (musika lo usa para el audio y el vídeo):

| Sistema | Instalación |
|---|---|
| Windows | `winget install ffmpeg` (o copiar `ffmpeg.exe` y `ffprobe.exe` junto a `musika.exe`) |
| Ubuntu | `sudo apt install ffmpeg` (el `.deb` lo instala solo) |

## Instalación

**Ejecutables ya compilados:** en la pestaña *Actions* del repositorio, cada compilación deja como artefactos `musika-windows` (un zip con `musika.exe`) y `musika-ubuntu` (el paquete `.deb`). Al publicar una etiqueta `v*` (por ejemplo `git tag v0.1.0 && git push --tags`) se crea además una *release* con los dos.

En Ubuntu (22.04 o posterior), el `.deb` se instala así y trae FFmpeg como dependencia:

```
sudo apt install ./musika_0.4.0_amd64.deb
```

**Compilar a mano** (con [Rust](https://rustup.rs) instalado):

```
cargo build --release
```

El ejecutable queda en `target/release/musika` (`musika.exe` en Windows). No tiene dependencias de Rust. En Ubuntu, `sh empaquetado/deb.sh` crea después el `.deb`.

## Uso

Sin argumentos (o con doble clic en Windows) arranca un **modo guiado** que va preguntando.

```
musika <canción> [opciones]
```

| Opción | Qué hace | Por defecto |
|---|---|---|
| `-p, --plataforma` | `spotify`, `youtube`, `apple`, `tidal`, `amazon`, `deezer`, `soundcloud` o `todas`; varias separadas por comas | `spotify` |
| `-s, --salida` | `audio`, `video` o `ambos` | `audio` |
| `-t, --titulo` | Nombre de la canción | etiqueta del archivo o su nombre |
| `-a, --artista` | Artista | etiqueta del archivo |
| `-f, --formato` | `wav` (24 bits), `flac`, `mp3` (320 kbps) o `m4a` (AAC 256 kbps) | `wav` |
| `-o, --dir` | Carpeta de salida | la de la canción |
| `--caratula` | Imagen para la etiqueta del disco y el fondo | etiqueta con el título |
| `--color` | Color `RRGGBB` de la etiqueta y del fondo | `D9482B` |
| `--video-formato` | `horizontal` (1920×1080), `vertical` (1080×1920) o `cuadrado` (1080×1080) | `horizontal` |
| `--lufs`, `--pico` | Nivel a medida (LUFS y dBTP) | — |
| `--forzar-nivel` | Llega siempre al nivel pedido aunque haya que comprimir | ganancia fija |
| `--fondo` | `tocadiscos` (chasis de aluminio) o `liso` | `tocadiscos` |
| `--rpm` | Vueltas por minuto del disco en el vídeo | `8` |
| `--fuente` | Archivo `.ttf` para los textos del vídeo | una del sistema |
| `-l, --listar` | Muestra los niveles de cada plataforma | |

### Ejemplos

```
musika cancion.wav                                  # cancion_spotify.wav
musika cancion.mp3 -p spotify,apple -f flac         # un archivo por plataforma
musika cancion.wav -s video -t "Mi canción" -a "Mi grupo"
musika cancion.wav -p todas -s ambos --caratula portada.jpg
musika cancion.wav -s video --video-formato vertical --color 2B7BD9
```

Los archivos se llaman `<canción>_<plataforma>.<formato>` y `<canción>_video.mp4`.

## Qué hace con el audio

Mide la canción según el estándar ITU-R BS.1770 (sonoridad integrada en LUFS y pico real en dBTP) y le aplica **una ganancia fija**: sube o baja toda la canción por igual, sin tocar la mezcla ni la dinámica, hasta el nivel de la plataforma.

Si para llegar a ese nivel hubiera que superar el pico máximo, se queda en el punto más alto que el pico permite. Es lo mismo que hace Spotify al reproducir: sube las canciones flojas solo hasta donde deja el pico, dejando 1 dB de margen. Con `--forzar-nivel` se llega siempre al nivel, a cambio de comprimir los picos (filtro `loudnorm` de FFmpeg en dos pasadas).

WAV y FLAC salen a 24 bits y con la frecuencia de muestreo del original.

| Plataforma | Sonoridad (LUFS) | Pico máximo (dBTP) |
|---|---|---|
| Spotify | −14 | −1 |
| YouTube | −14 | −1 |
| Apple Music | −16 | −1 |
| Tidal | −14 | −1 |
| Amazon Music | −14 | −2 |
| Deezer | −15 | −1 |
| SoundCloud | −14 | −1 |

Sobre estos números (revisados en noviembre de 2026):

- **Spotify** es la única que publica los suyos: [−14 LUFS y pico por debajo de −1 dBTP](https://support.spotify.com/artists/article/loudness-normalization/), o de −2 dBTP si el máster suena más fuerte que −14 LUFS.
- **El resto** son los valores que manejan las guías de masterización; las plataformas no los documentan y pueden cambiarlos. En Amazon Music unas guías dan −1 dBTP y otras −2; aquí se usa −2, el más prudente. Sobre SoundCloud tampoco hay acuerdo en si normaliza.
- **Un solo archivo a −14 LUFS y −1 dBTP sirve para casi todas.** Las plataformas ajustan el volumen al reproducir, así que no hace falta un archivo distinto para cada una; la única que se aparta de verdad es Apple Music (−16).

Puedes fijar tus propios valores con `--lufs` y `--pico`.

musika solo ajusta el nivel: no ecualiza ni comprime (salvo con `--forzar-nivel`). No sustituye a una masterización; deja la canción al volumen que espera cada plataforma.

## El vídeo

Un tocadiscos visto desde arriba, con un panel de reproductor al lado:

- **El tocadiscos:** un chasis de aluminio cepillado con su plato de corona punteada, botones y control de velocidad. El vinilo gira despacio (8 vueltas por minuto; se cambia con `--rpm`, un tocadiscos real va a 33,3) bajo un brazo con contrapeso, portacápsulas y cápsula. Con `--fondo liso` se quita el chasis y queda solo el disco sobre el fondo.
- **El panel:** una tarjeta translúcida con el título, el artista, un visualizador de barras que sigue la música y la barra de progreso con el tiempo.
- **Con `--caratula`**, la imagen va en la etiqueta central del disco y, desenfocada, de fondo.
- **Sin carátula**, la etiqueta lleva el título y el fondo es un gris oscuro de estudio con un toque del color elegido con `--color`.

Los textos usan la tipografía más moderna que encuentre en el sistema (Inter, Poppins o Ubuntu en Linux; Segoe UI en Windows) o la que indiques con `--fuente`.

### Pensado para YouTube

El vídeo sale con los [ajustes de subida que recomienda YouTube](https://support.google.com/youtube/answer/1722171) y su audio lleva siempre el nivel de YouTube (−14 LUFS, −1 dBTP), sea cual sea la plataforma elegida con `-p` para los archivos de audio:

- MP4 con el índice al principio (*fast start*) y sin listas de edición.
- Vídeo H.264 perfil High, progresivo, 30 fps, 2 fotogramas B, GOP cerrado de medio segundo, 4:2:0 y color BT.709.
- Audio AAC-LC estéreo a 48 kHz y 384 kbps.

El formato `horizontal` (1920×1080) es el de un vídeo normal; el `vertical` (1080×1920) es el de los Shorts.
