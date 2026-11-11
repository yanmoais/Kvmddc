use std::fs;
use std::path::Path;

fn main() {
    ensure_generated_icons();
    tauri_build::build();
}

/// Deterministically generates anti-aliased 128x128 squircle dual-display `icon.png` and `icon.ico`
/// at build time if not already present, so no binary image files ever need to be tracked in Git.
fn ensure_generated_icons() {
    let icons_dir = Path::new("icons");
    let _ = fs::create_dir_all(icons_dir);
    let png_path = icons_dir.join("icon.png");
    let ico_path = icons_dir.join("icon.ico");

    if png_path.exists() && ico_path.exists() {
        return;
    }

    let size = 128usize;
    let rgba = render_squircle_dual_display_rgba(size);
    let png_bytes = encode_uncompressed_rgba_png(size as u32, size as u32, &rgba);

    if !png_path.exists() {
        let _ = fs::write(&png_path, &png_bytes);
    }
    if !ico_path.exists() {
        let ico_bytes = wrap_png_in_ico(size as u8, size as u8, &png_bytes);
        let _ = fs::write(&ico_path, &ico_bytes);
    }
}

fn sd_round_rect(px: f32, py: f32, cx: f32, cy: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hx - r);
    let qy = (py - cy).abs() - (hy - r);
    (qx.max(0.0).hypot(qy.max(0.0))) + qx.max(qy).min(0.0) - r
}

fn render_squircle_dual_display_rgba(size: usize) -> Vec<u8> {
    let mut buf = vec![0u8; size * size * 4];
    let s = size as f32;

    for y in 0..size {
        for x in 0..size {
            let px = (x as f32 + 0.5) / s * 128.0;
            let py = (y as f32 + 0.5) / s * 128.0;

            let d_bg = sd_round_rect(px, py, 64.0, 64.0, 52.0, 52.0, 23.0);
            if d_bg > 1.0 {
                continue;
            }
            let bg_a = (0.5 - d_bg).clamp(0.0, 1.0);

            let t = ((py - 12.0) / 104.0).clamp(0.0, 1.0);
            let mut r = 34.0 * (1.0 - t) + 14.0 * t;
            let mut g = 45.0 * (1.0 - t) + 18.0 * t;
            let mut b = 68.0 * (1.0 - t) + 28.0 * t;

            // Back 27" silver monitor
            let d_mon = sd_round_rect(px, py, 53.0, 52.0, 31.0, 20.5, 4.0);
            if d_mon <= 0.5 {
                let cov = (0.5 - d_mon).clamp(0.0, 1.0);
                let d_scr = sd_round_rect(px, py, 53.0, 51.0, 28.0, 17.5, 2.5);
                let (mr, mg, mb) = if d_scr <= 0.0 {
                    (28.0, 132.0, 250.0)
                } else {
                    (238.0, 243.0, 250.0)
                };
                r = r * (1.0 - cov) + mr * cov;
                g = g * (1.0 - cov) + mg * cov;
                b = b * (1.0 - cov) + mb * cov;
            }

            // Stand
            let d_stand = sd_round_rect(px, py, 53.0, 78.0, 4.5, 6.5, 1.5)
                .min(sd_round_rect(px, py, 53.0, 85.0, 12.0, 1.8, 1.0));
            if d_stand <= 0.5 {
                let cov = (0.5 - d_stand).clamp(0.0, 1.0);
                r = r * (1.0 - cov) + 210.0 * cov;
                g = g * (1.0 - cov) + 218.0 * cov;
                b = b * (1.0 - cov) + 230.0 * cov;
            }

            // Front 14" MacBook Pro
            let d_mbp = sd_round_rect(px, py, 84.0, 76.0, 20.5, 13.5, 3.0);
            if d_mbp <= 0.5 {
                let cov = (0.5 - d_mbp).clamp(0.0, 1.0);
                let d_scr = sd_round_rect(px, py, 84.0, 76.0, 18.5, 11.5, 2.0);
                let (mr, mg, mb) = if d_scr <= 0.0 {
                    (48.0, 176.0, 120.0)
                } else {
                    (36.0, 44.0, 58.0)
                };
                r = r * (1.0 - cov) + mr * cov;
                g = g * (1.0 - cov) + mg * cov;
                b = b * (1.0 - cov) + mb * cov;
            }
            let d_deck = sd_round_rect(px, py, 84.0, 90.5, 24.0, 2.0, 1.2);
            if d_deck <= 0.5 {
                let cov = (0.5 - d_deck).clamp(0.0, 1.0);
                r = r * (1.0 - cov) + 206.0 * cov;
                g = g * (1.0 - cov) + 214.0 * cov;
                b = b * (1.0 - cov) + 228.0 * cov;
            }

            let idx = (y * size + x) * 4;
            buf[idx] = r.round() as u8;
            buf[idx + 1] = g.round() as u8;
            buf[idx + 2] = b.round() as u8;
            buf[idx + 3] = (bg_a * 255.0).round() as u8;
        }
    }
    buf
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn push_png_chunk(out: &mut Vec<u8>, tag: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    let crc_start = out.len();
    out.extend_from_slice(tag);
    out.extend_from_slice(payload);
    let crc = crc32(&out[crc_start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn encode_uncompressed_rgba_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
    let row_bytes = width as usize * 4;
    for y in 0..height as usize {
        raw.push(0);
        let start = y * row_bytes;
        raw.extend_from_slice(&rgba[start..start + row_bytes]);
    }

    let mut zlib = Vec::with_capacity(raw.len() + 64);
    zlib.extend_from_slice(&[0x78, 0x01]);
    let chunks: Vec<&[u8]> = raw.chunks(32768).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        let is_last = i + 1 == chunks.len();
        zlib.push(if is_last { 0x01 } else { 0x00 });
        let len = chunk.len() as u16;
        let nlen = !len;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&nlen.to_le_bytes());
        zlib.extend_from_slice(chunk);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut png = Vec::new();
    png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    push_png_chunk(&mut png, b"IHDR", &ihdr);
    push_png_chunk(&mut png, b"IDAT", &zlib);
    push_png_chunk(&mut png, b"IEND", &[]);
    png
}

fn wrap_png_in_ico(width: u8, height: u8, png_bytes: &[u8]) -> Vec<u8> {
    let mut ico = Vec::with_capacity(22 + png_bytes.len());
    ico.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    ico.push(width);
    ico.push(height);
    ico.push(0);
    ico.push(0);
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&32u16.to_le_bytes());
    ico.extend_from_slice(&(png_bytes.len() as u32).to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend_from_slice(png_bytes);
    ico
}
