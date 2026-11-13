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
/// Radio del plato (lo que asoma bajo el disco) respecto al radio del disco.
pub const R_PLATO: f64 = 1.04;

use std::f64::consts::PI;

fn mezcla(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Valor pseudoaleatorio estable en 0..1.
fn azar(n: f64) -> f64 {
    let v = (n * 12.9898).sin() * 43758.5453;
    v - v.floor()
}

/// Reflejo de la luz sobre los surcos: dos haces opuestos y otros dos más débiles.
fn reflejo(ang: f64) -> f64 {
    let lobulo = |centro: f64, ancho: f64| {
        let x = (ang - centro).rem_euclid(PI);
        let d = x.min(PI - x);
        (-(d / ancho).powi(2)).exp()
    };
    0.50 * lobulo(-0.75, 0.13) + 0.16 * lobulo(-0.75, 0.42) + 0.14 * lobulo(0.95, 0.10) + 0.05 * lobulo(0.95, 0.35)
}

/// Color (r,g,b,alfa) del disco en un punto. `r` normalizado 0..1, `ang` en radianes.
fn muestra_disco(r: f64, ang: f64, radio_px: f64, color: Rgb) -> [f64; 4] {
    if r > 1.0 || r < R_AGUJERO {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let brillo = reflejo(ang);
    let gris = |v: f64| [v, v, v * 1.05 + 1.0, 255.0];

    if r > 0.988 {
        return gris(34.0 + 255.0 * brillo * 0.55); // canto
    }
    if r > 0.955 {
        return gris(11.0 + 255.0 * brillo * 0.45); // borde liso de entrada
    }
    if r > R_ETIQUETA + 0.04 {
        // Surcos: cada uno refleja un poco distinto, de ahí el brillo estriado.
        let surco = azar((r * radio_px * 0.85).floor());
        let separador = [0.53, 0.69, 0.84].iter().any(|&p| (r - p).abs() < 0.0035);
        if separador {
            return gris(6.0 + 255.0 * brillo * 0.75);
        }
        return gris(8.0 + 9.0 * surco + 255.0 * brillo * (0.25 + 0.75 * surco));
    }
    if r > R_ETIQUETA {
        return gris(9.0 + 255.0 * brillo * 0.6); // zona lisa junto a la etiqueta
    }
    // Etiqueta de papel.
    let mut luz = mezcla(1.08, 0.88, r / R_ETIQUETA) + brillo * 0.18;
    if (r - R_ETIQUETA * 0.93).abs() < 0.0035 || (r - R_ETIQUETA * 0.22).abs() < 0.0025 {
        luz *= 0.74;
    }
    if r < R_AGUJERO + 0.007 {
        luz *= 0.6;
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

/// Fondo del vídeo, con el plato y la sombra bajo el disco.
///
/// Sin carátula es opaco: un degradado teñido del color de la etiqueta.
/// Con carátula es un velo negro semitransparente (viñeta y sombras) que se
/// pone encima de la carátula desenfocada.
pub fn dibujar_fondo(ancho: usize, alto: usize, color: Rgb, disco: (f64, f64, f64), velo: bool) -> Vec<u8> {
    let (cx, cy, radio) = disco;
    let diag = ((ancho * ancho + alto * alto) as f64).sqrt();
    let plato = radio * R_PLATO;
    let comp = [color.0 as f64, color.1 as f64, color.2 as f64];
    let mut px = vec![0u8; ancho * alto * 4];
    for y in 0..alto {
        for x in 0..ancho {
            let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            // Viñeta respecto al centro del lienzo.
            let dc = ((fx - ancho as f64 / 2.0).powi(2) + (fy - alto as f64 / 2.0).powi(2)).sqrt() / (diag / 2.0);
            let vineta = 1.0 - 0.55 * dc.powf(1.8);
            // Sombra suave y desplazada bajo el plato.
            let ds = ((fx - cx - radio * 0.04).powi(2) + (fy - cy - radio * 0.07).powi(2)).sqrt();
            let sombra = 1.0 - 0.7 * (1.0 - ((ds - plato) / (radio * 0.16)).clamp(0.0, 1.0)).powi(2);
            let luz = vineta * sombra;
            let ruido = (azar(x as f64 * 0.37 + y as f64 * 91.7) - 0.5) * 2.0;

            let mut rgba = if velo {
                [0.0, 0.0, 0.0, 255.0 * (1.0 - 0.60 * luz)]
            } else {
                // Resplandor del color tras el disco y degradado en diagonal.
                let halo = (1.0 - d / (diag * 0.62)).clamp(0.0, 1.0).powi(2);
                let diagonal = (fx / ancho as f64 + fy / alto as f64) / 2.0;
                let mut c = [0.0, 0.0, 0.0, 255.0];
                for k in 0..3 {
                    let base = mezcla(26.0, 9.0, diagonal) + [0.0, 1.0, 5.0][k];
                    c[k] = (base + comp[k] * (0.34 * halo + 0.07 * (1.0 - diagonal))) * luz + ruido;
                }
                c
            };
            // Plato: un aro metálico oscuro que asoma bajo el disco.
            let cobertura = (plato - d + 0.5).clamp(0.0, 1.0);
            if cobertura > 0.0 {
                let aro = ((d / plato - 0.985) / 0.015).clamp(0.0, 1.0);
                let v = 26.0 + 30.0 * aro * (0.6 + 0.4 * reflejo((fy - cy).atan2(fx - cx)) * 2.0);
                for (k, c) in rgba.iter_mut().enumerate() {
                    let destino = if k == 3 { 255.0 } else { v };
                    *c = mezcla(*c, destino, cobertura);
                }
            }
            let i = (y * ancho + x) * 4;
            for k in 0..4 {
                px[i + k] = rgba[k].clamp(0.0, 255.0) as u8;
            }
        }
    }
    px
}

type P = (f64, f64);

fn dist_segmento(p: P, a: P, b: P) -> f64 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * vx + (p.1 - a.1) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
    ((p.0 - a.0 - t * vx).powi(2) + (p.1 - a.1 - t * vy).powi(2)).sqrt()
}

/// Distancia con signo a una caja orientada de `a` a `b` y semianchura `hw`;
/// devuelve también la posición transversal (-1..1) para sombrearla.
fn caja(p: P, a: P, b: P, hw: f64) -> (f64, f64) {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let largo = (vx * vx + vy * vy).sqrt();
    let (ux, uy) = (vx / largo, vy / largo);
    let (qx, qy) = (p.0 - a.0, p.1 - a.1);
    let u = qx * ux + qy * uy;
    let v = -qx * uy + qy * ux;
    (((u - largo / 2.0).abs() - largo / 2.0).max(v.abs() - hw), v / hw)
}

/// Brazo del tocadiscos. Devuelve (x, y, ancho, alto, píxeles RGBA) del
/// recorte que hay que superponer en el vídeo.
pub fn dibujar_brazo(
    lienzo: (usize, usize),
    disco: (f64, f64, f64),
    color: Rgb,
) -> (usize, usize, usize, usize, Vec<u8>) {
    let (cx, cy, r) = disco;
    let pt = |x: f64, y: f64| (cx + x * r, cy + y * r);
    let pivote = pt(1.13, -0.80);
    let aguja = pt(0.60, 0.36);
    let (dx, dy) = (aguja.0 - pivote.0, aguja.1 - pivote.1);
    let largo = (dx * dx + dy * dy).sqrt();
    let (ux, uy) = (dx / largo, dy / largo);
    let sobre = |t: f64, lado: f64| (pivote.0 + ux * t * r - uy * lado * r, pivote.1 + uy * t * r + ux * lado * r);
    let codo = sobre(largo / r - 0.20, 0.035);
    let punta = (aguja.0 + (aguja.0 - codo.0) * 0.12, aguja.1 + (aguja.1 - codo.1) * 0.12);

    // Recorte que contiene todo el brazo.
    let margen = 0.42 * r;
    let x0 = (pivote.0.min(aguja.0) - margen).max(0.0) as usize;
    let y0 = (pivote.1.min(aguja.1) - margen).max(0.0) as usize;
    let x1 = ((pivote.0.max(aguja.0) + margen) as usize).min(lienzo.0);
    let y1 = ((pivote.1.max(aguja.1) + margen) as usize).min(lienzo.1);
    let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));

    // Cada pieza: distancia con signo y color en el punto.
    let piezas = |p: P| -> [(f64, [f64; 3]); 6] {
        let metal = |v: f64, claro: f64, oscuro: f64| {
            let g = mezcla(claro, oscuro, (v + 0.35).abs().min(1.0).powf(1.5));
            [g, g, g + 3.0]
        };
        let d_base = ((p.0 - pivote.0).powi(2) + (p.1 - pivote.1).powi(2)).sqrt();
        let base = d_base - 0.115 * r;
        let aro = ((d_base / (0.115 * r) - 0.86) / 0.14).clamp(0.0, 1.0);
        let (contrapeso, vc) = caja(p, sobre(-0.30, 0.0), sobre(-0.11, 0.0), 0.058 * r);
        let tubo = dist_segmento(p, sobre(-0.12, 0.0), codo) - 0.013 * r;
        let (_, vt) = caja(p, sobre(-0.12, 0.0), codo, 0.013 * r);
        let eje = d_base - 0.055 * r;
        let (cabezal, _) = caja(p, codo, punta, 0.040 * r);
        let (capsula, _) = caja(p, aguja, punta, 0.040 * r);
        [
            (base, [38.0 + 34.0 * aro, 38.0 + 34.0 * aro, 42.0 + 34.0 * aro]),
            (contrapeso, metal(vc, 120.0, 34.0)),
            (tubo, metal(vt, 235.0, 120.0)),
            (eje, metal((p.1 - pivote.1) / (0.055 * r), 190.0, 70.0)),
            (cabezal, [24.0, 24.0, 27.0]),
            (capsula, [color.0 as f64, color.1 as f64, color.2 as f64]),
        ]
    };

    const SS: usize = 3;
    let desplazamiento = (0.030 * r, 0.050 * r);
    let mut px = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            // Sombra difusa de todo el conjunto.
            let p = ((x0 + x) as f64 + 0.5, (y0 + y) as f64 + 0.5);
            let ps = (p.0 - desplazamiento.0, p.1 - desplazamiento.1);
            let cerca = piezas(ps).iter().map(|c| c.0).fold(f64::MAX, f64::min);
            let a_sombra = 0.45 * (1.0 - (cerca / (0.045 * r) + 0.4).clamp(0.0, 1.0)).powi(2);
            let mut c = [0.0, 0.0, 0.0, a_sombra];
            // Piezas, de abajo arriba, con supermuestreo.
            let lejos = piezas(p).iter().all(|c| c.0 > 1.5);
            for n in 0..6 {
                if lejos {
                    break;
                }
                let mut cob = 0.0;
                let mut col = [0.0; 3];
                for sy in 0..SS {
                    for sx in 0..SS {
                        let q = (
                            (x0 + x) as f64 + (sx as f64 + 0.5) / SS as f64,
                            (y0 + y) as f64 + (sy as f64 + 0.5) / SS as f64,
                        );
                        let (d, k) = piezas(q)[n];
                        if d < 0.0 {
                            cob += 1.0;
                            for i in 0..3 {
                                col[i] += k[i];
                            }
                        }
                    }
                }
                if cob > 0.0 {
                    let a = cob / (SS * SS) as f64;
                    for i in 0..3 {
                        let nuevo = col[i] / cob;
                        c[i] = (nuevo * a + c[i] * c[3] * (1.0 - a)) / (a + c[3] * (1.0 - a));
                    }
                    c[3] = a + c[3] * (1.0 - a);
                }
            }
            let i = (y * w + x) * 4;
            for k in 0..3 {
                px[i + k] = c[k].clamp(0.0, 255.0) as u8;
            }
            px[i + 3] = (c[3] * 255.0).clamp(0.0, 255.0) as u8;
        }
    }
    (x0, y0, w, h, px)
}
