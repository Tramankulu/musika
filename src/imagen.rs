//! Dibujo del disco y del fondo, y escritura de PNG sin dependencias externas.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub type Rgb = (u8, u8, u8);

fn crc32(tabla: &[u32; 256], crc: u32, datos: &[u8]) -> u32 {
    let mut c = crc;
    for &b in datos {
        c = tabla[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c
}

fn tabla_crc() -> [u32; 256] {
    let mut t = [0u32; 256];
    for (n, e) in t.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *e = c;
    }
    t
}

fn trozo<W: Write>(w: &mut W, t: &[u32; 256], tipo: &[u8; 4], datos: &[u8]) -> std::io::Result<()> {
    w.write_all(&(datos.len() as u32).to_be_bytes())?;
    w.write_all(tipo)?;
    w.write_all(datos)?;
    let crc = crc32(t, crc32(t, 0xFFFF_FFFF, tipo), datos) ^ 0xFFFF_FFFF;
    w.write_all(&crc.to_be_bytes())
}

/// Escribe un PNG RGBA de 8 bits (deflate sin comprimir: simple y suficiente
/// para un archivo temporal).
pub fn escribir_png(ruta: &Path, ancho: usize, alto: usize, rgba: &[u8]) -> std::io::Result<()> {
    assert_eq!(rgba.len(), ancho * alto * 4);
    let t = tabla_crc();
    let mut w = BufWriter::new(File::create(ruta)?);
    w.write_all(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])?;

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(ancho as u32).to_be_bytes());
    ihdr.extend_from_slice(&(alto as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    trozo(&mut w, &t, b"IHDR", &ihdr)?;

    // Datos crudos: cada fila precedida del byte de filtro 0.
    let fila = ancho * 4;
    let mut crudo = Vec::with_capacity((fila + 1) * alto);
    for y in 0..alto {
        crudo.push(0);
        crudo.extend_from_slice(&rgba[y * fila..(y + 1) * fila]);
    }

    // zlib con bloques "stored".
    let mut z = Vec::with_capacity(crudo.len() + crudo.len() / 65535 * 5 + 16);
    z.extend_from_slice(&[0x78, 0x01]);
    let mut bloques = crudo.chunks(65535).peekable();
    while let Some(b) = bloques.next() {
        z.push(if bloques.peek().is_none() { 1 } else { 0 });
        let n = b.len() as u16;
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for parte in crudo.chunks(5552) {
        for &x in parte {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());

    trozo(&mut w, &t, b"IDAT", &z)?;
    trozo(&mut w, &t, b"IEND", &[])?;
    w.flush()
}

/// Radio de la etiqueta central respecto al radio del disco.
pub const R_ETIQUETA: f64 = 0.36;
/// Radio del agujero central respecto al radio del disco.
pub const R_AGUJERO: f64 = 0.018;

fn mezcla(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Color (r,g,b,alfa) del disco en un punto. `r` normalizado 0..1, `ang` en radianes.
fn muestra_disco(r: f64, ang: f64, radio_px: f64, color: Rgb) -> [f64; 4] {
    if r > 1.0 || r < R_AGUJERO {
        return [0.0, 0.0, 0.0, 0.0];
    }
    // Reflejos: dos lóbulos opuestos fuertes y dos débiles; hacen visible el giro.
    let c = (ang - 0.9).cos();
    let s = (ang - 0.9).sin();
    let brillo = (c * c).powi(7) * 0.34 + (s * s).powi(12) * 0.10;

    if r > 0.985 {
        let v = 30.0 + 255.0 * brillo * 0.5;
        return [v, v, v + 2.0, 255.0];
    }
    if r > R_ETIQUETA + 0.035 {
        // Surcos.
        let g = 0.5 + 0.5 * (r * radio_px * 1.25).sin();
        let separador = [0.53, 0.69, 0.84].iter().any(|&p| (r - p).abs() < 0.0035);
        let (base, k) = if separador { (7.0, 1.25) } else { (13.0 + 9.0 * g, 0.55 + 0.45 * g) };
        let v = base + 255.0 * brillo * k;
        return [v, v, v * 1.03 + 1.0, 255.0];
    }
    if r > R_ETIQUETA {
        // Zona lisa entre los surcos y la etiqueta.
        let v = 10.0 + 255.0 * brillo * 0.8;
        return [v, v, v + 1.0, 255.0];
    }
    // Etiqueta.
    let mut luz = mezcla(1.06, 0.90, r / R_ETIQUETA) + brillo * 0.25;
    if (r - R_ETIQUETA * 0.93).abs() < 0.004 || r < R_AGUJERO + 0.008 {
        luz *= 0.72;
    }
    [color.0 as f64 * luz, color.1 as f64 * luz, color.2 as f64 * luz, 255.0]
}

/// Dibuja un disco de vinilo en un lienzo cuadrado RGBA de `lado` píxeles.
pub fn dibujar_disco(lado: usize, color: Rgb) -> Vec<u8> {
    const SS: usize = 3; // supermuestreo para suavizar bordes
    let radio = lado as f64 / 2.0 - 1.0;
    let centro = lado as f64 / 2.0;
    let mut px = vec![0u8; lado * lado * 4];
    for y in 0..lado {
        for x in 0..lado {
            let mut acc = [0.0f64; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let dx = x as f64 + (sx as f64 + 0.5) / SS as f64 - centro;
                    let dy = y as f64 + (sy as f64 + 0.5) / SS as f64 - centro;
                    let r = (dx * dx + dy * dy).sqrt() / radio;
                    let m = muestra_disco(r, dy.atan2(dx), radio, color);
                    let a = m[3] / 255.0;
                    acc[0] += m[0] * a;
                    acc[1] += m[1] * a;
                    acc[2] += m[2] * a;
                    acc[3] += a;
                }
            }
            let i = (y * lado + x) * 4;
            if acc[3] > 0.0 {
                for k in 0..3 {
                    px[i + k] = (acc[k] / acc[3]).clamp(0.0, 255.0) as u8;
                }
                px[i + 3] = (acc[3] / (SS * SS) as f64 * 255.0).round() as u8;
            }
        }
    }
    px
}

/// Fondo oscuro con un degradado teñido del color de la etiqueta y la sombra del disco.
pub fn dibujar_fondo(ancho: usize, alto: usize, color: Rgb, disco: (f64, f64, f64)) -> Vec<u8> {
    let (cx, cy, radio) = disco;
    let diag = ((ancho * ancho + alto * alto) as f64).sqrt();
    let mut px = vec![255u8; ancho * alto * 4];
    for y in 0..alto {
        for x in 0..ancho {
            let (fx, fy) = (x as f64, y as f64);
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let halo = (1.0 - d / (diag * 0.75)).clamp(0.0, 1.0).powi(2);
            // Sombra desplazada bajo el disco.
            let ds = ((fx - cx - radio * 0.03).powi(2) + (fy - cy - radio * 0.05).powi(2)).sqrt();
            let sombra = 1.0 - 0.6 * (1.0 - ((ds - radio) / (radio * 0.10)).clamp(0.0, 1.0));
            // Tramado mínimo para que el degradado no haga bandas al comprimir.
            let ruido = ((x * 7 + y * 13) % 5) as f64 * 0.3;
            let i = (y * ancho + x) * 4;
            let comp = [color.0, color.1, color.2];
            for k in 0..3 {
                let base = 12.0 + [0.0, 1.0, 4.0][k];
                let v = (base + comp[k] as f64 * 0.22 * halo) * sombra + ruido;
                px[i + k] = v.clamp(0.0, 255.0) as u8;
            }
        }
    }
    px
}
