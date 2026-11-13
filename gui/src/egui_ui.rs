//! Ventana para Windows (y otros sistemas sin GTK), dibujada con egui.

use eframe::egui;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

const FORMATOS: [&str; 4] = ["wav", "flac", "mp3", "m4a"];
const EXT_AUDIO: [&str; 8] = ["wav", "mp3", "flac", "m4a", "ogg", "aiff", "aif", "opus"];
const EXT_IMAGEN: [&str; 5] = ["jpg", "jpeg", "png", "webp", "bmp"];

#[derive(Clone, Copy, PartialEq)]
enum Salida {
    Audio,
    Video,
    Ambos,
}

#[derive(Clone, Copy, PartialEq)]
enum Campo {
    Cancion,
    Caratula,
    Carpeta,
}

enum Aviso {
    Linea(String),
    Fin(bool),
}

struct App {
    cancion: String,
    salida: Salida,
    ajustar: bool,
    formato: usize,
    forzar: bool,
    titulo: String,
    artista: String,
    ondas: bool,
    chasis: bool,
    color: [u8; 3],
    caratula: String,
    rpm: f32,
    carpeta: String,

    registro: Vec<String>,
    trabajo: Option<Receiver<Aviso>>,
    inicio: Option<Instant>,
    resultado: Option<bool>,
    dialogo: Option<(Campo, Receiver<Option<String>>)>,
}

impl Default for App {
    fn default() -> Self {
        App {
            cancion: String::new(),
            salida: Salida::Ambos,
            ajustar: true,
            formato: 0,
            forzar: false,
            titulo: String::new(),
            artista: String::new(),
            ondas: false,
            chasis: true,
            color: [0xD9, 0x48, 0x2B],
            caratula: String::new(),
            rpm: 8.0,
            carpeta: String::new(),
            registro: Vec::new(),
            trabajo: None,
            inicio: None,
            resultado: None,
            dialogo: None,
        }
    }
}

/// Ruta del programa de línea de comandos: junto a esta ventana o en el PATH.
fn programa() -> PathBuf {
    let nombre = format!("musika{}", std::env::consts::EXE_SUFFIX);
    if let Ok(yo) = std::env::current_exe() {
        if let Some(dir) = yo.parent() {
            let junto = dir.join(&nombre);
            if junto.is_file() {
                return junto;
            }
        }
    }
    PathBuf::from(nombre)
}

/// Evita que en Windows se abra una consola al lanzar un programa.
fn sin_consola(c: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    #[cfg(not(windows))]
    {
        let _ = c;
    }
}

fn extension(ruta: &str) -> String {
    Path::new(ruta).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Abre el diálogo del sistema para elegir un archivo o una carpeta.
#[cfg(windows)]
fn elegir(campo: Campo) -> Option<String> {
    let d = rfd::FileDialog::new();
    let ruta = match campo {
        Campo::Cancion => d.add_filter("Audio", &EXT_AUDIO).pick_file(),
        Campo::Caratula => d.add_filter("Imagen", &EXT_IMAGEN).pick_file(),
        Campo::Carpeta => d.pick_folder(),
    };
    ruta.map(|p| p.to_string_lossy().into_owned())
}

/// Abre el diálogo del sistema (zenity o, si no está, kdialog).
#[cfg(not(windows))]
fn elegir(campo: Campo) -> Option<String> {
    let patron = |exts: &[&str]| exts.iter().map(|e| format!("*.{e}")).collect::<Vec<_>>().join(" ");
    let (titulo, filtro) = match campo {
        Campo::Cancion => ("Elige la canción", Some(("Audio", patron(&EXT_AUDIO)))),
        Campo::Caratula => ("Elige la imagen", Some(("Imagen", patron(&EXT_IMAGEN)))),
        Campo::Carpeta => ("Elige la carpeta de salida", None),
    };
    let mut zenity = Command::new("zenity");
    zenity.arg("--file-selection").arg(format!("--title={titulo}"));
    let mut kdialog = Command::new("kdialog");
    match &filtro {
        Some((nombre, patron)) => {
            zenity.arg(format!("--file-filter={nombre} | {patron}")).arg("--file-filter=Todos | *");
            kdialog.args(["--getopenfilename", "."]).arg(format!("{patron} | {nombre}"));
        }
        None => {
            zenity.arg("--directory");
            kdialog.args(["--getexistingdirectory", "."]);
        }
    }
    for mut orden in [zenity, kdialog] {
        if let Ok(r) = orden.stderr(Stdio::null()).output() {
            // Si el programa existe, su respuesta vale: vacía es que se canceló.
            let ruta = String::from_utf8_lossy(&r.stdout).trim().to_string();
            return if r.status.success() && !ruta.is_empty() { Some(ruta) } else { None };
        }
    }
    None
}

fn abrir_carpeta(carpeta: &Path) {
    let orden = if cfg!(windows) { "explorer" } else { "xdg-open" };
    let _ = Command::new(orden).arg(carpeta).spawn();
}

impl App {
    fn carpeta_de_salida(&self) -> Option<PathBuf> {
        if !self.carpeta.trim().is_empty() {
            return Some(PathBuf::from(self.carpeta.trim()));
        }
        Path::new(self.cancion.trim()).parent().map(Path::to_path_buf)
    }

    fn argumentos(&self) -> Vec<String> {
        let mut a = vec![self.cancion.trim().to_string()];
        let mut par = |opcion: &str, valor: String| {
            a.push(opcion.to_string());
            a.push(valor);
        };
        par(
            "--salida",
            match self.salida {
                Salida::Audio => "audio",
                Salida::Video => "video",
                Salida::Ambos => "ambos",
            }
            .into(),
        );
        if self.salida != Salida::Video {
            par("--formato", FORMATOS[self.formato].into());
        }
        if self.salida != Salida::Audio {
            par("--diseno", if self.ondas { "ondas" } else { "tocadiscos" }.into());
            par("--fondo", if self.chasis { "tocadiscos" } else { "liso" }.into());
            par("--color", format!("{:02X}{:02X}{:02X}", self.color[0], self.color[1], self.color[2]));
            par("--rpm", format!("{:.1}", self.rpm));
            if !self.caratula.trim().is_empty() {
                par("--caratula", self.caratula.trim().into());
            }
        }
        if !self.titulo.trim().is_empty() {
            par("--titulo", self.titulo.trim().into());
        }
        if !self.artista.trim().is_empty() {
            par("--artista", self.artista.trim().into());
        }
        if !self.carpeta.trim().is_empty() {
            par("--dir", self.carpeta.trim().into());
        }
        if !self.ajustar {
            a.push("--sin-ajuste".into());
        } else if self.forzar {
            a.push("--forzar-nivel".into());
        }
        a
    }

    /// Motivo por el que todavía no se puede empezar, si lo hay.
    fn falta(&self) -> Option<&'static str> {
        if self.cancion.trim().is_empty() {
            return Some("Elige una canción.");
        }
        if !Path::new(self.cancion.trim()).is_file() {
            return Some("No se encuentra el archivo de la canción.");
        }
        None
    }

    fn empezar(&mut self, ctx: &egui::Context) {
        let (tx, rx) = channel();
        self.registro.clear();
        self.resultado = None;
        self.inicio = Some(Instant::now());
        self.trabajo = Some(rx);
        let args = self.argumentos();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let enviar = |tx: &Sender<Aviso>, a: Aviso| {
                let _ = tx.send(a);
                ctx.request_repaint();
            };
            let mut orden = Command::new(programa());
            orden.args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
            sin_consola(&mut orden);
            let mut hijo = match orden.spawn() {
                Ok(h) => h,
                Err(e) => {
                    enviar(&tx, Aviso::Linea(format!("No se pudo ejecutar musika: {e}")));
                    enviar(&tx, Aviso::Fin(false));
                    return;
                }
            };
            // Los errores llegan por un hilo aparte para no bloquear la salida normal.
            let errores = hijo.stderr.take().map(|err| {
                let (tx, ctx) = (tx.clone(), ctx.clone());
                std::thread::spawn(move || {
                    for linea in BufReader::new(err).lines().map_while(Result::ok) {
                        let _ = tx.send(Aviso::Linea(linea));
                        ctx.request_repaint();
                    }
                })
            });
            if let Some(out) = hijo.stdout.take() {
                for linea in BufReader::new(out).lines().map_while(Result::ok) {
                    enviar(&tx, Aviso::Linea(linea));
                }
            }
            if let Some(h) = errores {
                let _ = h.join();
            }
            let bien = hijo.wait().map(|s| s.success()).unwrap_or(false);
            enviar(&tx, Aviso::Fin(bien));
        });
    }

    fn pedir(&mut self, campo: Campo, ctx: &egui::Context) {
        if self.dialogo.is_some() {
            return;
        }
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(elegir(campo));
            ctx.request_repaint();
        });
        self.dialogo = Some((campo, rx));
    }

    fn poner(&mut self, campo: Campo, ruta: String) {
        match campo {
            Campo::Cancion => self.cancion = ruta,
            Campo::Caratula => self.caratula = ruta,
            Campo::Carpeta => self.carpeta = ruta,
        }
    }

    /// Recoge lo que haya llegado de los hilos y los archivos soltados sobre la ventana.
    fn atender(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.trabajo {
            let mut fin = None;
            for aviso in rx.try_iter() {
                match aviso {
                    Aviso::Linea(l) => self.registro.push(l),
                    Aviso::Fin(bien) => fin = Some(bien),
                }
            }
            if let Some(bien) = fin {
                self.trabajo = None;
                self.resultado = Some(bien);
            }
        }
        let respuesta = self.dialogo.as_ref().and_then(|(campo, rx)| rx.try_recv().ok().map(|r| (*campo, r)));
        if let Some((campo, ruta)) = respuesta {
            self.dialogo = None;
            if let Some(ruta) = ruta {
                self.poner(campo, ruta);
            }
        }
        let soltados: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for ruta in soltados {
            let texto = ruta.to_string_lossy().into_owned();
            if EXT_IMAGEN.contains(&extension(&texto).as_str()) {
                self.caratula = texto;
            } else {
                self.cancion = texto;
            }
        }
    }

    fn fila_archivo(&mut self, ui: &mut egui::Ui, etiqueta: &str, campo: Campo, pista: &str) {
        ui.label(etiqueta);
        ui.horizontal(|ui| {
            let texto = match campo {
                Campo::Cancion => &mut self.cancion,
                Campo::Caratula => &mut self.caratula,
                Campo::Carpeta => &mut self.carpeta,
            };
            ui.add(egui::TextEdit::singleline(texto).desired_width(380.0).hint_text(pista));
            if ui.button("Examinar…").clicked() {
                let ctx = ui.ctx().clone();
                self.pedir(campo, &ctx);
            }
        });
        ui.end_row();
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.atender(ctx);
        let ocupado = self.trabajo.is_some();
        if ocupado {
            ctx.request_repaint_after(Duration::from_millis(500));
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("musika");
                ui.label("Prepara una canción para las plataformas y crea un vídeo de un disco girando.");
                ui.add_space(10.0);

                ui.add_enabled_ui(!ocupado, |ui| {
                    egui::Grid::new("cancion").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                        self.fila_archivo(ui, "Canción", Campo::Cancion, "Arrastra aquí un MP3 o WAV, o pulsa Examinar");
                        ui.label("Quiero");
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut self.salida, Salida::Audio, "Audio");
                            ui.radio_value(&mut self.salida, Salida::Video, "Vídeo");
                            ui.radio_value(&mut self.salida, Salida::Ambos, "Las dos cosas");
                        });
                        ui.end_row();
                        ui.label("Título");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.titulo)
                                .desired_width(380.0)
                                .hint_text("Vacío: el del archivo"),
                        );
                        ui.end_row();
                        ui.label("Artista");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.artista)
                                .desired_width(380.0)
                                .hint_text("Vacío: el del archivo"),
                        );
                        ui.end_row();
                        self.fila_archivo(ui, "Guardar en", Campo::Carpeta, "Vacío: junto a la canción");
                    });

                    ui.add_space(10.0);
                    ui.separator();
                    ui.strong("Audio");
                    ui.add_space(4.0);
                    ui.checkbox(&mut self.ajustar, "Ajustar el volumen para Spotify (−14 LUFS, pico −1 dB)");
                    ui.add_enabled(
                        self.ajustar,
                        egui::Checkbox::new(&mut self.forzar, "Llegar siempre a ese nivel, aunque haya que comprimir"),
                    );
                    if self.salida != Salida::Video {
                        ui.horizontal(|ui| {
                            ui.label("Formato del archivo");
                            for (i, f) in FORMATOS.iter().enumerate() {
                                ui.radio_value(&mut self.formato, i, f.to_uppercase());
                            }
                        });
                    }

                    if self.salida != Salida::Audio {
                        ui.add_space(10.0);
                        ui.separator();
                        ui.strong("Vídeo (para YouTube)");
                        ui.add_space(4.0);
                        egui::Grid::new("video").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                            ui.label("Diseño");
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut self.ondas, false, "Tocadiscos");
                                ui.radio_value(&mut self.ondas, true, "Disco con ondas de colores");
                            });
                            ui.end_row();
                            if !self.ondas {
                                ui.label("");
                                ui.checkbox(&mut self.chasis, "Con el chasis de aluminio");
                                ui.end_row();
                            }
                            ui.label("Color");
                            ui.color_edit_button_srgb(&mut self.color);
                            ui.end_row();
                            ui.label("Giro del disco");
                            ui.add(egui::Slider::new(&mut self.rpm, 0.0..=45.0).suffix(" rpm"));
                            ui.end_row();
                            self.fila_archivo(ui, "Carátula", Campo::Caratula, "Opcional: imagen para la etiqueta del disco");
                        });
                    }
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let falta = self.falta();
                    let boton = egui::Button::new(egui::RichText::new("Crear").strong().size(16.0));
                    if ui.add_enabled(!ocupado && falta.is_none(), boton).clicked() {
                        self.empezar(ctx);
                    }
                    if ocupado {
                        ui.spinner();
                        let s = self.inicio.map(|t| t.elapsed().as_secs()).unwrap_or(0);
                        ui.label(format!("Trabajando… {:02}:{:02}", s / 60, s % 60));
                    } else if let Some(motivo) = falta {
                        ui.label(motivo);
                    } else if let Some(bien) = self.resultado {
                        if bien {
                            ui.colored_label(egui::Color32::from_rgb(110, 200, 120), "Hecho.");
                            if let Some(carpeta) = self.carpeta_de_salida() {
                                if ui.button("Abrir la carpeta").clicked() {
                                    abrir_carpeta(&carpeta);
                                }
                            }
                        } else {
                            ui.colored_label(egui::Color32::from_rgb(235, 110, 100), "Ha fallado; mira el detalle abajo.");
                        }
                    }
                });

                if !self.registro.is_empty() {
                    ui.add_space(8.0);
                    egui::ScrollArea::vertical().id_salt("registro").max_height(220.0).stick_to_bottom(true).show(
                        ui,
                        |ui| {
                            for linea in &self.registro {
                                ui.monospace(linea);
                            }
                        },
                    );
                }
            });
        });
    }
}

pub fn ejecutar() -> Result<(), eframe::Error> {
    const ICONO: usize = 128;
    let icono = egui::IconData {
        rgba: crate::imagen::dibujar_disco(ICONO, (0xD9, 0x48, 0x2B)),
        width: ICONO as u32,
        height: ICONO as u32,
    };
    let opciones = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("musika")
            .with_app_id("musika")
            .with_inner_size([640.0, 780.0])
            .with_min_inner_size([520.0, 420.0])
            .with_icon(icono),
        ..Default::default()
    };
    eframe::run_native("musika", opciones, Box::new(|_cc| Ok(Box::new(App::default()))))
}
