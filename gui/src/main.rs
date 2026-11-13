//! musika-gui — ventana de escritorio para musika.
//!
//! No hace el trabajo ella misma: recoge las opciones en un formulario y
//! lanza el programa de línea de comandos `musika`, mostrando lo que escribe.
//!
//! En Linux la ventana es GTK 4, con los controles propios del escritorio.
//! En los demás sistemas se dibuja con egui.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
mod gtk_ui;

#[cfg(not(target_os = "linux"))]
mod egui_ui;
#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
#[path = "../../src/imagen.rs"]
mod imagen;

#[cfg(target_os = "linux")]
fn main() {
    gtk_ui::ejecutar();
}

#[cfg(not(target_os = "linux"))]
fn main() -> Result<(), eframe::Error> {
    egui_ui::ejecutar()
}
