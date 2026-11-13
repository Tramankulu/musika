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
sudo apt install ./musika_0.1.0_amd64.deb
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
| `--caratula` | Imagen para la etiqueta central del disco | etiqueta con el título |
| `--color` | Color `RRGGBB` de la etiqueta y del fondo | `D9482B` |
| `--video-formato` | `horizontal` (1920×1080), `vertical` (1080×1920) o `cuadrado` (1080×1080) | `horizontal` |
| `--lufs`, `--pico` | Nivel a medida (LUFS y dBTP) | — |
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

Normalización de sonoridad en dos pasadas con el filtro `loudnorm` de FFmpeg (EBU R128): primero mide la canción y después aplica la ganancia necesaria para llegar al nivel de la plataforma sin pasar del pico máximo.

| Plataforma | Sonoridad (LUFS) | Pico máximo (dBTP) |
|---|---|---|
| Spotify | −14 | −1 |
| YouTube | −14 | −1 |
| Apple Music | −16 | −1 |
| Tidal | −14 | −1 |
| Amazon Music | −14 | −2 |
| Deezer | −15 | −1 |
| SoundCloud | −14 | −1 |

Son los niveles de referencia habituales; las plataformas los cambian de vez en cuando, así que puedes fijar los tuyos con `--lufs` y `--pico`.

- **Ganancia lineal:** se sube o baja el volumen entero por igual; la dinámica de la mezcla no cambia. Es lo normal.
- **Ajuste dinámico:** si la canción no puede llegar al nivel sin superar el pico, FFmpeg aplica una ganancia variable. El programa avisa cuando ocurre.

musika solo ajusta el nivel: no ecualiza ni comprime. No sustituye a una masterización, deja la canción al volumen que espera cada plataforma.

## El vídeo

Un vinilo girando a 33⅓ rpm con el título y el artista, la forma de onda de la canción y, en la etiqueta central, la carátula que le pases (o el título si no pasas ninguna). H.264 a 30 fps con audio AAC a 320 kbps, usando el ajuste de YouTube si está entre las plataformas pedidas y, si no, el de la primera.
