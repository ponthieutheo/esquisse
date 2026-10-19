//! Photoshop (.psd / .psb): the merged picture that ends the file ("image data" section),
//! written from Adobe's published file format specification. File › Import (`esq-app`) and
//! `Sketchup::ImageRep` (`esq-ruby`) read Photoshop files through it.
//!
//! The file is untrusted: the header's sizes are checked before anything is made.

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.b.len()).ok_or("fichier tronqué")?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn skip_block(&mut self, wide: bool) -> Result<(), String> {
        let n = if wide { self.u64()? as usize } else { self.u32()? as usize };
        self.take(n).map(|_| ())
    }
}

/// Whether the bytes are a Photoshop file.
pub fn is_psd(bytes: &[u8]) -> bool {
    bytes.starts_with(b"8BPS")
}

/// PackBits run-length decoding of one row into `out` (exactly `len` bytes).
pub fn unpack_bits(src: &[u8], len: usize, out: &mut Vec<u8>) -> Result<(), String> {
    let start = out.len();
    let mut i = 0;
    while out.len() - start < len {
        let n = *src.get(i).ok_or("ligne RLE tronquée")? as i8;
        i += 1;
        if n >= 0 {
            let k = n as usize + 1;
            out.extend_from_slice(src.get(i..i + k).ok_or("ligne RLE tronquée")?);
            i += k;
        } else if n != -128 {
            let v = *src.get(i).ok_or("ligne RLE tronquée")?;
            i += 1;
            out.extend(std::iter::repeat_n(v, (1 - n as isize) as usize));
        }
    }
    out.truncate(start + len);
    Ok(())
}

/// The merged picture: width, height, RGBA pixels (rows from the top), and whether the file
/// has a transparency channel.
pub fn decode(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>, bool), String> {
    let mut r = Reader { b: bytes, at: 0 };
    if r.take(4)? != b"8BPS" {
        return Err("signature Photoshop absente".into());
    }
    let version = r.u16()?;
    let wide = match version {
        1 => false,
        2 => true,
        v => return Err(format!("version Photoshop {v} inconnue")),
    };
    r.take(6)?;
    let channels = r.u16()? as usize;
    let h = r.u32()? as usize;
    let w = r.u32()? as usize;
    let depth = r.u16()?;
    let mode = r.u16()?;
    // The header's sizes are checked before anything is made: a 30-byte file declaring
    // 30000 × 30000 pixels of 65535 channels made the reader reserve 59 TB (the application
    // died). Photoshop has at most 56 channels; the picture made is at most 512 MB, the
    // limit of the other image readers.
    if w == 0 || h == 0 || channels == 0 || channels > 56 || w * h > (512 << 20) / 4 {
        return Err("taille d'image incohérente".into());
    }
    let palette = {
        let n = r.u32()? as usize;
        r.take(n)?.to_vec()
    };
    r.skip_block(false)?; // image resources
    r.skip_block(wide)?; // layers and masks
    let compression = r.u16()?;
    let bpc = match depth {
        1 => 0,
        8 => 1,
        16 => 2,
        32 => 4,
        d => return Err(format!("profondeur {d} bits non prise en charge")),
    };
    let row = if depth == 1 { w.div_ceil(8) } else { w * bpc };
    let size = row * h * channels;
    // What the file can hold: as is, or run-length encoded (128 bytes from 2 at best).
    let left = bytes.len().saturating_sub(r.at);
    if size > left.saturating_mul(64).saturating_add(1 << 16) {
        return Err("données d'image tronquées".into());
    }
    let mut planes: Vec<u8> = Vec::with_capacity(size);
    match compression {
        0 => planes.extend_from_slice(r.take(size)?),
        1 => {
            let counts: Vec<usize> = (0..h * channels)
                .map(|_| if wide { r.u32().map(|x| x as usize) } else { r.u16().map(|x| x as usize) })
                .collect::<Result<_, _>>()?;
            for c in counts {
                unpack_bits(r.take(c)?, row, &mut planes)?;
            }
        }
        c => return Err(format!("compression {c} non prise en charge")),
    }
    // One 8-bit sample of channel `c` at pixel `i` (16 and 32 bits keep the high byte).
    let sample = |c: usize, i: usize| -> u8 {
        let (y, x) = (i / w, i % w);
        let base = c * row * h + y * row;
        match depth {
            1 => {
                // Bitmap: 1 = black.
                if planes[base + x / 8] & (0x80 >> (x % 8)) != 0 { 0 } else { 255 }
            }
            32 => {
                let o = base + x * 4;
                let f = f32::from_be_bytes([planes[o], planes[o + 1], planes[o + 2], planes[o + 3]]);
                (f.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0).round() as u8
            }
            _ => planes[base + x * bpc],
        }
    };
    // The channel that follows the colour channels, when there is one, is the merged
    // transparency.
    let colour = match mode {
        3 => 3,
        4 => 4,
        _ => 1,
    };
    let alpha = channels > colour;
    let mut out = Vec::with_capacity(w * h * 4);
    for i in 0..w * h {
        let a = |k: usize| if channels > k { sample(k, i) } else { 255 };
        let px = match mode {
            // RGB.
            3 if channels >= 3 => [sample(0, i), sample(1, i), sample(2, i), a(3)],
            // Grayscale, duotone (shown as its gray), bitmap.
            0 | 1 | 8 => {
                let g = sample(0, i);
                [g, g, g, a(1)]
            }
            // Indexed: 256-entry palette stored as all reds, all greens, all blues.
            2 => {
                let k = sample(0, i) as usize;
                let pal = |c: usize| palette.get(c * 256 + k).copied().unwrap_or(0);
                [pal(0), pal(1), pal(2), 255]
            }
            // CMYK: Photoshop stores 255 - ink.
            4 if channels >= 4 => {
                let (c, m, y, k) = (sample(0, i) as u32, sample(1, i) as u32, sample(2, i) as u32, sample(3, i) as u32);
                [(c * k / 255) as u8, (m * k / 255) as u8, (y * k / 255) as u8, a(4)]
            }
            mode => return Err(format!("mode de couleur Photoshop {mode} non pris en charge")),
        };
        out.extend_from_slice(&px);
    }
    Ok((w as u32, h as u32, out, alpha && mode != 2))
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A small RGB Photoshop file (raw or RLE), 3 × 2 with an alpha channel.
    pub fn psd_bytes(rle: bool) -> Vec<u8> {
        let (w, h) = (3u32, 2u32);
        let mut b = b"8BPS".to_vec();
        b.extend_from_slice(&1u16.to_be_bytes());
        b.extend_from_slice(&[0; 6]);
        b.extend_from_slice(&4u16.to_be_bytes()); // R, G, B, A
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&8u16.to_be_bytes());
        b.extend_from_slice(&3u16.to_be_bytes()); // RGB
        b.extend_from_slice(&0u32.to_be_bytes()); // colour mode data
        b.extend_from_slice(&0u32.to_be_bytes()); // resources
        b.extend_from_slice(&0u32.to_be_bytes()); // layers
        let planes: [[u8; 6]; 4] =
            [[255, 0, 0, 10, 20, 30], [0, 255, 0, 10, 20, 30], [0, 0, 255, 10, 20, 30], [255; 6]];
        if rle {
            b.extend_from_slice(&1u16.to_be_bytes());
            let mut rows = Vec::new();
            for p in &planes {
                for y in 0..2 {
                    let row = &p[y * 3..y * 3 + 3];
                    // A literal run of 3 bytes.
                    let mut enc = vec![2u8];
                    enc.extend_from_slice(row);
                    rows.push(enc);
                }
            }
            for r in &rows {
                b.extend_from_slice(&(r.len() as u16).to_be_bytes());
            }
            for r in rows {
                b.extend(r);
            }
        } else {
            b.extend_from_slice(&0u16.to_be_bytes());
            for p in &planes {
                b.extend_from_slice(p);
            }
        }
        b
    }

    #[test]
    fn photoshop_files_are_read() {
        for rle in [false, true] {
            let (w, h, px, alpha) = decode(&psd_bytes(rle)).unwrap();
            assert_eq!((w, h, alpha), (3, 2, true));
            assert_eq!(px[0..4], [255, 0, 0, 255]);
            assert_eq!(px[8..12], [0, 0, 255, 255]);
            assert_eq!(px[16..20], [20, 20, 20, 255]);
        }
        assert!(decode(b"8BPS").is_err());
        assert!(decode(b"GIF89a").is_err() && !is_psd(b"GIF89a") && is_psd(&psd_bytes(false)));
        let mut runs = Vec::new();
        unpack_bits(&[0xFE, 7, 1, 9, 8], 5, &mut runs).unwrap();
        assert_eq!(runs, [7, 7, 7, 9, 8]);
    }

    /// A Photoshop header of a few bytes declaring 30000 × 30000 pixels of 65535 channels
    /// (or 16000 × 16000 of 4 channels, with no data): the reader reserved the planes'
    /// size at once (59 TB, the process died on the allocation). Refused from the header.
    #[test]
    fn photoshop_headers_cannot_ask_for_terabytes() {
        let header = |w: u32, h: u32, channels: u16, compression: u16| {
            let mut b = b"8BPS".to_vec();
            b.extend_from_slice(&1u16.to_be_bytes());
            b.extend_from_slice(&[0; 6]);
            b.extend_from_slice(&channels.to_be_bytes());
            b.extend_from_slice(&h.to_be_bytes());
            b.extend_from_slice(&w.to_be_bytes());
            b.extend_from_slice(&8u16.to_be_bytes());
            b.extend_from_slice(&3u16.to_be_bytes());
            b.extend_from_slice(&[0; 12]);
            b.extend_from_slice(&compression.to_be_bytes());
            b
        };
        assert!(decode(&header(30_000, 30_000, 65_535, 1)).is_err());
        assert!(decode(&header(16_000, 16_000, 4, 1)).is_err());
        // Within the picture limit, but far more than the file could hold.
        assert!(decode(&header(8_000, 8_000, 4, 1)).is_err());
        assert!(decode(&header(100_000, 100_000, 3, 0)).is_err());
    }
}
