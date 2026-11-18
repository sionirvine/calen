//! Procedurally drawn app/tray icon: a calendar page showing today's date.

/// 3×5 pixel digits.
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b010, 0b010, 0b010],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

const RED: [u8; 3] = [0xe5, 0x48, 0x4d];
const PAGE: [u8; 3] = [0xfa, 0xfa, 0xfa];
const INK: [u8; 3] = [0x26, 0x26, 0x2b];
const MUTED: [u8; 3] = [0x8b, 0x8b, 0x94];

/// RGBA pixels, `size`×`size`. `day` = number to draw (None → today's).
/// `dnd` greys out the header; `problem` adds a small amber dot.
pub fn render(size: u32, day: Option<u32>, dnd: bool, problem: bool) -> Vec<u8> {
    let n = size as i32;
    let mut px = vec![0u8; (size * size * 4) as usize];
    let s = n as f32 / 32.0;
    let radius = 5.0 * s;
    let (x0, y0, x1, y1) = (1.5 * s, 2.0 * s, n as f32 - 1.5 * s, n as f32 - 1.0 * s);
    let header_bottom = y0 + 9.0 * s;
    let header = if dnd { MUTED } else { RED };

    let inside = |x: f32, y: f32| -> f32 {
        // coverage for a rounded rect via distance to corner centers (cheap AA)
        let cx = x.clamp(x0 + radius, x1 - radius);
        let cy = y.clamp(y0 + radius, y1 - radius);
        let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
        if x < x0 || x > x1 || y < y0 || y > y1 {
            return 0.0;
        }
        (radius - d + 0.5).clamp(0.0, 1.0)
    };

    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let a = inside(fx, fy);
            if a <= 0.0 {
                continue;
            }
            let c = if fy < header_bottom { header } else { PAGE };
            let i = ((y * n + x) * 4) as usize;
            px[i..i + 3].copy_from_slice(&c);
            px[i + 3] = (a * 255.0) as u8;
        }
    }

    // binder rings
    for ring_x in [10.0 * s, n as f32 - 10.0 * s] {
        fill_rect(&mut px, n, ring_x - 1.2 * s, 0.5 * s, 2.4 * s, 5.0 * s, PAGE);
    }

    // Some(0): app icon — a grid of days with one highlighted, instead of digits.
    if day == Some(0) {
        let (cols, rows) = (4, 3);
        let pad_x = 6.0 * s;
        let top = header_bottom + 3.5 * s;
        let cell_w = (x1 - x0 - 2.0 * pad_x) / cols as f32;
        let cell_h = (y1 - top - 3.0 * s) / rows as f32;
        let d = 3.2 * s;
        for r in 0..rows {
            for c in 0..cols {
                let cx = x0 + pad_x + (c as f32 + 0.5) * cell_w;
                let cy = top + (r as f32 + 0.5) * cell_h;
                let col = if r == 1 && c == 2 { RED } else { [0xc9, 0xc9, 0xcf] };
                fill_rect(&mut px, n, cx - d / 2.0, cy - d / 2.0, d, d, col);
            }
        }
        return px;
    }

    // date digits
    let day = day.unwrap_or_else(|| chrono::Datelike::day(&chrono::Local::now()));
    let text = day.to_string();
    let scale = if text.len() == 1 { 3.4 * s } else { 3.0 * s };
    let glyph_w = 3.0 * scale;
    let gap = 1.2 * s;
    let total_w = text.len() as f32 * glyph_w + (text.len() as f32 - 1.0) * gap;
    let mut gx = (n as f32 - total_w) / 2.0;
    let body_top = header_bottom;
    let gy = body_top + ((y1 - body_top) - 5.0 * scale) / 2.0;
    for ch in text.chars() {
        let d = ch.to_digit(10).unwrap() as usize;
        for (row, bits) in DIGITS[d].iter().enumerate() {
            for col in 0..3 {
                if bits & (0b100 >> col) != 0 {
                    fill_rect(&mut px, n, gx + col as f32 * scale, gy + row as f32 * scale, scale, scale, INK);
                }
            }
        }
        gx += glyph_w + gap;
    }

    if problem {
        let r = 5.0 * s;
        let (cx, cy) = (n as f32 - r - 0.5, n as f32 - r - 0.5);
        for y in 0..n {
            for x in 0..n {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                let a = (r - d + 0.5).clamp(0.0, 1.0);
                if a > 0.0 {
                    let i = ((y * n + x) * 4) as usize;
                    px[i..i + 3].copy_from_slice(&[0xf5, 0xa5, 0x24]);
                    px[i + 3] = px[i + 3].max((a * 255.0) as u8);
                }
            }
        }
    }
    px
}

/// PNG bytes of the icon (used for packaging assets).
pub fn png(size: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_raw(size, size, render(size, Some(0), false, false)).expect("icon buffer");
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).expect("png encode");
    out
}

/// Multi-size .ico with PNG-compressed entries.
pub fn ico(sizes: &[u32]) -> Vec<u8> {
    let pngs: Vec<Vec<u8>> = sizes.iter().map(|s| png(*s)).collect();
    let mut out = vec![0, 0, 1, 0];
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (s, p) in sizes.iter().zip(&pngs) {
        let d = if *s >= 256 { 0 } else { *s as u8 };
        out.extend_from_slice(&[d, d, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(p.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += p.len() as u32;
    }
    for p in pngs {
        out.extend(p);
    }
    out
}

/// `calen --export-icons <dir>`: write packaging icons.
pub fn export(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("calen.png"), png(256))?;
    std::fs::write(dir.join("calen.ico"), ico(&[16, 24, 32, 48, 64, 256]))?;
    Ok(())
}

fn fill_rect(px: &mut [u8], n: i32, x: f32, y: f32, w: f32, h: f32, c: [u8; 3]) {
    let (xa, ya) = (x.floor() as i32, y.floor() as i32);
    let (xb, yb) = ((x + w).ceil() as i32, (y + h).ceil() as i32);
    for yy in ya.max(0)..yb.min(n) {
        for xx in xa.max(0)..xb.min(n) {
            // fractional coverage on edges
            let cov_x = ((xx as f32 + 1.0).min(x + w) - (xx as f32).max(x)).clamp(0.0, 1.0);
            let cov_y = ((yy as f32 + 1.0).min(y + h) - (yy as f32).max(y)).clamp(0.0, 1.0);
            let a = cov_x * cov_y;
            if a <= 0.0 {
                continue;
            }
            let i = ((yy * n + xx) * 4) as usize;
            for k in 0..3 {
                px[i + k] = (px[i + k] as f32 * (1.0 - a) + c[k] as f32 * a) as u8;
            }
            px[i + 3] = px[i + 3].max((a * 255.0) as u8);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn renders_sizes() {
        for s in [16, 24, 32, 64, 256] {
            let px = super::render(s, Some(28), false, true);
            assert_eq!(px.len(), (s * s * 4) as usize);
            assert!(px.chunks(4).any(|p| p[3] == 255));
        }
    }
}
