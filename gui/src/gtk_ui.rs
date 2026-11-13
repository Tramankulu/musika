//! Ventana para Linux, hecha con GTK 4: usa los controles, los diálogos y el
//! tema del propio escritorio.

#![allow(deprecated)] // ColorButton y FileChooserNative siguen siendo lo más compatible con GTK 4.6

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::Cell;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc::{channel, Sender};
use std::time::{Duration, Instant};

const ID: &str = "io.github.tramankulu.musika";

const FORMATOS: [(&str, &str); 4] = [
    ("wav", "WAV (24 bits)"),
    ("flac", "FLAC"),
    ("mp3", "MP3 (320 kbps)"),
    ("m4a", "M4A (AAC 256 kbps)"),
];
const EXT_AUDIO: [&str; 8] = ["wav", "mp3", "flac", "m4a", "ogg", "aiff", "aif", "opus"];
const EXT_IMAGEN: [&str; 5] = ["jpg", "jpeg", "png", "webp", "bmp"];

enum Aviso {
    Linea(String),
    Fin(bool),
}

struct Ui {
    ventana: gtk::ApplicationWindow,
    formulario: gtk::Box,
    cancion: gtk::Entry,
    titulo: gtk::Entry,
    artista: gtk::Entry,
    carpeta: gtk::Entry,
    sal_audio: gtk::CheckButton,
    sal_video: gtk::CheckButton,
    ajustar: gtk::CheckButton,
    formato: gtk::DropDown,
    forzar: gtk::CheckButton,
    marco_video: gtk::Frame,
    dis_ondas: gtk::CheckButton,
    chasis: gtk::CheckButton,
    color: gtk::ColorButton,
    rpm: gtk::Scale,
    caratula: gtk::Entry,
    crear: gtk::Button,
    girando: gtk::Spinner,
    estado: gtk::Label,
    abrir: gtk::Button,
    registro: gtk::TextView,
    registro_caja: gtk::ScrolledWindow,
    ocupado: Cell<bool>,
}

/// Ruta del programa de línea de comandos: junto a esta ventana o en el PATH.
fn programa() -> PathBuf {
    if let Ok(yo) = std::env::current_exe() {
        if let Some(dir) = yo.parent() {
            let junto = dir.join("musika");
            if junto.is_file() {
                return junto;
            }
        }
    }
    PathBuf::from("musika")
}

fn texto(e: &gtk::Entry) -> String {
    e.text().trim().to_string()
}

fn etiqueta(t: &str) -> gtk::Label {
    gtk::Label::builder().label(t).xalign(0.0).build()
}

fn campo(pista: &str) -> gtk::Entry {
    gtk::Entry::builder().hexpand(true).placeholder_text(pista).build()
}

fn rejilla() -> gtk::Grid {
    gtk::Grid::builder()
        .row_spacing(8)
        .column_spacing(12)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(12)
        .margin_end(12)
        .build()
}

/// Añade a la rejilla una fila «etiqueta: control».
fn fila(g: &gtk::Grid, n: i32, t: &str, control: &impl IsA<gtk::Widget>) {
    g.attach(&etiqueta(t), 0, n, 1, 1);
    g.attach(control, 1, n, 1, 1);
}

/// Un campo de texto con su botón «Examinar…».
fn con_examinar(e: &gtk::Entry) -> (gtk::Box, gtk::Button) {
    let caja = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let boton = gtk::Button::with_label("Examinar…");
    caja.append(e);
    caja.append(&boton);
    (caja, boton)
}

impl Ui {
    fn con_audio(&self) -> bool {
        !self.sal_video.is_active()
    }

    fn con_video(&self) -> bool {
        !self.sal_audio.is_active()
    }

    fn carpeta_de_salida(&self) -> Option<PathBuf> {
        let elegida = texto(&self.carpeta);
        if !elegida.is_empty() {
            return Some(PathBuf::from(elegida));
        }
        Path::new(&texto(&self.cancion)).parent().map(Path::to_path_buf)
    }

    /// Motivo por el que todavía no se puede empezar, si lo hay.
    fn falta(&self) -> Option<&'static str> {
        let cancion = texto(&self.cancion);
        if cancion.is_empty() {
            return Some("Elige una canción.");
        }
        if !Path::new(&cancion).is_file() {
            return Some("No se encuentra el archivo de la canción.");
        }
        None
    }

    fn argumentos(&self) -> Vec<String> {
        let mut a = vec![texto(&self.cancion)];
        let mut par = |opcion: &str, valor: String| {
            a.push(opcion.to_string());
            a.push(valor);
        };
        let salida = match (self.con_audio(), self.con_video()) {
            (true, false) => "audio",
            (false, true) => "video",
            _ => "ambos",
        };
        par("--salida", salida.into());
        if self.con_audio() {
            par("--formato", FORMATOS[(self.formato.selected() as usize).min(FORMATOS.len() - 1)].0.into());
        }
        if self.con_video() {
            par("--diseno", if self.dis_ondas.is_active() { "ondas" } else { "tocadiscos" }.into());
            par("--fondo", if self.chasis.is_active() { "tocadiscos" } else { "liso" }.into());
            let c = self.color.rgba();
            let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            par("--color", format!("{:02X}{:02X}{:02X}", byte(c.red()), byte(c.green()), byte(c.blue())));
            par("--rpm", format!("{:.0}", self.rpm.value()));
            if !texto(&self.caratula).is_empty() {
                par("--caratula", texto(&self.caratula));
            }
        }
        for (opcion, entrada) in [("--titulo", &self.titulo), ("--artista", &self.artista), ("--dir", &self.carpeta)] {
            if !texto(entrada).is_empty() {
                par(opcion, texto(entrada));
            }
        }
        if !self.ajustar.is_active() {
            a.push("--sin-ajuste".into());
        } else if self.forzar.is_active() {
            a.push("--forzar-nivel".into());
        }
        a
    }

    fn anotar(&self, linea: &str) {
        let buf = self.registro.buffer();
        let mut fin = buf.end_iter();
        buf.insert(&mut fin, linea);
        buf.insert(&mut fin, "\n");
        let marca = buf.create_mark(None, &buf.end_iter(), false);
        self.registro.scroll_mark_onscreen(&marca);
        buf.delete_mark(&marca);
    }

    fn poner_ocupado(&self, ocupado: bool) {
        self.ocupado.set(ocupado);
        self.formulario.set_sensitive(!ocupado);
        self.crear.set_sensitive(!ocupado);
        self.girando.set_visible(ocupado);
        if ocupado {
            self.girando.start();
        } else {
            self.girando.stop();
        }
    }

    /// Muestra u oculta las secciones según lo que se haya pedido.
    fn ajustar_secciones(&self) {
        self.formato.set_sensitive(self.con_audio());
        self.forzar.set_sensitive(self.ajustar.is_active());
        self.marco_video.set_visible(self.con_video());
        self.chasis.set_sensitive(!self.dis_ondas.is_active());
    }
}

/// Diálogo del sistema para elegir un archivo o una carpeta; deja la ruta en `destino`.
fn elegir(ui: &Rc<Ui>, destino: &gtk::Entry, titulo: &str, filtro: Option<(&str, &[&str])>) {
    let accion = if filtro.is_some() { gtk::FileChooserAction::Open } else { gtk::FileChooserAction::SelectFolder };
    let d = gtk::FileChooserNative::new(Some(titulo), Some(&ui.ventana), accion, Some("Elegir"), Some("Cancelar"));
    d.set_modal(true);
    if let Some((nombre, exts)) = filtro {
        let f = gtk::FileFilter::new();
        f.set_name(Some(nombre));
        for e in exts {
            f.add_pattern(&format!("*.{e}"));
            f.add_pattern(&format!("*.{}", e.to_uppercase()));
        }
        d.add_filter(&f);
        let todos = gtk::FileFilter::new();
        todos.set_name(Some("Todos los archivos"));
        todos.add_pattern("*");
        d.add_filter(&todos);
    }
    let destino = destino.clone();
    // El diálogo nativo desaparece si nadie lo sujeta: lo guarda su propia respuesta.
    let vivo = d.clone();
    d.connect_response(move |d, respuesta| {
        let _ = &vivo;
        if respuesta == gtk::ResponseType::Accept {
            if let Some(ruta) = d.file().and_then(|f| f.path()) {
                destino.set_text(&ruta.to_string_lossy());
            }
        }
        d.destroy();
    });
    d.show();
}

/// Lanza musika en otro hilo y va volcando en la ventana lo que escribe.
fn empezar(ui: &Rc<Ui>) {
    if ui.ocupado.get() {
        return;
    }
    if let Some(motivo) = ui.falta() {
        ui.estado.set_text(motivo);
        return;
    }
    let args = ui.argumentos();
    ui.registro.buffer().set_text("");
    ui.registro_caja.set_visible(true);
    ui.abrir.set_visible(false);
    ui.estado.set_text("Trabajando…");
    ui.poner_ocupado(true);

    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut orden = Command::new(programa());
        orden.args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut hijo = match orden.spawn() {
            Ok(h) => h,
            Err(e) => {
                let _ = tx.send(Aviso::Linea(format!("No se pudo ejecutar musika: {e}")));
                let _ = tx.send(Aviso::Fin(false));
                return;
            }
        };
        // Los errores llegan por un hilo aparte para no bloquear la salida normal.
        let errores = hijo.stderr.take().map(|err| {
            let tx: Sender<Aviso> = tx.clone();
            std::thread::spawn(move || {
                for linea in BufReader::new(err).lines().map_while(Result::ok) {
                    let _ = tx.send(Aviso::Linea(linea));
                }
            })
        });
        if let Some(out) = hijo.stdout.take() {
            for linea in BufReader::new(out).lines().map_while(Result::ok) {
                let _ = tx.send(Aviso::Linea(linea));
            }
        }
        if let Some(h) = errores {
            let _ = h.join();
        }
        let bien = hijo.wait().map(|s| s.success()).unwrap_or(false);
        let _ = tx.send(Aviso::Fin(bien));
    });

    let ui = ui.clone();
    let inicio = Instant::now();
    glib::timeout_add_local(Duration::from_millis(150), move || {
        for aviso in rx.try_iter() {
            match aviso {
                Aviso::Linea(l) => ui.anotar(&l),
                Aviso::Fin(bien) => {
                    ui.poner_ocupado(false);
                    ui.estado.set_text(if bien { "Hecho." } else { "Ha fallado; mira el detalle abajo." });
                    ui.abrir.set_visible(bien);
                    return glib::ControlFlow::Break;
                }
            }
        }
        let s = inicio.elapsed().as_secs();
        ui.estado.set_text(&format!("Trabajando… {:02}:{:02}", s / 60, s % 60));
        glib::ControlFlow::Continue
    });
}

fn construir(app: &gtk::Application) {
    let ventana = gtk::ApplicationWindow::builder()
        .application(app)
        .title("musika")
        .icon_name("musika")
        .default_width(640)
        .default_height(760)
        .build();

    // ── Canción y datos generales ──
    let cancion = campo("Arrastra aquí un MP3 o WAV, o pulsa Examinar");
    let titulo = campo("Vacío: el del archivo");
    let artista = campo("Vacío: el del archivo");
    let carpeta = campo("Vacío: junto a la canción");
    let (caja_cancion, examinar_cancion) = con_examinar(&cancion);
    let (caja_carpeta, examinar_carpeta) = con_examinar(&carpeta);

    let sal_audio = gtk::CheckButton::with_label("Audio");
    let sal_video = gtk::CheckButton::with_label("Vídeo");
    let sal_ambos = gtk::CheckButton::with_label("Las dos cosas");
    sal_video.set_group(Some(&sal_audio));
    sal_ambos.set_group(Some(&sal_audio));
    sal_ambos.set_active(true);
    let caja_salida = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    caja_salida.append(&sal_audio);
    caja_salida.append(&sal_video);
    caja_salida.append(&sal_ambos);

    let general = rejilla();
    general.set_margin_start(0);
    general.set_margin_end(0);
    fila(&general, 0, "Canción", &caja_cancion);
    fila(&general, 1, "Quiero", &caja_salida);
    fila(&general, 2, "Título", &titulo);
    fila(&general, 3, "Artista", &artista);
    fila(&general, 4, "Guardar en", &caja_carpeta);

    // ── Audio ──
    let ajustar = gtk::CheckButton::with_label("Ajustar el volumen para Spotify (−14 LUFS, pico −1 dB)");
    ajustar.set_active(true);
    let forzar = gtk::CheckButton::with_label("Llegar siempre a ese nivel, aunque haya que comprimir");
    forzar.set_margin_start(24);
    let nombres: Vec<&str> = FORMATOS.iter().map(|f| f.1).collect();
    let formato = gtk::DropDown::from_strings(&nombres);
    formato.set_halign(gtk::Align::Start);
    let audio = rejilla();
    audio.attach(&ajustar, 0, 0, 2, 1);
    audio.attach(&forzar, 0, 1, 2, 1);
    fila(&audio, 2, "Formato del archivo", &formato);
    let marco_audio = gtk::Frame::new(Some("Audio"));
    marco_audio.set_child(Some(&audio));

    // ── Vídeo ──
    let dis_tocadiscos = gtk::CheckButton::with_label("Tocadiscos");
    let dis_ondas = gtk::CheckButton::with_label("Disco con ondas de colores");
    dis_ondas.set_group(Some(&dis_tocadiscos));
    dis_tocadiscos.set_active(true);
    let caja_diseno = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    caja_diseno.append(&dis_tocadiscos);
    caja_diseno.append(&dis_ondas);
    let chasis = gtk::CheckButton::with_label("Con el chasis de aluminio");
    chasis.set_active(true);
    let color = gtk::ColorButton::with_rgba(&gdk::RGBA::new(0.851, 0.282, 0.169, 1.0));
    color.set_halign(gtk::Align::Start);
    color.set_title("Color de la etiqueta del disco");
    let rpm = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 45.0, 1.0);
    rpm.set_value(8.0);
    rpm.set_digits(0);
    rpm.set_draw_value(true);
    rpm.set_value_pos(gtk::PositionType::Right);
    rpm.set_hexpand(true);
    let caratula = campo("Opcional: imagen para la etiqueta del disco");
    let (caja_caratula, examinar_caratula) = con_examinar(&caratula);
    let video = rejilla();
    fila(&video, 0, "Diseño", &caja_diseno);
    video.attach(&chasis, 1, 1, 1, 1);
    fila(&video, 2, "Color", &color);
    fila(&video, 3, "Giro (rpm)", &rpm);
    fila(&video, 4, "Carátula", &caja_caratula);
    let marco_video = gtk::Frame::new(Some("Vídeo para YouTube (1920×1080)"));
    marco_video.set_child(Some(&video));

    let formulario = gtk::Box::new(gtk::Orientation::Vertical, 12);
    formulario.append(&general);
    formulario.append(&marco_audio);
    formulario.append(&marco_video);

    // ── Acciones y registro ──
    let crear = gtk::Button::with_label("Crear");
    crear.add_css_class("suggested-action");
    let girando = gtk::Spinner::new();
    girando.set_visible(false);
    let estado = etiqueta("");
    estado.set_hexpand(true);
    estado.set_wrap(true);
    let abrir = gtk::Button::with_label("Abrir la carpeta");
    abrir.set_visible(false);
    let acciones = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    acciones.append(&crear);
    acciones.append(&girando);
    acciones.append(&estado);
    acciones.append(&abrir);

    let registro = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(6)
        .bottom_margin(6)
        .left_margin(8)
        .right_margin(8)
        .build();
    let registro_caja = gtk::ScrolledWindow::builder()
        .child(&registro)
        .min_content_height(170)
        .vexpand(true)
        .has_frame(true)
        .visible(false)
        .build();

    let contenido = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(18)
        .margin_end(18)
        .build();
    contenido.append(&formulario);
    contenido.append(&acciones);
    contenido.append(&registro_caja);
    let desplazable = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).child(&contenido).build();
    ventana.set_child(Some(&desplazable));

    let ui = Rc::new(Ui {
        ventana: ventana.clone(),
        formulario,
        cancion,
        titulo,
        artista,
        carpeta,
        sal_audio,
        sal_video,
        ajustar,
        formato,
        forzar,
        marco_video,
        dis_ondas,
        chasis,
        color,
        rpm,
        caratula,
        crear,
        girando,
        estado,
        abrir,
        registro,
        registro_caja,
        ocupado: Cell::new(false),
    });

    // ── Comportamiento ──
    for boton in [&ui.sal_audio, &ui.sal_video, &sal_ambos, &ui.dis_ondas, &ui.ajustar] {
        let ui = ui.clone();
        boton.connect_toggled(move |_| ui.ajustar_secciones());
    }
    {
        let ui = ui.clone();
        examinar_cancion.connect_clicked(move |_| elegir(&ui, &ui.cancion, "Elige la canción", Some(("Audio", &EXT_AUDIO))));
    }
    {
        let ui = ui.clone();
        examinar_caratula
            .connect_clicked(move |_| elegir(&ui, &ui.caratula, "Elige la imagen", Some(("Imágenes", &EXT_IMAGEN))));
    }
    {
        let ui = ui.clone();
        examinar_carpeta.connect_clicked(move |_| elegir(&ui, &ui.carpeta, "Elige la carpeta de salida", None));
    }
    {
        let ui = ui.clone();
        ui.crear.clone().connect_clicked(move |_| empezar(&ui));
    }
    {
        let ui = ui.clone();
        ui.abrir.clone().connect_clicked(move |_| {
            if let Some(carpeta) = ui.carpeta_de_salida() {
                let _ = Command::new("xdg-open").arg(carpeta).spawn();
            }
        });
    }
    // Soltar archivos sobre la ventana: una imagen va a la carátula; lo demás, a la canción.
    let soltar = gtk::DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
    {
        let ui = ui.clone();
        soltar.connect_drop(move |_, valor, _, _| {
            let Some(ruta) = valor.get::<gio::File>().ok().and_then(|f| f.path()) else { return false };
            let ext = ruta.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            let destino = if EXT_IMAGEN.contains(&ext.as_str()) { &ui.caratula } else { &ui.cancion };
            destino.set_text(&ruta.to_string_lossy());
            true
        });
    }
    ventana.add_controller(soltar);

    ui.ajustar_secciones();
    ventana.present();
}

pub fn ejecutar() {
    let app = gtk::Application::builder().application_id(ID).build();
    app.connect_activate(construir);
    let _ = app.run();
}
