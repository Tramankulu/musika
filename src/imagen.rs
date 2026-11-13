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

/// Gira el tono de un color (en grados) y ajusta su saturación.
fn girar_tono(c: Rgb, grados: f64, saturacion: f64) -> [f64; 3] {
    let (r, g, b) = (c.0 as f64 / 255.0, c.1 as f64 / 255.0, c.2 as f64 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let delta = max - min;
    let mut h = if delta < 1e-6 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    let s = (if max > 0.0 { delta / max } else { 0.0 } * saturacion).min(1.0);
    h = (h + grados).rem_euclid(360.0);
    let f = |n: f64| {
        let k = (n + h / 60.0).rem_euclid(6.0);
        255.0 * (max - max * s * k.min(4.0 - k).clamp(0.0, 1.0))
    };
    [f(5.0), f(3.0), f(1.0)]
}

/// Pinta un color con opacidad `a` (0..1) encima de un píxel RGBA.
fn pintar(base: &mut [f64; 4], rgb: [f64; 3], a: f64) {
    let ab = base[3] / 255.0;
    let total = a + ab * (1.0 - a);
    if total > 0.0 {
        for k in 0..3 {
            base[k] = (rgb[k] * a + base[k] * ab * (1.0 - a)) / total;
        }
    }
    base[3] = total * 255.0;
}

/// Fondo del vídeo, con el plato, la sombra bajo el disco y el panel de
/// cristal donde van los textos (`panel` = x, y, ancho, alto).
///
/// Sin carátula es opaco: manchas de luz suaves con el color de la etiqueta
/// y dos tonos vecinos. Con carátula es un velo semitransparente (viñeta,
/// sombras y panel) que se pone encima de la carátula desenfocada.
pub fn dibujar_fondo(
    ancho: usize,
    alto: usize,
    color: Rgb,
    disco: (f64, f64, f64),
    velo: bool,
    panel: (f64, f64, f64, f64),
) -> Vec<u8> {
    let (cx, cy, radio) = disco;
    let (w, h) = (ancho as f64, alto as f64);
    let diag = (w * w + h * h).sqrt();
    let plato = radio * R_PLATO;
    // Manchas de luz: (x, y, radio, intensidad, color).
    let manchas = [
        (cx, cy, diag * 0.55, 0.30, girar_tono(color, 0.0, 1.0)),
        (w * 0.98, h * 1.02, diag * 0.55, 0.30, girar_tono(color, 50.0, 0.9)),
        (w * 0.80, -h * 0.10, diag * 0.45, 0.22, girar_tono(color, -70.0, 0.8)),
    ];
    let radio_panel = panel.2.min(panel.3) * 0.07;
    let mut px = vec![0u8; ancho * alto * 4];
    for y in 0..alto {
        for x in 0..ancho {
            let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            // Viñeta respecto al centro del lienzo.
            let dc = ((fx - w / 2.0).powi(2) + (fy - h / 2.0).powi(2)).sqrt() / (diag / 2.0);
            let vineta = 1.0 - 0.50 * dc.powf(1.8);
            // Sombra suave y desplazada bajo el plato.
            let ds = ((fx - cx - radio * 0.04).powi(2) + (fy - cy - radio * 0.07).powi(2)).sqrt();
            let sombra = 1.0 - 0.7 * (1.0 - ((ds - plato) / (radio * 0.16)).clamp(0.0, 1.0)).powi(2);
            let luz = vineta * sombra;
            let ruido = (azar(x as f64 * 0.37 + y as f64 * 91.7) - 0.5) * 2.0;

            let mut rgba = if velo {
                [0.0, 0.0, 0.0, 255.0 * (1.0 - 0.60 * luz)]
            } else {
                let mut c = [10.0, 10.0, 14.0, 255.0];
                for (mx, my, mr, fuerza, tono) in &manchas {
                    let dm = ((fx - mx).powi(2) + (fy - my).powi(2)).sqrt();
                    let f = (1.0 - dm / mr).clamp(0.0, 1.0).powi(2) * fuerza;
                    for k in 0..3 {
                        c[k] += tono[k] * f;
                    }
                }
                for v in c.iter_mut().take(3) {
                    *v = *v * luz + ruido;
                }
                c
            };

            // Panel de cristal: relleno translúcido y un filo claro.
            let qx = (fx - panel.0 - panel.2 / 2.0).abs() - panel.2 / 2.0 + radio_panel;
            let qy = (fy - panel.1 - panel.3 / 2.0).abs() - panel.3 / 2.0 + radio_panel;
            let dp = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - radio_panel;
            let dentro = (0.5 - dp).clamp(0.0, 1.0);
            if dentro > 0.0 {
                if velo {
                    pintar(&mut rgba, [0.0, 0.0, 0.0], 0.28 * dentro);
                }
                // Algo más claro arriba, como un cristal iluminado desde lo alto.
                let brillo = 0.075 - 0.035 * ((fy - panel.1) / panel.3).clamp(0.0, 1.0);
                pintar(&mut rgba, [255.0, 255.0, 255.0], brillo * dentro);
                let filo = (1.0 - (dp + 1.0).abs()).clamp(0.0, 1.0);
                pintar(&mut rgba, [255.0, 255.0, 255.0], 0.16 * filo);
            }

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

/// Máscara de franjas verticales para convertir la onda en barras.
pub fn dibujar_barras(ancho: usize, alto: usize, barra: usize, paso: usize) -> Vec<u8> {
    let mut px = vec![0u8; ancho * alto * 4];
    for y in 0..alto {
        for x in (0..ancho).filter(|x| x % paso < barra) {
            let i = (y * ancho + x) * 4;
            px[i..i + 4].fill(255);
        }
    }
    px
}

/// Círculo relleno, para el tirador de la barra de progreso.
pub fn dibujar_punto(lado: usize, color: [u8; 3]) -> Vec<u8> {
    let c = lado as f64 / 2.0;
    let mut px = vec![0u8; lado * lado * 4];
    for y in 0..lado {
        for x in 0..lado {
            let d = ((x as f64 + 0.5 - c).powi(2) + (y as f64 + 0.5 - c).powi(2)).sqrt();
            let i = (y * lado + x) * 4;
            px[i..i + 3].copy_from_slice(&color);
            px[i + 3] = ((c - d).clamp(0.0, 1.0) * 255.0) as u8;
        }
    }
    px
}

type P = (f64, f64);

fn suma(a: P, dir: P, t: f64) -> P {
    (a.0 + dir.0 * t, a.1 + dir.1 * t)
}

fn unitario(a: P, b: P) -> P {
    let (x, y) = (b.0 - a.0, b.1 - a.1);
    let n = (x * x + y * y).sqrt();
    (x / n, y / n)
}

fn girar(v: P, grados: f64) -> P {
    let (s, c) = grados.to_radians().sin_cos();
    (v.0 * c - v.1 * s, v.0 * s + v.1 * c)
}

/// Coordenadas de `p` en el eje que sale de `o` con dirección `dir`:
/// (a lo largo, a lo ancho).
fn local(p: P, o: P, dir: P) -> P {
    let (qx, qy) = (p.0 - o.0, p.1 - o.1);
    (qx * dir.0 + qy * dir.1, -qx * dir.1 + qy * dir.0)
}

fn circulo(p: P, c: P, r: f64) -> f64 {
    ((p.0 - c.0).powi(2) + (p.1 - c.1).powi(2)).sqrt() - r
}

/// Cápsula (segmento con extremos redondos) de `a` a `b`.
fn capsula(p: P, a: P, b: P, hw: f64) -> f64 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * vx + (p.1 - a.1) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
    ((p.0 - a.0 - t * vx).powi(2) + (p.1 - a.1 - t * vy).powi(2)).sqrt() - hw
}

/// Sombreado de un cilindro visto desde arriba: `v` va de -1 a 1 a lo ancho.
fn cilindro(v: f64, oscuro: f64, claro: f64) -> [f64; 3] {
    let g = oscuro + (claro - oscuro) * (-((v + 0.3) / 0.38).powi(2)).exp() - 0.25 * oscuro * v * v;
    [g, g, g + 3.0]
}

/// Brazo del tocadiscos: base, contrapeso, eje, tubo, portacápsulas con su
/// asa y cápsula. Devuelve (x, y, ancho, alto, píxeles RGBA) del recorte que
/// hay que superponer en el vídeo.
pub fn dibujar_brazo(
    lienzo: (usize, usize),
    disco: (f64, f64, f64),
    color: Rgb,
) -> (usize, usize, usize, usize, Vec<u8>) {
    let (cx, cy, r) = disco;
    let pivote = (cx + 1.13 * r, cy - 0.80 * r);
    let aguja = (cx + 0.60 * r, cy + 0.36 * r);
    // El portacápsulas va acodado hacia el centro del disco, como en un brazo real.
    let cabezal = girar(unitario(pivote, aguja), 18.0);
    let codo = suma(aguja, cabezal, -0.17 * r);
    let frente = suma(aguja, cabezal, 0.045 * r);
    let tubo = unitario(pivote, codo);
    let largo_cabezal = 0.215 * r;
    let asa_a = suma(codo, cabezal, 0.15 * r);
    let asa_b = suma(asa_a, girar(cabezal, -70.0), 0.13 * r);
    let reposo = suma(pivote, girar(tubo, -24.0), 0.45 * r);
    let mando = (pivote.0 + 0.085 * r, pivote.1 + 0.105 * r);
    let acento = [color.0 as f64, color.1 as f64, color.2 as f64];

    // Devuelve la distancia a la pieza más cercana y el color de la que queda encima.
    let escena = |p: P| -> (f64, Option<[f64; 3]>) {
        let mut cerca = f64::MAX;
        let mut arriba = None;
        let mut pieza = |d: f64, c: [f64; 3]| {
            cerca = cerca.min(d);
            if d < 0.0 {
                arriba = Some(c);
            }
        };
        let gris = |g: f64| [g, g, g + 3.0];

        // Placa base.
        let d = circulo(p, pivote, 0.165 * r);
        let borde = ((d / (0.165 * r) + 0.10) / 0.10).clamp(0.0, 1.0);
        pieza(d, gris(24.0 + 26.0 * borde));
        // Mando del antideslizamiento y apoyo del brazo.
        let d = circulo(p, mando, 0.036 * r);
        pieza(d, gris(150.0 + 70.0 * d / (0.036 * r)));
        let d = circulo(p, reposo, 0.040 * r);
        pieza(d, gris(34.0 + 50.0 * ((d / (0.040 * r) + 0.25) / 0.25).clamp(0.0, 1.0)));
        pieza(circulo(p, reposo, 0.016 * r), gris(12.0));

        // Parte trasera del tubo, contrapeso y su anillo graduado.
        let (u, v) = local(p, pivote, (-tubo.0, -tubo.1));
        pieza(capsula(p, pivote, suma(pivote, tubo, -0.16 * r), 0.017 * r), cilindro(v / (0.017 * r), 95.0, 235.0));
        let d = ((u - 0.25 * r).abs() - 0.11 * r).max(v.abs() - 0.064 * r);
        let estria = 0.86 + 0.14 * (u * 0.9).sin();
        let c = cilindro(v / (0.064 * r), 22.0, 105.0);
        pieza(d, [c[0] * estria, c[1] * estria, c[2] * estria]);
        let d = ((u - 0.14 * r).abs() - 0.016 * r).max(v.abs() - 0.068 * r);
        pieza(d, cilindro(v / (0.068 * r), 95.0, 240.0));

        // Carcasa del eje.
        let d = circulo(p, pivote, 0.082 * r);
        let k = -d / (0.082 * r);
        pieza(d, gris(if k < 0.16 { 165.0 } else { 62.0 + 46.0 * (pivote.1 - p.1) / (0.082 * r) }));

        // Cápsula (asoma por delante del portacápsulas) y asa.
        let (u, v) = local(p, codo, cabezal);
        let d = ((u - 0.165 * r).abs() - 0.085 * r).max(v.abs() - 0.034 * r);
        let luz = 0.72 + 0.28 * (-((v / (0.034 * r) + 0.3) / 0.5).powi(2)).exp();
        pieza(d, [acento[0] * luz, acento[1] * luz, acento[2] * luz]);
        pieza(capsula(p, asa_a, asa_b, 0.008 * r), gris(205.0));
        pieza(circulo(p, asa_b, 0.013 * r), gris(225.0));

        // Tubo y casquillo de unión.
        let (ut, vt) = local(p, pivote, tubo);
        pieza(capsula(p, pivote, codo, 0.0125 * r), cilindro(vt / (0.0125 * r), 105.0, 245.0));
        let largo_tubo = local(codo, pivote, tubo).0;
        let d = ((ut - largo_tubo + 0.012 * r).abs() - 0.024 * r).max(vt.abs() - 0.020 * r);
        pieza(d, cilindro(vt / (0.020 * r), 30.0, 110.0));

        // Portacápsulas: se ensancha hacia delante; lleva dos ranuras con sus tornillos.
        let hw = mezcla(0.020, 0.041, (u / largo_cabezal).clamp(0.0, 1.0)) * r;
        let d = ((u - largo_cabezal / 2.0).abs() - largo_cabezal / 2.0).max(v.abs() - hw);
        pieza(d, cilindro(v / hw, 110.0, 222.0));
        for lado in [-1.0, 1.0] {
            let ranura = |t: f64| suma(suma(codo, cabezal, t * largo_cabezal), girar(cabezal, 90.0), lado * 0.021 * r);
            pieza(capsula(p, ranura(0.52), ranura(0.86), 0.0055 * r), gris(40.0));
            let d = circulo(p, ranura(0.70), 0.0105 * r);
            pieza(d, gris(70.0 - 40.0 * d / (0.0105 * r)));
        }
        pieza(circulo(p, frente, 0.0), gris(0.0)); // solo para incluir la punta en la sombra

        // Tapa del eje.
        let d = circulo(p, pivote, 0.043 * r);
        let giro = ((p.1 - pivote.1).atan2(p.0 - pivote.0) * 2.0 + 1.0).cos();
        pieza(d, gris(175.0 + 45.0 * giro + 30.0 * d / (0.043 * r)));
        pieza(circulo(p, pivote, 0.013 * r), gris(28.0));

        (cerca, arriba)
    };

    // Recorte que contiene todo el brazo.
    let x0 = (asa_a.0.min(frente.0) - 0.22 * r).max(0.0) as usize;
    let y0 = (pivote.1 - 0.48 * r).max(0.0) as usize;
    let x1 = ((pivote.0 + 0.48 * r).max(0.0) as usize).min(lienzo.0);
    let y1 = ((frente.1.max(asa_b.1) + 0.22 * r).max(0.0) as usize).min(lienzo.1);
    let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));

    const SS: usize = 3;
    let caida = (0.030 * r, 0.050 * r);
    let mut px = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let p = ((x0 + x) as f64 + 0.5, (y0 + y) as f64 + 0.5);
            // Sombra difusa de todo el conjunto.
            let (d_sombra, _) = escena((p.0 - caida.0, p.1 - caida.1));
            let a_sombra = 0.50 * (1.0 - (d_sombra / (0.05 * r) + 0.35).clamp(0.0, 1.0)).powi(2);
            // Piezas, con supermuestreo solo cerca de alguna.
            let (mut cob, mut col) = (0.0, [0.0; 3]);
            if escena(p).0 < 1.5 {
                for sy in 0..SS {
                    for sx in 0..SS {
                        let q = (
                            (x0 + x) as f64 + (sx as f64 + 0.5) / SS as f64,
                            (y0 + y) as f64 + (sy as f64 + 0.5) / SS as f64,
                        );
                        if let (_, Some(c)) = escena(q) {
                            cob += 1.0;
                            for k in 0..3 {
                                col[k] += c[k];
                            }
                        }
                    }
                }
            }
            let a = cob / (SS * SS) as f64;
            let total = a + a_sombra * (1.0 - a);
            let i = (y * w + x) * 4;
            if total > 0.0 {
                for k in 0..3 {
                    px[i + k] = (col[k] / cob.max(1.0) * a / total).clamp(0.0, 255.0) as u8;
                }
                px[i + 3] = (total * 255.0).clamp(0.0, 255.0) as u8;
            }
        }
    }
    (x0, y0, w, h, px)
}
