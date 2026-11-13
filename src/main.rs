//! musika — prepara una canción para las plataformas de streaming y,
//! opcionalmente, genera un vídeo de un disco girando con el nombre de la canción.
//!
//! No tiene dependencias de Rust: todo el trabajo de audio y vídeo lo hace
//! FFmpeg, que debe estar instalado (o junto al ejecutable).

mod imagen;

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

type Res<T> = Result<T, String>;

// ───────────────────────── Plataformas ─────────────────────────

struct Plataforma {
    id: &'static str,
    nombre: &'static str,
    lufs: f64,
    pico: f64,
}

/// Niveles de referencia habituales de cada plataforma (sonoridad integrada
/// en LUFS y pico real máximo en dBTP).
const PLATAFORMAS: &[Plataforma] = &[
    Plataforma { id: "spotify", nombre: "Spotify", lufs: -14.0, pico: -1.0 },
    Plataforma { id: "youtube", nombre: "YouTube", lufs: -14.0, pico: -1.0 },
    Plataforma { id: "apple", nombre: "Apple Music", lufs: -16.0, pico: -1.0 },
    Plataforma { id: "tidal", nombre: "Tidal", lufs: -14.0, pico: -1.0 },
    Plataforma { id: "amazon", nombre: "Amazon Music", lufs: -14.0, pico: -2.0 },
    Plataforma { id: "deezer", nombre: "Deezer", lufs: -15.0, pico: -1.0 },
    Plataforma { id: "soundcloud", nombre: "SoundCloud", lufs: -14.0, pico: -1.0 },
];

#[derive(Clone)]
struct Objetivo {
    id: String,
    nombre: String,
    lufs: f64,
    pico: f64,
}

// ───────────────────────── Opciones ─────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Salida {
    Audio,
    Video,
    Ambos,
}

struct Opciones {
    entrada: PathBuf,
    plataformas: Vec<String>,
    salida: Salida,
    titulo: Option<String>,
    artista: Option<String>,
    formato: String,
    dir: Option<PathBuf>,
    caratula: Option<PathBuf>,
    color: imagen::Rgb,
    video_formato: String,
    lufs: Option<f64>,
    pico: Option<f64>,
    fuente: Option<PathBuf>,
}

const AYUDA: &str = "\
musika — prepara una canción para Spotify, YouTube… y crea un vídeo de un disco girando

USO:
    musika <canción> [opciones]
    musika                      (sin argumentos: modo guiado con preguntas)

OPCIONES:
    -p, --plataforma <lista>    spotify, youtube, apple, tidal, amazon, deezer,
                                soundcloud o todas; varias separadas por comas
                                [por defecto: spotify]
    -s, --salida <tipo>         audio | video | ambos            [por defecto: audio]
    -t, --titulo <texto>        Nombre de la canción (por defecto: etiqueta o nombre del archivo)
    -a, --artista <texto>       Artista
    -f, --formato <fmt>         wav | flac | mp3 | m4a           [por defecto: wav]
    -o, --dir <carpeta>         Carpeta de salida (por defecto: la de la canción)
        --caratula <imagen>     Imagen para la etiqueta central del disco
        --color <RRGGBB>        Color de la etiqueta y del fondo  [por defecto: D9482B]
        --video-formato <f>     horizontal (1920x1080) | vertical (1080x1920) | cuadrado (1080x1080)
        --lufs <n>              Sonoridad a medida, p. ej. -14 (crea la salida «personalizado»)
        --pico <n>              Pico real máximo a medida en dBTP, p. ej. -1
        --fuente <archivo.ttf>  Tipografía para los textos del vídeo
    -l, --listar                Muestra los niveles de cada plataforma
    -h, --ayuda                 Muestra esta ayuda

EJEMPLOS:
    musika cancion.wav
    musika cancion.mp3 -p spotify,apple -f flac
    musika cancion.wav -s video -t \"Mi canción\" -a \"Mi grupo\"
    musika cancion.wav -p todas -s ambos --caratula portada.jpg
";

fn parsear_color(s: &str) -> Res<imagen::Rgb> {
    let h = s.trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("Color no válido «{s}»: usa el formato RRGGBB, p. ej. D9482B"));
    }
    let n = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap();
    Ok((n(0), n(2), n(4)))
}

fn listar_plataformas() {
    println!("{:<12} {:<14} {:>8} {:>10}", "id", "plataforma", "LUFS", "pico dBTP");
    for p in PLATAFORMAS {
        println!("{:<12} {:<14} {:>8} {:>10}", p.id, p.nombre, p.lufs, p.pico);
    }
}

fn opciones_por_defecto(entrada: PathBuf) -> Opciones {
    Opciones {
        entrada,
        plataformas: vec!["spotify".into()],
        salida: Salida::Audio,
        titulo: None,
        artista: None,
        formato: "wav".into(),
        dir: None,
        caratula: None,
        color: (0xD9, 0x48, 0x2B),
        video_formato: "horizontal".into(),
        lufs: None,
        pico: None,
        fuente: None,
    }
}

fn parsear_salida(s: &str) -> Res<Salida> {
    match s.to_lowercase().as_str() {
        "audio" | "1" => Ok(Salida::Audio),
        "video" | "vídeo" | "2" => Ok(Salida::Video),
        "ambos" | "3" => Ok(Salida::Ambos),
        _ => Err(format!("Salida no válida «{s}»: usa audio, video o ambos")),
    }
}

fn parsear_args(args: &[String]) -> Res<Option<Opciones>> {
    let mut o = opciones_por_defecto(PathBuf::new());
    let mut entrada: Option<PathBuf> = None;
    let mut plataformas_dadas = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let mut valor = || -> Res<String> {
            i += 1;
            args.get(i).cloned().ok_or_else(|| format!("Falta el valor de la opción {a}"))
        };
        match a {
            "-h" | "--ayuda" | "--help" => {
                print!("{AYUDA}");
                return Ok(None);
            }
            "-l" | "--listar" => {
                listar_plataformas();
                return Ok(None);
            }
            "-p" | "--plataforma" => {
                o.plataformas = valor()?.split(',').map(|s| s.trim().to_lowercase()).collect();
                plataformas_dadas = true;
            }
            "-s" | "--salida" => o.salida = parsear_salida(&valor()?)?,
            "-t" | "--titulo" => o.titulo = Some(valor()?),
            "-a" | "--artista" => o.artista = Some(valor()?),
            "-f" | "--formato" => o.formato = valor()?.to_lowercase(),
            "-o" | "--dir" => o.dir = Some(PathBuf::from(valor()?)),
            "--caratula" => o.caratula = Some(PathBuf::from(valor()?)),
            "--color" => o.color = parsear_color(&valor()?)?,
            "--video-formato" => o.video_formato = valor()?.to_lowercase(),
            "--lufs" => o.lufs = Some(numero(&valor()?, "--lufs")?),
            "--pico" => o.pico = Some(numero(&valor()?, "--pico")?),
            "--fuente" => o.fuente = Some(PathBuf::from(valor()?)),
            _ if a.starts_with('-') && a.len() > 1 => {
                return Err(format!("Opción desconocida: {a}\nUsa --ayuda para ver las opciones."))
            }
            _ => {
                if entrada.is_some() {
                    return Err("Solo se admite una canción cada vez.".into());
                }
                entrada = Some(PathBuf::from(a));
            }
        }
        i += 1;
    }
    o.entrada = entrada.ok_or("Falta el archivo de la canción. Usa --ayuda para ver el uso.")?;
    if o.lufs.is_some() && !plataformas_dadas {
        o.plataformas.clear(); // solo la salida personalizada
    }
    Ok(Some(o))
}

fn numero(s: &str, opcion: &str) -> Res<f64> {
    s.replace(',', ".").parse().map_err(|_| format!("Valor numérico no válido para {opcion}: «{s}»"))
}

// ───────────────────────── Modo guiado ─────────────────────────

fn preguntar(texto: &str) -> String {
    print!("{texto}");
    let _ = io::stdout().flush();
    let mut s = String::new();
    let _ = io::stdin().read_line(&mut s);
    s.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_string()
}

fn modo_guiado() -> Res<Opciones> {
    println!("musika — modo guiado (usa «musika --ayuda» para ver las opciones de línea de comandos)\n");
    let ruta = preguntar("Arrastra aquí la canción o escribe su ruta: ");
    if ruta.is_empty() {
        return Err("No se ha indicado ninguna canción.".into());
    }
    let mut o = opciones_por_defecto(PathBuf::from(ruta));

    println!("\nPlataformas: spotify, youtube, apple, tidal, amazon, deezer, soundcloud, todas");
    let p = preguntar("¿Para qué plataformas? (separadas por comas) [spotify]: ");
    if !p.is_empty() {
        o.plataformas = p.split(',').map(|s| s.trim().to_lowercase()).collect();
    }
    let s = preguntar("\n¿Qué quieres obtener? 1) audio  2) vídeo del disco  3) ambos [1]: ");
    if !s.is_empty() {
        o.salida = parsear_salida(&s)?;
    }
    if o.salida != Salida::Video {
        let f = preguntar("Formato del audio: wav, flac, mp3, m4a [wav]: ");
        if !f.is_empty() {
            o.formato = f.to_lowercase();
        }
    }
    if o.salida != Salida::Audio {
        let t = preguntar("Nombre de la canción [Intro = el del archivo]: ");
        if !t.is_empty() {
            o.titulo = Some(t);
        }
        let a = preguntar("Artista [Intro = el del archivo]: ");
        if !a.is_empty() {
            o.artista = Some(a);
        }
        let c = preguntar("Imagen para la etiqueta del disco [Intro = ninguna]: ");
        if !c.is_empty() {
            o.caratula = Some(PathBuf::from(c));
        }
    }
    println!();
    Ok(o)
}

// ───────────────────────── FFmpeg ─────────────────────────

struct Ff {
    ffmpeg: PathBuf,
    ffprobe: Option<PathBuf>,
}

fn buscar_programa(nombre: &str) -> Option<PathBuf> {
    let exe = format!("{nombre}{}", std::env::consts::EXE_SUFFIX);
    let mut candidatos = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        candidatos.push(dir.join(&exe));
    }
    candidatos.push(PathBuf::from(&exe));
    candidatos.into_iter().find(|c| {
        Command::new(c)
            .arg("-version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

fn localizar_ffmpeg() -> Res<Ff> {
    let ffmpeg = buscar_programa("ffmpeg").ok_or(
        "No se encuentra FFmpeg. Instálalo y vuelve a intentarlo:\n  \
         Windows:  winget install ffmpeg   (o copia ffmpeg.exe junto a musika.exe)\n  \
         Ubuntu:   sudo apt install ffmpeg",
    )?;
    Ok(Ff { ffmpeg, ffprobe: buscar_programa("ffprobe") })
}

/// Ejecuta FFmpeg y devuelve lo que escribe por stderr.
fn ffmpeg(ff: &Ff, dir: &Path, args: &[String]) -> Res<String> {
    let r = Command::new(&ff.ffmpeg)
        .current_dir(dir)
        .args(["-hide_banner", "-nostdin", "-y"])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("No se pudo ejecutar FFmpeg: {e}"))?;
    let err = String::from_utf8_lossy(&r.stderr).into_owned();
    if !r.status.success() {
        let cola: Vec<&str> = err.lines().rev().take(8).collect();
        let cola: Vec<&str> = cola.into_iter().rev().collect();
        return Err(format!("FFmpeg ha fallado:\n{}", cola.join("\n")));
    }
    Ok(err)
}

#[derive(Default)]
struct Info {
    titulo: Option<String>,
    artista: Option<String>,
    duracion: Option<f64>,
    frecuencia: Option<u32>,
}

fn leer_info(ff: &Ff, entrada: &Path) -> Info {
    let mut info = Info::default();
    let Some(probe) = &ff.ffprobe else { return info };
    let Ok(r) = Command::new(probe)
        .args(["-v", "error", "-select_streams", "a:0", "-show_entries"])
        .arg("stream=sample_rate:stream_tags=title,artist:format=duration:format_tags=title,artist")
        .args(["-of", "default=noprint_wrappers=1"])
        .arg(entrada)
        .stdin(Stdio::null())
        .output()
    else {
        return info;
    };
    for linea in String::from_utf8_lossy(&r.stdout).lines() {
        let Some((k, v)) = linea.split_once('=') else { continue };
        let v = v.trim();
        if v.is_empty() || v == "N/A" {
            continue;
        }
        match k.to_lowercase().as_str() {
            "sample_rate" => info.frecuencia = v.parse().ok(),
            "duration" => info.duracion = v.parse().ok(),
            "tag:title" => info.titulo = Some(v.to_string()),
            "tag:artist" => info.artista = Some(v.to_string()),
            _ => {}
        }
    }
    info
}

// ───────────────────────── Masterización ─────────────────────────

#[derive(Clone)]
struct Medida {
    i: f64,
    tp: f64,
    lra: f64,
    umbral: f64,
    offset: f64,
}

/// Extrae un valor del bloque JSON que imprime el filtro loudnorm.
fn campo_json(texto: &str, clave: &str) -> Option<String> {
    let inicio = texto.rfind('{')?;
    let bloque = &texto[inicio..];
    let pos = bloque.find(&format!("\"{clave}\""))?;
    let resto = &bloque[pos + clave.len() + 2..];
    let resto = &resto[resto.find('"')? + 1..];
    Some(resto[..resto.find('"')?].to_string())
}

fn campo_num(texto: &str, clave: &str) -> Res<f64> {
    let v = campo_json(texto, clave).ok_or("No se pudo leer el análisis de sonoridad de FFmpeg.")?;
    let n: f64 = v.parse().map_err(|_| format!("Valor inesperado en el análisis ({clave} = {v})."))?;
    if !n.is_finite() {
        return Err("El audio parece estar en silencio: no se puede medir su sonoridad.".into());
    }
    Ok(n)
}

/// Primera pasada: mide la canción.
fn medir(ff: &Ff, tmp: &Path, entrada: &Path, ob: &Objetivo) -> Res<Medida> {
    let filtro = format!("loudnorm=I={}:TP={}:LRA=11:print_format=json", ob.lufs, ob.pico);
    let mut args = vec!["-i".to_string(), ruta(entrada)];
    args.extend(["-map", "0:a:0", "-af", &filtro, "-f", "null", "-"].map(String::from));
    let salida = ffmpeg(ff, tmp, &args)?;
    Ok(Medida {
        i: campo_num(&salida, "input_i")?,
        tp: campo_num(&salida, "input_tp")?,
        lra: campo_num(&salida, "input_lra")?,
        umbral: campo_num(&salida, "input_thresh")?,
        offset: campo_num(&salida, "target_offset")?,
    })
}

struct Resultado {
    lufs: f64,
    pico: f64,
    lineal: bool,
}

fn codec_audio(formato: &str) -> Res<Vec<&'static str>> {
    Ok(match formato {
        "wav" => vec!["-c:a", "pcm_s24le"],
        "flac" => vec!["-c:a", "flac", "-sample_fmt", "s32", "-bits_per_raw_sample", "24"],
        "mp3" => vec!["-c:a", "libmp3lame", "-b:a", "320k"],
        "m4a" => vec!["-c:a", "aac", "-b:a", "256k"],
        _ => return Err(format!("Formato no válido «{formato}»: usa wav, flac, mp3 o m4a")),
    })
}

/// Segunda pasada: aplica la ganancia con los valores medidos y escribe el archivo.
#[allow(clippy::too_many_arguments)]
fn masterizar(
    ff: &Ff,
    tmp: &Path,
    entrada: &Path,
    destino: &Path,
    ob: &Objetivo,
    m: &Medida,
    formato: &str,
    frecuencia: u32,
    etiquetas: &[(&str, &str)],
) -> Res<Resultado> {
    // Con un LRA objetivo >= al medido, loudnorm trabaja en modo lineal
    // (una ganancia fija, sin tocar la dinámica) siempre que el pico lo permita.
    let lra = m.lra.clamp(11.0, 50.0);
    let filtro = format!(
        "loudnorm=I={}:TP={}:LRA={lra}:measured_I={}:measured_TP={}:measured_LRA={}:\
         measured_thresh={}:offset={}:linear=true:print_format=json",
        ob.lufs, ob.pico, m.i, m.tp, m.lra, m.umbral, m.offset
    );
    let mut args = vec!["-i".to_string(), ruta(entrada)];
    args.extend(["-map", "0:a:0", "-vn", "-map_metadata", "0", "-af", &filtro].map(String::from));
    args.extend(["-ar".to_string(), frecuencia.to_string()]);
    for (k, v) in etiquetas {
        args.extend(["-metadata".to_string(), format!("{k}={v}")]);
    }
    args.extend(codec_audio(formato)?.into_iter().map(String::from));
    args.push(ruta(destino));
    let salida = ffmpeg(ff, tmp, &args)?;
    Ok(Resultado {
        lufs: campo_num(&salida, "output_i")?,
        pico: campo_num(&salida, "output_tp")?,
        lineal: campo_json(&salida, "normalization_type").is_some_and(|t| t == "linear"),
    })
}

// ───────────────────────── Vídeo ─────────────────────────

struct Diseno {
    ancho: usize,
    alto: usize,
    disco: usize,
    disco_x: usize,
    disco_y: usize,
    texto_cx: usize,
    texto_ancho: usize,
    titulo_y: usize,
    ondas: (usize, usize, usize, usize), // x, y, ancho, alto
}

fn diseno(formato: &str) -> Res<Diseno> {
    Ok(match formato {
        "horizontal" => Diseno {
            ancho: 1920, alto: 1080, disco: 820, disco_x: 120, disco_y: 130,
            texto_cx: 1430, texto_ancho: 800, titulo_y: 390, ondas: (1050, 640, 760, 160),
        },
        "vertical" => Diseno {
            ancho: 1080, alto: 1920, disco: 900, disco_x: 90, disco_y: 260,
            texto_cx: 540, texto_ancho: 960, titulo_y: 1290, ondas: (60, 1540, 960, 200),
        },
        "cuadrado" => Diseno {
            ancho: 1080, alto: 1080, disco: 640, disco_x: 220, disco_y: 60,
            texto_cx: 540, texto_ancho: 980, titulo_y: 760, ondas: (50, 930, 980, 120),
        },
        _ => return Err(format!("Formato de vídeo no válido «{formato}»: usa horizontal, vertical o cuadrado")),
    })
}

fn buscar_fuente(indicada: Option<&Path>) -> Res<Option<PathBuf>> {
    if let Some(f) = indicada {
        return if f.is_file() { Ok(Some(f.to_path_buf())) } else { Err(format!("No existe la fuente {}", f.display())) };
    }
    let mut candidatos: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        let win = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
        for f in ["segoeuib.ttf", "arialbd.ttf", "arial.ttf", "tahoma.ttf"] {
            candidatos.push(Path::new(&win).join("Fonts").join(f));
        }
    } else {
        candidatos.extend(
            [
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
                "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
                "/usr/share/fonts/truetype/liberation2/LiberationSans-Bold.ttf",
                "/usr/share/fonts/truetype/ubuntu/Ubuntu-B.ttf",
                "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
                "/usr/share/fonts/truetype/freefont/FreeSansBold.ttf",
                "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
                "/Library/Fonts/Arial Bold.ttf",
            ]
            .map(PathBuf::from),
        );
    }
    if let Some(f) = candidatos.into_iter().find(|c| c.is_file()) {
        return Ok(Some(f));
    }
    // Último recurso en Linux: preguntar a fontconfig.
    let fc = Command::new("fc-match").args(["-f", "%{file}", "sans:bold"]).output().ok();
    Ok(fc
        .map(|r| PathBuf::from(String::from_utf8_lossy(&r.stdout).trim().to_string()))
        .filter(|p| p.is_file()))
}

fn recortar(texto: &str, max: usize) -> String {
    if texto.chars().count() <= max {
        texto.to_string()
    } else {
        let mut s: String = texto.chars().take(max.saturating_sub(1)).collect();
        s.push('…');
        s
    }
}

/// Tamaño de letra para que `n` caracteres quepan en `ancho` píxeles.
fn cuerpo(ancho: usize, n: usize, min: f64, max: f64) -> u32 {
    (ancho as f64 / (0.6 * n.max(1) as f64)).clamp(min.min(max), max) as u32
}

struct Video<'a> {
    audio: &'a Path,
    destino: &'a Path,
    titulo: &'a str,
    artista: &'a str,
    duracion: Option<f64>,
}

fn crear_video(ff: &Ff, tmp: &Path, o: &Opciones, v: &Video) -> Res<()> {
    let d = diseno(&o.video_formato)?;
    let lado = d.disco;
    let escribir = |nombre: &str, datos: &[u8]| {
        std::fs::write(tmp.join(nombre), datos).map_err(|e| format!("No se pudo escribir un archivo temporal: {e}"))
    };

    // Imágenes base.
    let png = |nombre: &str, w: usize, h: usize, px: &[u8]| {
        imagen::escribir_png(&tmp.join(nombre), w, h, px).map_err(|e| format!("No se pudo crear {nombre}: {e}"))
    };
    png("disco.png", lado, lado, &imagen::dibujar_disco(lado, o.color))?;
    let centro = (d.disco_x as f64 + lado as f64 / 2.0, d.disco_y as f64 + lado as f64 / 2.0);
    png("fondo.png", d.ancho, d.alto, &imagen::dibujar_fondo(d.ancho, d.alto, o.color, (centro.0, centro.1, lado as f64 / 2.0)))?;

    // Tipografía: se copia a la carpeta temporal para referirla por nombre y
    // no tener que escapar rutas (las de Windows llevan «:»).
    let fuente = match buscar_fuente(o.fuente.as_deref())? {
        Some(f) => {
            std::fs::copy(&f, tmp.join("fuente.ttf")).map_err(|e| format!("No se pudo leer la fuente {}: {e}", f.display()))?;
            "fontfile=fuente.ttf:"
        }
        None => "",
    };
    // Los textos van en archivos para no tener que escapar nada.
    let texto = |archivo: &str, contenido: &str, color: &str, cuerpo: u32, x: &str, y: &str| -> Res<String> {
        escribir(archivo, contenido.as_bytes())?;
        Ok(format!(
            "drawtext={fuente}textfile={archivo}:expansion=none:fontcolor={color}:fontsize={cuerpo}:x={x}:y={y}"
        ))
    };

    // Etiqueta central: carátula recortada en círculo o, si no hay, textos.
    let etiqueta = (lado as f64 * imagen::R_ETIQUETA * 0.9) as usize / 2 * 2;
    let mut disco_listo = "disco.png".to_string();
    let mut filtro_disco = String::from("format=rgba");
    if let Some(caratula) = &o.caratula {
        if !caratula.is_file() {
            return Err(format!("No existe la imagen {}", caratula.display()));
        }
        let (r, agujero) = (etiqueta as f64 / 2.0, lado as f64 / 2.0 * (imagen::R_AGUJERO + 0.008));
        let grafo = format!(
            "[1:v]scale={etiqueta}:{etiqueta}:force_original_aspect_ratio=increase,crop={etiqueta}:{etiqueta},format=rgba,\
             geq=r='r(X,Y)':g='g(X,Y)':b='b(X,Y)':\
             a='255*clip({r}-hypot(X-{r},Y-{r}),0,1)*clip(hypot(X-{r},Y-{r})-{agujero},0,1)'[c];\
             [0:v]format=rgba[d];[d][c]overlay=(W-w)/2:(H-h)/2:format=auto"
        );
        let mut args: Vec<String> = ["-i", "disco.png", "-i"].map(String::from).to_vec();
        args.push(ruta(&absoluta(caratula)?));
        args.extend(["-filter_complex", &grafo, "-frames:v", "1", "-update", "1", "disco2.png"].map(String::from));
        ffmpeg(ff, tmp, &args).map_err(|e| format!("No se pudo usar la carátula. {e}"))?;
        disco_listo = "disco2.png".into();
    } else {
        let lum = 0.299 * o.color.0 as f64 + 0.587 * o.color.1 as f64 + 0.114 * o.color.2 as f64;
        let tinta = if lum > 150.0 { "0x141414" } else { "white" };
        let t = recortar(v.titulo, 22);
        let ct = cuerpo(etiqueta * 4 / 5, t.chars().count(), 12.0, lado as f64 * 0.040);
        let y = format!("h/2-{}-text_h/2", lado as f64 * 0.085);
        filtro_disco += &format!(",{}", texto("e_titulo.txt", &t, tinta, ct, "(w-text_w)/2", &y)?);
        if !v.artista.is_empty() {
            let a = recortar(v.artista, 26);
            let ca = cuerpo(etiqueta * 4 / 5, a.chars().count(), 10.0, lado as f64 * 0.028);
            let y = format!("h/2+{}-text_h/2", lado as f64 * 0.085);
            filtro_disco += &format!(",{}", texto("e_artista.txt", &a, tinta, ca, "(w-text_w)/2", &y)?);
        }
    }

    // Textos principales.
    let titulo = recortar(v.titulo, 44);
    let ct = cuerpo(d.texto_ancho, titulo.chars().count(), 34.0, 84.0);
    let mut textos = texto("titulo.txt", &titulo, "white", ct, &format!("{}-text_w/2", d.texto_cx), &d.titulo_y.to_string())?;
    if !v.artista.is_empty() {
        let artista = recortar(v.artista, 50);
        let ca = cuerpo(d.texto_ancho, artista.chars().count(), 24.0, ct as f64 * 0.6);
        let y = d.titulo_y + ct as usize + 34;
        textos += &format!(",{}", texto("artista.txt", &artista, "white@0.7", ca, &format!("{}-text_w/2", d.texto_cx), &y.to_string())?);
    }

    // 33⅓ rpm = 3,4907 rad/s.
    let (ox, oy, ow, oh) = d.ondas;
    let grafo = format!(
        "[1:v]{filtro_disco},rotate=a=3.4907*t:c=black@0:ow=iw:oh=ih[disco];\
         [2:v][disco]overlay={}:{}:format=auto[b1];\
         [0:a]showwaves=s={ow}x{oh}:mode=cline:rate=30:colors=white@0.55,format=rgba[ondas];\
         [b1][ondas]overlay={ox}:{oy}:format=auto[b2];\
         [b2]{textos},format=yuv420p[v]",
        d.disco_x, d.disco_y
    );

    let mut args = vec!["-i".to_string(), ruta(v.audio)];
    args.extend(["-loop", "1", "-framerate", "30", "-i", &disco_listo].map(String::from));
    args.extend(["-loop", "1", "-framerate", "30", "-i", "fondo.png"].map(String::from));
    args.extend(["-filter_complex", &grafo, "-map", "[v]", "-map", "0:a:0"].map(String::from));
    args.extend(["-c:v", "libx264", "-preset", "medium", "-crf", "18", "-r", "30"].map(String::from));
    args.extend(["-c:a", "aac", "-b:a", "320k", "-ar", "48000", "-movflags", "+faststart"].map(String::from));
    args.extend(["-metadata".to_string(), format!("title={}", v.titulo)]);
    if !v.artista.is_empty() {
        args.extend(["-metadata".to_string(), format!("artist={}", v.artista)]);
    }
    match v.duracion {
        Some(t) => args.extend(["-t".to_string(), format!("{t:.3}")]),
        None => args.push("-shortest".into()),
    }
    args.push(ruta(v.destino));
    ffmpeg(ff, tmp, &args)?;
    Ok(())
}

// ───────────────────────── Programa ─────────────────────────

fn ruta(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn absoluta(p: &Path) -> Res<PathBuf> {
    std::path::absolute(p).map_err(|e| format!("Ruta no válida {}: {e}", p.display()))
}

fn objetivos(o: &Opciones) -> Res<Vec<Objetivo>> {
    let mut lista: Vec<Objetivo> = Vec::new();
    for id in &o.plataformas {
        let elegidas: Vec<&Plataforma> = match id.as_str() {
            "" => continue,
            "todas" | "all" => PLATAFORMAS.iter().collect(),
            "applemusic" | "apple-music" => PLATAFORMAS.iter().filter(|p| p.id == "apple").collect(),
            otro => {
                let p = PLATAFORMAS.iter().find(|p| p.id == otro).ok_or_else(|| {
                    format!("Plataforma desconocida «{otro}». Usa --listar para ver las disponibles.")
                })?;
                vec![p]
            }
        };
        for p in elegidas {
            if !lista.iter().any(|x| x.id == p.id) {
                lista.push(Objetivo { id: p.id.into(), nombre: p.nombre.into(), lufs: p.lufs, pico: p.pico });
            }
        }
    }
    if o.lufs.is_some() || (o.pico.is_some() && lista.is_empty()) {
        lista.push(Objetivo {
            id: "personalizado".into(),
            nombre: "Personalizado".into(),
            lufs: o.lufs.unwrap_or(-14.0),
            pico: o.pico.unwrap_or(-1.0),
        });
    } else if let Some(pico) = o.pico {
        lista.iter_mut().for_each(|x| x.pico = pico);
    }
    for x in &lista {
        if !(-70.0..=-5.0).contains(&x.lufs) || !(-9.0..=0.0).contains(&x.pico) {
            return Err("Valores fuera de rango: --lufs admite de -70 a -5 y --pico de -9 a 0.".into());
        }
    }
    if lista.is_empty() {
        return Err("No se ha indicado ninguna plataforma.".into());
    }
    Ok(lista)
}

struct Temporal(PathBuf);
impl Drop for Temporal {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn ejecutar(o: Opciones) -> Res<()> {
    let ff = localizar_ffmpeg()?;
    let entrada = absoluta(&o.entrada)?;
    if !entrada.is_file() {
        return Err(format!("No existe el archivo {}", entrada.display()));
    }
    codec_audio(&o.formato)?;
    diseno(&o.video_formato)?;
    let lista = objetivos(&o)?;

    let dir = match &o.dir {
        Some(d) => absoluta(d)?,
        None => entrada.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")),
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("No se pudo crear la carpeta {}: {e}", dir.display()))?;
    let tmp = Temporal(std::env::temp_dir().join(format!("musika_{}", std::process::id())));
    std::fs::create_dir_all(&tmp.0).map_err(|e| format!("No se pudo crear la carpeta temporal: {e}"))?;

    let info = leer_info(&ff, &entrada);
    let base = entrada.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "cancion".into());
    let titulo = o.titulo.clone().or(info.titulo.clone()).unwrap_or_else(|| base.clone());
    let artista = o.artista.clone().or(info.artista.clone()).unwrap_or_default();
    let frecuencia = if info.frecuencia == Some(44100) { 44100 } else { 48000 };
    let mut etiquetas: Vec<(&str, &str)> = vec![("title", &titulo)];
    if !artista.is_empty() {
        etiquetas.push(("artist", &artista));
    }

    println!("Canción: {titulo}{}", if artista.is_empty() { String::new() } else { format!(" — {artista}") });

    // Las plataformas con el mismo objetivo comparten el análisis.
    let mut medidas: HashMap<String, Medida> = HashMap::new();
    let mut medir_una = |ob: &Objetivo| -> Res<Medida> {
        let clave = format!("{}/{}", ob.lufs, ob.pico);
        if let Some(m) = medidas.get(&clave) {
            return Ok(m.clone());
        }
        let primera = medidas.is_empty();
        if primera {
            println!("Analizando la sonoridad…");
        }
        let m = medir(&ff, &tmp.0, &entrada, ob)?;
        if primera {
            println!("  Original: {:.1} LUFS, pico {:.1} dBTP, rango {:.1} LU\n", m.i, m.tp, m.lra);
        }
        medidas.insert(clave, m.clone());
        Ok(m)
    };
    let informe = |ob: &Objetivo, r: &Resultado, destino: &Path| {
        println!(
            "  ✔ {:<13} {:.1} LUFS, pico {:.1} dBTP ({})  →  {}",
            ob.nombre,
            r.lufs,
            r.pico,
            if r.lineal { "ganancia lineal" } else { "ajuste dinámico" },
            destino.display()
        );
    };

    let mut dinamico = false;
    if o.salida != Salida::Video {
        println!("Preparando el audio:");
        for ob in &lista {
            let m = medir_una(ob)?;
            let destino = dir.join(format!("{base}_{}.{}", ob.id, o.formato));
            if destino == entrada {
                return Err("El archivo de salida coincidiría con el original; elige otra carpeta con --dir.".into());
            }
            let r = masterizar(&ff, &tmp.0, &entrada, &destino, ob, &m, &o.formato, frecuencia, &etiquetas)?;
            dinamico |= !r.lineal;
            informe(ob, &r, &destino);
        }
    }

    if o.salida != Salida::Audio {
        // El vídeo lleva el ajuste de YouTube si se ha pedido; si no, el de la primera plataforma.
        let ob = lista.iter().find(|x| x.id == "youtube").unwrap_or(&lista[0]);
        let m = medir_una(ob)?;
        println!("Preparando el vídeo (audio con el ajuste de {}):", ob.nombre);
        let master = tmp.0.join("master.wav");
        let r = masterizar(&ff, &tmp.0, &entrada, &master, ob, &m, "wav", 48000, &[])?;
        dinamico |= !r.lineal;
        let destino = dir.join(format!("{base}_video.mp4"));
        println!("  Dibujando el disco y codificando; puede tardar unos minutos…");
        let v = Video { audio: &master, destino: &destino, titulo: &titulo, artista: &artista, duracion: info.duracion };
        crear_video(&ff, &tmp.0, &o, &v)?;
        informe(ob, &r, &destino);
    }

    if dinamico {
        println!(
            "\nNota: «ajuste dinámico» indica que la canción no llegaba al nivel pedido sin superar\n\
             el pico máximo, así que se ha aplicado una ganancia variable en lugar de una fija."
        );
    }
    println!("\nListo.");
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let guiado = args.is_empty();
    let resultado = if guiado { modo_guiado().map(Some) } else { parsear_args(&args) }
        .and_then(|o| o.map_or(Ok(()), ejecutar));
    let codigo = match resultado {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    };
    if guiado {
        // Si se abrió con doble clic, que la ventana no se cierre sin poder leer.
        preguntar("\nPulsa Intro para salir…");
    }
    std::process::exit(codigo);
}
