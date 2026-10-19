//! JPEG images decoded to the pixels SketchUp gets from them, for the average colour of a
//! texture (`docs/skp-format.md` §2.6, §3.5, §9).
//!
//! SketchUp 2026 reads images through OpenImageIO 2.5 over libjpeg-turbo 3.0.3 (the versions
//! SketchUpAPI.dll names), with libjpeg's defaults: accurate integer inverse DCT
//! (`JDCT_ISLOW`), "fancy" upsampling of the subsampled components (a triangle filter, edges
//! repeated), fixed-point YCbCr → RGB conversion. Other decoders round differently (the `image`
//! crate's, by 1 to 12 levels on many pixels), enough to move the average of an image by one
//! level, and with it the colorization offsets SketchUp derives from it. This decoder follows
//! the JPEG specification (ITU-T T.81) and the arithmetic of the IJG decoder, so that its pixels
//! are those of SketchUp's: baseline, extended and progressive Huffman files of 8-bit precision
//! (what textures are). Anything else (arithmetic coding, lossless, 12 bits, more than four
//! components) gives `None`. Corrupt data is handled as libjpeg handles it: the bits past the
//! end of a segment are zeros, the blocks after it stay empty until the next restart marker.
//!
//! This software is based in part on the work of the Independent JPEG Group (the integer
//! inverse DCT, upsampling and colour conversion arithmetic of libjpeg, reproduced here).
//!
//! The file is untrusted: every count and index is checked, allocations are bounded by
//! `max_alloc` before they are made, the number of scans is bounded, and nothing panics.

/// Natural (row-major) position of each coefficient in zigzag order, with 16 extra entries so
/// that a corrupt run length stays inside the block (as libjpeg's table).
const NATURAL: [u8; 80] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21,
    28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54,
    47, 55, 62, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63,
];

fn natural(k: usize) -> usize {
    NATURAL.get(k).copied().unwrap_or(63) as usize
}

/// Scans read at most (real progressive files have about ten). Each scan visits every block.
const MAX_SCANS: usize = 128;
/// Components of a frame (libjpeg's `MAX_COMPONENTS`).
const MAX_COMPONENTS: usize = 10;
/// Blocks of one MCU (libjpeg's `D_MAX_BLOCKS_IN_MCU`).
const MAX_BLOCKS_IN_MCU: usize = 10;
/// Largest width or height (libjpeg's `JPEG_MAX_DIMENSION`).
const MAX_DIMENSION: usize = 65500;

/// A decoded image: `channels` bytes per pixel (1 grey, 3 RGB), rows from the top.
#[derive(Debug)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub channels: usize,
    pub data: Vec<u8>,
    /// A progressive file whose first AC coefficients are not all refined: libjpeg smooths its
    /// blocks, which this decoder does not.
    pub partial: bool,
}

/// Whether the bytes are a JPEG file.
pub fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8])
}

/// Width and height from the frame header, without decoding.
pub fn dimensions(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut p = Parser { data: bytes, pos: 0 };
    if p.u8()? != 0xFF || p.u8()? != 0xD8 {
        return None;
    }
    loop {
        let m = p.next_marker();
        match m {
            0xC0..=0xCF if !matches!(m, 0xC4 | 0xC8 | 0xCC) => {
                let seg = p.segment()?;
                let h = u16::from_be_bytes([*seg.get(1)?, *seg.get(2)?]) as usize;
                let w = u16::from_be_bytes([*seg.get(3)?, *seg.get(4)?]) as usize;
                return Some((w, h));
            }
            0xD9 | 0xDA => return None,
            0xD0..=0xD7 | 0x01 => {}
            _ => {
                p.segment()?;
            }
        }
    }
}

/// Decodes a JPEG file as SketchUp does; `None` when it is not one this decoder reads, is
/// malformed past what libjpeg tolerates, or would need more than `max_alloc` bytes.
pub fn decode(bytes: &[u8], max_alloc: usize) -> Option<Image> {
    let mut d = Decoder::default();
    d.read(bytes, max_alloc)?;
    d.output(max_alloc)
}

/// The average of each channel over all the pixels (sum / count, truncated), as SketchUp
/// computes it, and the image's size.
pub fn average(bytes: &[u8], max_alloc: usize) -> Option<[u8; 3]> {
    let img = decode(bytes, max_alloc)?;
    let n = (img.width * img.height) as u64;
    if n == 0 {
        return None;
    }
    let mut sum = [0u64; 3];
    for px in img.data.chunks_exact(img.channels) {
        for (c, s) in sum.iter_mut().enumerate() {
            *s += *px.get(c.min(img.channels - 1)).unwrap_or(&0) as u64;
        }
    }
    Some(sum.map(|s| (s / n) as u8))
}

// ---------------------------------------------------------------------------------------------
// Markers

struct Parser<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn u8(&mut self) -> Option<u8> {
        let b = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    /// The next marker, skipping whatever is not one (libjpeg's `next_marker`); the end of the
    /// data is a fake end of image, as libjpeg's sources insert.
    fn next_marker(&mut self) -> u8 {
        loop {
            let Some(mut c) = self.u8() else { return 0xD9 };
            while c != 0xFF {
                let Some(x) = self.u8() else { return 0xD9 };
                c = x;
            }
            loop {
                let Some(x) = self.u8() else { return 0xD9 };
                c = x;
                if c != 0xFF {
                    break;
                }
            }
            if c != 0 {
                return c;
            }
        }
    }

    /// A marker segment: its bytes after the length.
    fn segment(&mut self) -> Option<&[u8]> {
        let hi = self.u8()? as usize;
        let lo = self.u8()? as usize;
        let len = (hi << 8 | lo).checked_sub(2)?;
        let s = self.data.get(self.pos..self.pos.checked_add(len)?)?;
        self.pos += len;
        Some(s)
    }
}

// ---------------------------------------------------------------------------------------------
// Huffman tables

#[derive(Clone)]
struct Huff {
    vals: [u8; 256],
    /// Largest code of each length (−1: none), with a sentinel at 17.
    maxcode: [i32; 18],
    /// Index in `vals` of the first code of each length, minus that code.
    valoffset: [i32; 18],
    /// For the codes of 8 bits or less, by their next 8 bits: length << 8 | symbol (0: longer).
    lookup: Vec<u16>,
}

/// A table as defined: number of codes of each length 1–16, and the symbols.
#[derive(Clone)]
struct HuffSpec {
    bits: [u8; 17],
    vals: Vec<u8>,
}

impl Huff {
    /// libjpeg's `jpeg_make_d_derived_tbl`: `None` for a table that is not a valid code.
    fn new(spec: &HuffSpec, dc: bool) -> Option<Huff> {
        let mut size = Vec::with_capacity(257);
        for l in 1..=16usize {
            for _ in 0..spec.bits[l] {
                size.push(l as u8);
            }
        }
        if size.len() > 256 || size.len() > spec.vals.len() {
            return None;
        }
        let n = size.len();
        let mut codes = vec![0u32; n];
        let mut code = 0u32;
        let mut si = size.first().copied().unwrap_or(0) as u32;
        let mut p = 0;
        while p < n {
            while p < n && size[p] as u32 == si {
                codes[p] = code;
                code += 1;
                p += 1;
            }
            if code >= 1 << si {
                return None;
            }
            code <<= 1;
            si += 1;
        }
        let mut h = Huff { vals: [0; 256], maxcode: [-1; 18], valoffset: [0; 18], lookup: vec![0; 256] };
        for (i, v) in spec.vals.iter().take(256).enumerate() {
            h.vals[i] = *v;
        }
        let mut p = 0usize;
        for l in 1..=16usize {
            let c = spec.bits[l] as usize;
            if c > 0 {
                h.valoffset[l] = p as i32 - codes[p] as i32;
                p += c;
                h.maxcode[l] = codes[p - 1] as i32;
            }
        }
        h.maxcode[17] = 0xFFFFF;
        let mut p = 0usize;
        for l in 1..=8usize {
            for _ in 0..spec.bits[l] {
                let shift = 8 - l;
                let base = (codes[p] as usize) << shift;
                for k in 0..1usize << shift {
                    if let Some(e) = h.lookup.get_mut(base + k) {
                        *e = (l as u16) << 8 | h.vals[p] as u16;
                    }
                }
                p += 1;
            }
        }
        if dc && h.vals[..n].iter().any(|&s| s > 15) {
            return None;
        }
        Some(h)
    }

    fn decode(&self, br: &mut Bits) -> u8 {
        let e = self.lookup[br.peek(8) as usize];
        if e != 0 {
            br.skip(u32::from(e >> 8));
            return e as u8;
        }
        let mut l = 9usize;
        let mut code = br.get(9) as i32;
        while l <= 16 && code > self.maxcode[l] {
            code = code << 1 | br.get(1) as i32;
            l += 1;
        }
        if l > 16 {
            return 0;
        }
        self.vals[((code + self.valoffset[l]) & 0xFF) as usize]
    }
}

/// The tables of Annex K, which libjpeg-turbo gives a file that defines none (Motion JPEG).
fn std_table(class: usize, slot: usize) -> Option<HuffSpec> {
    const DC_L: [u8; 17] = [0, 0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
    const DC_C: [u8; 17] = [0, 0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
    const AC_L: [u8; 17] = [0, 0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d];
    const AC_C: [u8; 17] = [0, 0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
    const AC_L_V: [u8; 162] = [
        0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71,
        0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0, 0x24, 0x33, 0x62, 0x72,
        0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x34, 0x35, 0x36, 0x37,
        0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59,
        0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83,
        0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3,
        0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3,
        0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2,
        0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
    ];
    const AC_C_V: [u8; 162] = [
        0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71, 0x13, 0x22,
        0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0, 0x15, 0x62, 0x72, 0xd1,
        0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x35, 0x36,
        0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58,
        0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a,
        0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a,
        0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba,
        0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda,
        0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
    ];
    let dc_vals: Vec<u8> = (0..12).collect();
    match (class, slot) {
        (0, 0) => Some(HuffSpec { bits: DC_L, vals: dc_vals }),
        (0, 1) => Some(HuffSpec { bits: DC_C, vals: dc_vals }),
        (1, 0) => Some(HuffSpec { bits: AC_L, vals: AC_L_V.to_vec() }),
        (1, 1) => Some(HuffSpec { bits: AC_C, vals: AC_C_V.to_vec() }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Entropy-coded data

/// The bits of an entropy-coded segment, as libjpeg's bit reader gives them: stuffed bytes
/// removed, zeros past a marker (or the end of the data).
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    acc: u64,
    n: u32,
    /// The marker that ended the data (libjpeg's `unread_marker`).
    marker: Option<u8>,
    /// Zeros were read past the end (libjpeg's `insufficient_data`).
    insufficient: bool,
}

impl Bits<'_> {
    fn fill(&mut self) {
        while self.n <= 56 && self.marker.is_none() {
            let Some(&c) = self.data.get(self.pos) else {
                self.marker = Some(0xD9);
                break;
            };
            self.pos += 1;
            let mut byte = c;
            if c == 0xFF {
                let mut next;
                loop {
                    match self.data.get(self.pos) {
                        Some(&x) => {
                            self.pos += 1;
                            next = x;
                        }
                        None => {
                            next = 0xD9;
                        }
                    }
                    if next != 0xFF {
                        break;
                    }
                }
                if next != 0 {
                    self.marker = Some(next);
                    break;
                }
                byte = 0xFF;
            }
            self.acc = self.acc << 8 | byte as u64;
            self.n += 8;
        }
    }

    fn peek(&mut self, k: u32) -> u32 {
        if self.n < k {
            self.fill();
        }
        let v = if self.n >= k { self.acc >> (self.n - k) } else { self.acc << (k - self.n) };
        (v & ((1u64 << k) - 1)) as u32
    }

    fn skip(&mut self, k: u32) {
        if k > self.n {
            self.insufficient = true;
            self.n = 0;
            self.acc = 0;
        } else {
            self.n -= k;
            self.acc &= (1u64 << self.n) - 1;
        }
    }

    fn get(&mut self, k: u32) -> u32 {
        if k == 0 {
            return 0;
        }
        let v = self.peek(k);
        self.skip(k);
        v
    }

    /// Discards the bits left (at a restart).
    fn reset(&mut self) {
        self.acc = 0;
        self.n = 0;
    }

    /// The next marker from where the reading is (libjpeg's `next_marker`).
    fn next_marker(&mut self) -> u8 {
        let mut p = Parser { data: self.data, pos: self.pos };
        let m = p.next_marker();
        self.pos = p.pos;
        m
    }
}

/// `HUFF_EXTEND`: the signed value of `r`, `s` bits long.
fn extend(r: u32, s: u32) -> i32 {
    if s == 0 {
        0
    } else if r < 1 << (s - 1) {
        r as i32 - (1 << s) + 1
    } else {
        r as i32
    }
}

// ---------------------------------------------------------------------------------------------
// Decoder

#[derive(Clone)]
struct Component {
    id: u8,
    h: usize,
    v: usize,
    tq: usize,
    /// Quantization table latched at the component's first scan (natural order).
    quant: Option<[u16; 64]>,
    /// Blocks of the component's samples, and allocated (rounded up to the MCU).
    bw: usize,
    bh: usize,
    pw: usize,
    ph: usize,
    /// Samples of the component.
    dw: usize,
    dh: usize,
    coefs: Vec<i16>,
    /// Successive approximation reached per coefficient (−1: none yet), progressive files.
    coef_bits: [i32; 64],
}

#[derive(Default)]
struct Decoder {
    width: usize,
    height: usize,
    progressive: bool,
    comps: Vec<Component>,
    hmax: usize,
    vmax: usize,
    quant: [Option<[u16; 64]>; 4],
    huff: [[Option<HuffSpec>; 4]; 2],
    restart_interval: usize,
    jfif: bool,
    adobe: Option<u8>,
    scans: usize,
    started: bool,
    partial: bool,
}

/// The parameters of one scan.
struct Scan {
    comps: Vec<usize>,
    dc: Vec<Option<Huff>>,
    ac: Vec<Option<Huff>>,
    ss: usize,
    se: usize,
    ah: u32,
    al: u32,
}

impl Decoder {
    fn read(&mut self, bytes: &[u8], max_alloc: usize) -> Option<()> {
        let mut p = Parser { data: bytes, pos: 0 };
        if p.u8()? != 0xFF || p.u8()? != 0xD8 {
            return None;
        }
        let mut frame = false;
        let mut marker = p.next_marker();
        loop {
            match marker {
                0xD8 => return None,
                0xD9 => break,
                0xC0..=0xC2 => {
                    if frame {
                        return None;
                    }
                    let seg = p.segment()?;
                    self.frame(seg, marker == 0xC2, max_alloc)?;
                    frame = true;
                }
                // Other frames: lossless, hierarchical, arithmetic coding.
                0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => return None,
                0xC4 => self.dht(p.segment()?)?,
                0xDB => self.dqt(p.segment()?)?,
                0xDD => {
                    let s = p.segment()?;
                    self.restart_interval = u16::from_be_bytes([*s.first()?, *s.get(1)?]) as usize;
                }
                0xE0 => {
                    let s = p.segment()?;
                    if s.len() >= 14 && s.starts_with(b"JFIF\0") {
                        self.jfif = true;
                    }
                }
                0xEE => {
                    let s = p.segment()?;
                    if s.len() >= 12 && s.starts_with(b"Adobe") {
                        self.adobe = Some(s[11]);
                    }
                }
                0xDA => {
                    if !frame {
                        return None;
                    }
                    let seg = p.segment()?;
                    let scan = self.sos(seg)?;
                    self.scans += 1;
                    if self.scans > MAX_SCANS {
                        return None;
                    }
                    let mut br = Bits { data: bytes, pos: p.pos, acc: 0, n: 0, marker: None, insufficient: false };
                    self.scan(&scan, &mut br)?;
                    p.pos = br.pos;
                    marker = match br.marker {
                        Some(m) => m,
                        None => p.next_marker(),
                    };
                    continue;
                }
                // Restart markers out of place, TEM: no segment.
                0xD0..=0xD7 | 0x01 => {}
                // Arithmetic conditioning, DNL, application data, comments: skipped.
                0xCC | 0xDC | 0xE1..=0xED | 0xEF | 0xFE => {
                    p.segment()?;
                }
                // libjpeg stops at any other marker.
                _ => return None,
            }
            marker = p.next_marker();
        }
        if !(frame && self.started) {
            return None;
        }
        self.partial = self.progressive && self.smoothing_useful();
        Some(())
    }

    /// libjpeg's `smoothing_ok`: whether libjpeg smooths the blocks of this progressive file
    /// (some of the first AC coefficients are not fully refined).
    fn smoothing_useful(&self) -> bool {
        const Q: [usize; 10] = [0, 1, 8, 16, 9, 2, 3, 10, 17, 24];
        let mut useful = false;
        for c in &self.comps {
            let Some(q) = c.quant else { return false };
            if Q.iter().any(|&i| q[i] == 0) || c.coef_bits[0] < 0 {
                return false;
            }
            useful |= c.coef_bits[1..10].iter().any(|&b| b != 0);
        }
        useful
    }

    fn frame(&mut self, s: &[u8], progressive: bool, max_alloc: usize) -> Option<()> {
        if *s.first()? != 8 {
            return None;
        }
        self.height = u16::from_be_bytes([*s.get(1)?, *s.get(2)?]) as usize;
        self.width = u16::from_be_bytes([*s.get(3)?, *s.get(4)?]) as usize;
        let n = *s.get(5)? as usize;
        if self.width == 0
            || self.height == 0
            || self.width > MAX_DIMENSION
            || self.height > MAX_DIMENSION
            || n == 0
            || n > MAX_COMPONENTS
            || s.len() != 6 + 3 * n
        {
            return None;
        }
        self.progressive = progressive;
        for i in 0..n {
            let c = &s[6 + 3 * i..9 + 3 * i];
            let (h, v) = ((c[1] >> 4) as usize, (c[1] & 15) as usize);
            if !(1..=4).contains(&h) || !(1..=4).contains(&v) {
                return None;
            }
            let tq = c[2] as usize;
            if tq > 3 {
                return None;
            }
            self.comps.push(Component {
                id: c[0],
                h,
                v,
                tq,
                quant: None,
                bw: 0,
                bh: 0,
                pw: 0,
                ph: 0,
                dw: 0,
                dh: 0,
                coefs: Vec::new(),
                coef_bits: [-1; 64],
            });
        }
        self.hmax = self.comps.iter().map(|c| c.h).max()?;
        self.vmax = self.comps.iter().map(|c| c.v).max()?;
        let (mcux, mcuy) = (self.width.div_ceil(8 * self.hmax), self.height.div_ceil(8 * self.vmax));
        let mut total = 0usize;
        for c in &mut self.comps {
            c.bw = (self.width * c.h).div_ceil(8 * self.hmax);
            c.bh = (self.height * c.v).div_ceil(8 * self.vmax);
            c.dw = (self.width * c.h).div_ceil(self.hmax);
            c.dh = (self.height * c.v).div_ceil(self.vmax);
            c.pw = mcux * c.h;
            c.ph = mcuy * c.v;
            // Coefficients (2 bytes) and, at the output, the samples (1 byte).
            total = total.checked_add(c.pw.checked_mul(c.ph)?.checked_mul(64 * 3)?)?;
        }
        total = total.checked_add(self.width.checked_mul(self.height)?.checked_mul(4)?)?;
        if total > max_alloc {
            return None;
        }
        for c in &mut self.comps {
            c.coefs = vec![0; c.pw * c.ph * 64];
        }
        Some(())
    }

    fn dqt(&mut self, mut s: &[u8]) -> Option<()> {
        while !s.is_empty() {
            let pq = s[0] >> 4;
            let tq = (s[0] & 15) as usize;
            if tq > 3 {
                return None;
            }
            let size = if pq != 0 { 128 } else { 64 };
            let body = s.get(1..1 + size)?;
            let mut q = [0u16; 64];
            for i in 0..64 {
                let v = if pq != 0 { u16::from_be_bytes([body[2 * i], body[2 * i + 1]]) } else { body[i] as u16 };
                q[natural(i)] = v;
            }
            self.quant[tq] = Some(q);
            s = &s[1 + size..];
        }
        Some(())
    }

    fn dht(&mut self, mut s: &[u8]) -> Option<()> {
        while s.len() > 16 {
            let index = s[0];
            let mut bits = [0u8; 17];
            bits[1..17].copy_from_slice(s.get(1..17)?);
            let count: usize = bits.iter().map(|&b| b as usize).sum();
            if count > 256 || count > s.len() - 17 {
                return None;
            }
            let vals = s[17..17 + count].to_vec();
            let class = (index >> 4) as usize;
            let slot = (index & 15) as usize;
            if class > 1 || slot > 3 {
                return None;
            }
            self.huff[class][slot] = Some(HuffSpec { bits, vals });
            s = &s[17 + count..];
        }
        Some(())
    }

    fn sos(&mut self, s: &[u8]) -> Option<Scan> {
        let n = *s.first()? as usize;
        if n == 0 || n > 4 || s.len() != 4 + 2 * n {
            return None;
        }
        let mut comps = Vec::with_capacity(n);
        let mut tables = Vec::with_capacity(n);
        for i in 0..n {
            let (id, t) = (s[1 + 2 * i], s[2 + 2 * i]);
            let ci = (0..self.comps.len()).find(|&c| self.comps[c].id == id && !comps.contains(&c))?;
            comps.push(ci);
            tables.push(((t >> 4) as usize, (t & 15) as usize));
        }
        let ss = s[1 + 2 * n] as usize;
        let se = s[2 + 2 * n] as usize;
        let a = s[3 + 2 * n];
        let (ah, al) = ((a >> 4) as u32, (a & 15) as u32);
        let (ss, se, ah, al) = if self.progressive {
            let dc = ss == 0;
            let bad =
                (dc && se != 0) || (!dc && (ss > se || se > 63 || n != 1)) || (ah != 0 && al + 1 != ah) || al > 13;
            if bad {
                return None;
            }
            (ss, se, ah, al)
        } else {
            (0, 63, 0, 0)
        };
        // The blocks of one MCU.
        let blocks: usize = if n == 1 { 1 } else { comps.iter().map(|&c| self.comps[c].h * self.comps[c].v).sum() };
        if blocks > MAX_BLOCKS_IN_MCU {
            return None;
        }
        // Quantization tables are latched at the first scan of each component.
        for &c in &comps {
            if self.comps[c].quant.is_none() {
                let tq = self.comps[c].tq;
                self.comps[c].quant = Some(self.quant.get(tq).copied().flatten()?);
            }
        }
        let table = |class: usize, slot: usize, huff: &[[Option<HuffSpec>; 4]; 2]| -> Option<Huff> {
            let spec = huff.get(class)?.get(slot)?.clone().or_else(|| std_table(class, slot))?;
            Huff::new(&spec, class == 0)
        };
        let mut dc = Vec::with_capacity(n);
        let mut ac = Vec::with_capacity(n);
        for &(td, ta) in &tables {
            let need_dc = !self.progressive || (ss == 0 && ah == 0);
            let need_ac = !self.progressive || ss != 0;
            dc.push(if need_dc { Some(table(0, td, &self.huff)?) } else { None });
            ac.push(if need_ac { Some(table(1, ta, &self.huff)?) } else { None });
        }
        if self.progressive {
            for &c in &comps {
                let bits = &mut self.comps[c].coef_bits;
                for b in bits.iter_mut().take(se + 1).skip(ss) {
                    *b = al as i32;
                }
            }
        }
        self.started = true;
        Some(Scan { comps, dc, ac, ss, se, ah, al })
    }

    /// Decodes the entropy-coded data of a scan into the coefficients.
    fn scan(&mut self, scan: &Scan, br: &mut Bits) -> Option<()> {
        let single = scan.comps.len() == 1;
        let (mcux, mcuy) = if single {
            let c = &self.comps[scan.comps[0]];
            (c.bw, c.bh)
        } else {
            (self.width.div_ceil(8 * self.hmax), self.height.div_ceil(8 * self.vmax))
        };
        let mut last_dc = vec![0i32; scan.comps.len()];
        let mut eobrun = 0u32;
        let mut restarts_to_go = self.restart_interval;
        let mut next_restart = 0u8;
        let total = mcux.checked_mul(mcuy)?;
        for m in 0..total {
            if self.restart_interval > 0 {
                if restarts_to_go == 0 {
                    // libjpeg's `process_restart` and `read_restart_marker`.
                    br.reset();
                    if br.marker.is_none() {
                        br.marker = Some(br.next_marker());
                    }
                    if br.marker == Some(0xD0 + next_restart) {
                        br.marker = None;
                    } else {
                        resync(br, next_restart);
                    }
                    next_restart = (next_restart + 1) & 7;
                    last_dc.iter_mut().for_each(|d| *d = 0);
                    eobrun = 0;
                    restarts_to_go = self.restart_interval;
                    if br.marker.is_none() {
                        br.insufficient = false;
                    }
                }
            } else if br.insufficient {
                // Nothing more can be read in this scan.
                break;
            }
            if !br.insufficient {
                let (mx, my) = (m % mcux, m / mcux);
                for (si, &ci) in scan.comps.iter().enumerate() {
                    let (h, v) = if single { (1, 1) } else { (self.comps[ci].h, self.comps[ci].v) };
                    for by in 0..v {
                        for bx in 0..h {
                            let (x, y) = (mx * h + bx, my * v + by);
                            let c = &mut self.comps[ci];
                            let at = (y * c.pw + x) * 64;
                            let block = c.coefs.get_mut(at..at + 64)?;
                            let dc = scan.dc.get(si).and_then(|t| t.as_ref());
                            let ac = scan.ac.get(si).and_then(|t| t.as_ref());
                            if !self.progressive {
                                decode_block(br, block, dc?, ac?, &mut last_dc[si]);
                            } else if scan.ss == 0 {
                                if scan.ah == 0 {
                                    let s = dc?.decode(br) as u32;
                                    let diff = extend(br.get(s), s);
                                    last_dc[si] = last_dc[si].checked_add(diff)?;
                                    block[0] = last_dc[si].wrapping_shl(scan.al) as i16;
                                } else if br.get(1) != 0 {
                                    block[0] |= (1i32 << scan.al) as i16;
                                }
                            } else if scan.ah == 0 {
                                ac_first(br, block, ac?, scan, &mut eobrun);
                            } else {
                                ac_refine(br, block, ac?, scan, &mut eobrun);
                            }
                        }
                    }
                }
            }
            if self.restart_interval > 0 {
                restarts_to_go -= 1;
            }
        }
        Some(())
    }

    /// The pixels: inverse DCT of each component, upsampling, colour conversion.
    fn output(&self, max_alloc: usize) -> Option<Image> {
        let channels = match self.comps.len() {
            1 => 1,
            3 | 4 => 3,
            _ => return None,
        };
        let out_len = self.width.checked_mul(self.height)?.checked_mul(channels)?;
        if out_len > max_alloc {
            return None;
        }
        // Each component's samples.
        let mut planes = Vec::with_capacity(self.comps.len());
        for c in &self.comps {
            let stride = c.pw * 8;
            let mut plane = vec![0u8; stride * c.ph * 8];
            let q = c.quant.unwrap_or([0; 64]);
            let mut qm = [0i32; 64];
            for (m, &v) in qm.iter_mut().zip(q.iter()) {
                // libjpeg keeps the multipliers as 16-bit signed values.
                *m = v as i16 as i32;
            }
            for by in 0..c.bh {
                for bx in 0..c.bw {
                    let at = (by * c.pw + bx) * 64;
                    let coefs = c.coefs.get(at..at + 64)?;
                    idct_islow(coefs, &qm, &mut plane[by * 8 * stride + bx * 8..], stride);
                }
            }
            planes.push(plane);
        }
        let mut rows: Vec<Vec<u8>> = self.comps.iter().map(|c| vec![0u8; (2 * c.dw).max(self.width) + 2]).collect();
        let mut colsum = vec![0i32; self.comps.iter().map(|c| c.dw).max()? + 2];
        let tables = ColorTables::new();
        let ycc = match self.comps.len() {
            3 => self.ycc3(),
            4 => matches!(self.adobe, Some(t) if t != 0),
            _ => false,
        };
        let mut data = vec![0u8; out_len];
        for y in 0..self.height {
            for (ci, c) in self.comps.iter().enumerate() {
                self.upsample_row(c, &planes[ci], y, &mut rows[ci], &mut colsum)?;
            }
            let out = &mut data[y * self.width * channels..(y + 1) * self.width * channels];
            match self.comps.len() {
                1 => out.copy_from_slice(&rows[0][..self.width]),
                3 => {
                    for x in 0..self.width {
                        let (a, b, c) = (rows[0][x], rows[1][x], rows[2][x]);
                        let px = if ycc { tables.rgb(a, b, c) } else { [a, b, c] };
                        out[3 * x..3 * x + 3].copy_from_slice(&px);
                    }
                }
                _ => {
                    for x in 0..self.width {
                        let (a, b, c, k) = (rows[0][x], rows[1][x], rows[2][x], rows[3][x]);
                        // YCCK → CMYK (inverted RGB, K kept), then OpenImageIO's CMYK → RGB.
                        let cmy = if ycc { tables.rgb(a, b, c).map(|v| 255 - v) } else { [a, b, c] };
                        let kf = k as f32 / 255.0;
                        for (o, &v) in out[3 * x..3 * x + 3].iter_mut().zip(cmy.iter()) {
                            *o = ((v as f32 / 255.0) * kf * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                        }
                    }
                }
            }
        }
        Some(Image { width: self.width, height: self.height, channels, data, partial: self.partial })
    }

    /// Whether three components are YCbCr (libjpeg's `default_decompress_parms`).
    fn ycc3(&self) -> bool {
        if self.jfif {
            return true;
        }
        if let Some(t) = self.adobe {
            return t != 0;
        }
        let ids: Vec<u8> = self.comps.iter().map(|c| c.id).collect();
        ids != [82, 71, 66]
    }

    /// Row `y` of the output, of one component (libjpeg's upsampling methods).
    fn upsample_row(&self, c: &Component, plane: &[u8], y: usize, out: &mut [u8], colsum: &mut [i32]) -> Option<()> {
        let stride = c.pw * 8;
        let row = |r: usize| -> Option<&[u8]> { plane.get(r * stride..r * stride + c.dw) };
        let (hmax, vmax) = (self.hmax, self.vmax);
        let dw = c.dw;
        if c.h == hmax && c.v == vmax {
            out[..self.width].copy_from_slice(row(y)?.get(..self.width)?);
        } else if c.h * 2 == hmax && c.v == vmax {
            let input = row(y)?;
            if dw > 2 {
                for x in 0..dw {
                    let here = 3 * input[x] as i32;
                    let left = input[x.saturating_sub(1)] as i32;
                    let right = input[(x + 1).min(dw - 1)] as i32;
                    out[2 * x] = ((here + left + 1) >> 2) as u8;
                    out[2 * x + 1] = ((here + right + 2) >> 2) as u8;
                }
            } else {
                for x in 0..dw {
                    out[2 * x] = input[x];
                    out[2 * x + 1] = input[x];
                }
            }
        } else if c.h == hmax && c.v * 2 == vmax {
            let i = y / 2;
            let (near, bias) = if y.is_multiple_of(2) { (i.saturating_sub(1), 1) } else { ((i + 1).min(c.dh - 1), 2) };
            let (a, b) = (row(i)?, row(near)?);
            for x in 0..dw {
                out[x] = ((3 * a[x] as i32 + b[x] as i32 + bias) >> 2) as u8;
            }
        } else if c.h * 2 == hmax && c.v * 2 == vmax {
            let i = y / 2;
            if dw > 2 {
                let near = if y.is_multiple_of(2) { i.saturating_sub(1) } else { (i + 1).min(c.dh - 1) };
                let (a, b) = (row(i)?, row(near)?);
                for x in 0..dw {
                    colsum[x] = 3 * a[x] as i32 + b[x] as i32;
                }
                for x in 0..dw {
                    let here = 3 * colsum[x];
                    out[2 * x] = ((here + colsum[x.saturating_sub(1)] + 8) >> 4) as u8;
                    out[2 * x + 1] = ((here + colsum[(x + 1).min(dw - 1)] + 7) >> 4) as u8;
                }
            } else {
                let input = row(i)?;
                for x in 0..dw {
                    out[2 * x] = input[x];
                    out[2 * x + 1] = input[x];
                }
            }
        } else if hmax % c.h == 0 && vmax % c.v == 0 {
            let (he, ve) = (hmax / c.h, vmax / c.v);
            let input = row(y / ve)?;
            for (x, o) in out.iter_mut().enumerate().take(self.width) {
                *o = *input.get(x / he)?;
            }
        } else {
            return None;
        }
        Some(())
    }
}

/// libjpeg's `jpeg_resync_to_restart`: what to do with a marker that is not the expected
/// restart marker `desired`.
fn resync(br: &mut Bits, desired: u8) {
    loop {
        let m = br.marker.unwrap_or(0) as i32;
        let d = desired as i32;
        let rst = |k: i32| 0xD0 + (k & 7);
        let action = if m < 0xC0 {
            2
        } else if !(0xD0..=0xD7).contains(&m) || m == rst(d + 1) || m == rst(d + 2) {
            3
        } else if m == rst(d - 1) || m == rst(d - 2) {
            2
        } else {
            1
        };
        match action {
            1 => {
                br.marker = None;
                return;
            }
            // (Each search moves forward; the end of the data is an end of image, action 3.)
            2 => br.marker = Some(br.next_marker()),
            _ => return,
        }
    }
}

/// A block of a sequential scan (libjpeg's `decode_mcu`).
fn decode_block(br: &mut Bits, block: &mut [i16], dc: &Huff, ac: &Huff, last_dc: &mut i32) {
    let s = dc.decode(br) as u32;
    let diff = extend(br.get(s), s);
    *last_dc = last_dc.wrapping_add(diff);
    block[0] = *last_dc as i16;
    let mut k = 1usize;
    while k < 64 {
        let rs = ac.decode(br) as u32;
        let (r, s) = (rs >> 4, rs & 15);
        if s != 0 {
            k += r as usize;
            let v = extend(br.get(s), s);
            block[natural(k)] = v as i16;
        } else {
            if r != 15 {
                break;
            }
            k += 15;
        }
        k += 1;
    }
}

/// First AC scan of a progressive file (libjpeg's `decode_mcu_AC_first`).
fn ac_first(br: &mut Bits, block: &mut [i16], ac: &Huff, scan: &Scan, eobrun: &mut u32) {
    if *eobrun > 0 {
        *eobrun -= 1;
        return;
    }
    let mut k = scan.ss;
    while k <= scan.se {
        let rs = ac.decode(br) as u32;
        let (r, s) = (rs >> 4, rs & 15);
        if s != 0 {
            k += r as usize;
            let v = extend(br.get(s), s);
            block[natural(k)] = v.wrapping_shl(scan.al) as i16;
        } else if r == 15 {
            k += 15;
        } else {
            *eobrun = 1 << r;
            if r != 0 {
                *eobrun += br.get(r);
            }
            *eobrun -= 1;
            break;
        }
        k += 1;
    }
}

/// Refinement AC scan of a progressive file (libjpeg's `decode_mcu_AC_refine`).
fn ac_refine(br: &mut Bits, block: &mut [i16], ac: &Huff, scan: &Scan, eobrun: &mut u32) {
    let p1 = (1i32 << scan.al) as i16;
    let m1 = (-1i32 << scan.al) as i16;
    // A correction bit: 1 when the magnitude of a coefficient already nonzero grows.
    fn refine(br: &mut Bits, coef: &mut i16, p1: i16, m1: i16) {
        if br.get(1) != 0 && (*coef & p1) == 0 {
            *coef = if *coef >= 0 { coef.wrapping_add(p1) } else { coef.wrapping_add(m1) };
        }
    }
    let mut k = scan.ss;
    if *eobrun == 0 {
        while k <= scan.se {
            let rs = ac.decode(br) as u32;
            let (mut r, s) = ((rs >> 4) as i32, rs & 15);
            let mut value = 0i16;
            if s != 0 {
                value = if br.get(1) != 0 { p1 } else { m1 };
            } else if r != 15 {
                *eobrun = 1 << r;
                if r != 0 {
                    *eobrun += br.get(r as u32);
                }
                break;
            }
            loop {
                let pos = natural(k);
                if block[pos] != 0 {
                    refine(br, &mut block[pos], p1, m1);
                } else {
                    r -= 1;
                    if r < 0 {
                        break;
                    }
                }
                k += 1;
                if k > scan.se {
                    break;
                }
            }
            if value != 0 {
                block[natural(k)] = value;
            }
            k += 1;
        }
    }
    if *eobrun > 0 {
        while k <= scan.se {
            let pos = natural(k);
            if block[pos] != 0 {
                refine(br, &mut block[pos], p1, m1);
            }
            k += 1;
        }
        *eobrun -= 1;
    }
}

// ---------------------------------------------------------------------------------------------
// Inverse DCT (libjpeg's `jpeg_idct_islow`)

const CONST_BITS: i32 = 13;
const PASS1_BITS: i32 = 2;
const FIX_0_298631336: i64 = 2446;
const FIX_0_390180644: i64 = 3196;
const FIX_0_541196100: i64 = 4433;
const FIX_0_765366865: i64 = 6270;
const FIX_0_899976223: i64 = 7373;
const FIX_1_175875602: i64 = 9633;
const FIX_1_501321110: i64 = 12299;
const FIX_1_847759065: i64 = 15137;
const FIX_1_961570560: i64 = 16069;
const FIX_2_053119869: i64 = 16819;
const FIX_2_562915447: i64 = 20995;
const FIX_3_072711026: i64 = 25172;

fn descale(x: i64, n: i32) -> i64 {
    (x + (1 << (n - 1))) >> n
}

/// After the inverse DCT: a sample from a value centred on 0 (libjpeg's range-limit table,
/// which wraps every 1024).
fn idct_sample(x: i64) -> u8 {
    let i = (x & 1023) as i32;
    match i {
        0..=127 => (i + 128) as u8,
        128..=511 => 255,
        512..=895 => 0,
        _ => (i - 896) as u8,
    }
}

/// The even and odd parts shared by both passes: the 8 outputs of one row or column (inputs
/// in natural order, already dequantized).
fn idct_1d(i: [i64; 8]) -> [i64; 8] {
    let (z2, z3) = (i[2], i[6]);
    let z1 = (z2 + z3) * FIX_0_541196100;
    let tmp2 = z1 + z3 * -FIX_1_847759065;
    let tmp3 = z1 + z2 * FIX_0_765366865;
    let tmp0 = (i[0] + i[4]) << CONST_BITS;
    let tmp1 = (i[0] - i[4]) << CONST_BITS;
    let (tmp10, tmp13, tmp11, tmp12) = (tmp0 + tmp3, tmp0 - tmp3, tmp1 + tmp2, tmp1 - tmp2);
    let (mut t0, mut t1, mut t2, mut t3) = (i[7], i[5], i[3], i[1]);
    let mut z1 = t0 + t3;
    let mut z2 = t1 + t2;
    let mut z3 = t0 + t2;
    let mut z4 = t1 + t3;
    let z5 = (z3 + z4) * FIX_1_175875602;
    t0 *= FIX_0_298631336;
    t1 *= FIX_2_053119869;
    t2 *= FIX_3_072711026;
    t3 *= FIX_1_501321110;
    z1 *= -FIX_0_899976223;
    z2 *= -FIX_2_562915447;
    z3 *= -FIX_1_961570560;
    z4 *= -FIX_0_390180644;
    z3 += z5;
    z4 += z5;
    t0 += z1 + z3;
    t1 += z2 + z4;
    t2 += z2 + z3;
    t3 += z1 + z4;
    [tmp10 + t3, tmp11 + t2, tmp12 + t1, tmp13 + t0, tmp13 - t0, tmp12 - t1, tmp11 - t2, tmp10 - t3]
}

fn idct_islow(coefs: &[i16], quant: &[i32; 64], out: &mut [u8], stride: usize) {
    let mut ws = [0i32; 64];
    for col in 0..8 {
        let q = |r: usize| coefs[r * 8 + col] as i32 * quant[r * 8 + col];
        if (1..8).all(|r| coefs[r * 8 + col] == 0) {
            let dc = q(0) << PASS1_BITS;
            for r in 0..8 {
                ws[r * 8 + col] = dc;
            }
            continue;
        }
        let v = idct_1d(std::array::from_fn(|r| q(r) as i64));
        for r in 0..8 {
            ws[r * 8 + col] = descale(v[r], CONST_BITS - PASS1_BITS) as i32;
        }
    }
    for row in 0..8 {
        let w = &ws[row * 8..row * 8 + 8];
        let line = &mut out[row * stride..row * stride + 8];
        if w[1..].iter().all(|&x| x == 0) {
            let s = idct_sample(descale(w[0] as i64, PASS1_BITS + 3));
            line.fill(s);
            continue;
        }
        let v = idct_1d(std::array::from_fn(|k| w[k] as i64));
        for (o, x) in line.iter_mut().zip(v) {
            *o = idct_sample(descale(x, CONST_BITS + PASS1_BITS + 3));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Colour conversion (libjpeg's `ycc_rgb_convert`)

struct ColorTables {
    cr_r: [i32; 256],
    cb_b: [i32; 256],
    cr_g: [i32; 256],
    cb_g: [i32; 256],
}

impl ColorTables {
    fn new() -> ColorTables {
        const SCALEBITS: i32 = 16;
        const ONE_HALF: i32 = 1 << (SCALEBITS - 1);
        let fix = |x: f64| (x * (1i64 << SCALEBITS) as f64 + 0.5) as i32;
        let mut t = ColorTables { cr_r: [0; 256], cb_b: [0; 256], cr_g: [0; 256], cb_g: [0; 256] };
        for i in 0..256usize {
            let x = i as i32 - 128;
            t.cr_r[i] = (fix(1.40200) * x + ONE_HALF) >> SCALEBITS;
            t.cb_b[i] = (fix(1.77200) * x + ONE_HALF) >> SCALEBITS;
            t.cr_g[i] = -fix(0.71414) * x;
            t.cb_g[i] = -fix(0.34414) * x + ONE_HALF;
        }
        t
    }

    fn rgb(&self, y: u8, cb: u8, cr: u8) -> [u8; 3] {
        let y = y as i32;
        let (cb, cr) = (cb as usize, cr as usize);
        let clamp = |v: i32| v.clamp(0, 255) as u8;
        [clamp(y + self.cr_r[cr]), clamp(y + ((self.cb_g[cb] + self.cr_g[cr]) >> 16)), clamp(y + self.cb_b[cb])]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat block decodes to its level (the shortcut and the full transform agree).
    #[test]
    fn a_flat_block_is_its_dc() {
        let mut coefs = [0i16; 64];
        let q = [1i32; 64];
        let mut out = [0u8; 64];
        for (dc, want) in [(0i16, 128u8), (-1024, 0), (1016, 255), (80, 138), (-80, 118)] {
            coefs[0] = dc;
            idct_islow(&coefs, &q, &mut out, 8);
            assert!(out.iter().all(|&s| s == want), "{dc}: {out:?}");
        }
        // With an AC term, the full transform runs.
        coefs[0] = 0;
        coefs[1] = 100;
        idct_islow(&coefs, &q, &mut out, 8);
        assert!(out[0] > 128 && out[7] < 128, "{out:?}");
        assert_eq!(&out[..8], &out[8..16]);
    }

    #[test]
    fn colour_tables_are_libjpegs() {
        let t = ColorTables::new();
        assert_eq!(t.rgb(128, 128, 128), [128, 128, 128]);
        assert_eq!(t.rgb(0, 128, 255), [178, 0, 0]);
        assert_eq!(t.rgb(255, 0, 128), [255, 255, 28]);
    }

    #[test]
    fn std_tables_are_valid_codes() {
        for (class, slot) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            assert!(Huff::new(&std_table(class, slot).unwrap(), class == 0).is_some());
        }
    }

    /// Broken, truncated and hostile inputs give `None` or an image, never a panic.
    #[test]
    fn hostile_inputs_do_not_panic() {
        let cases: Vec<Vec<u8>> = vec![
            vec![],
            vec![0xFF, 0xD8],
            vec![0xFF, 0xD8, 0xFF, 0xC0, 0, 11, 8, 0, 1, 0, 1, 1, 1, 0x11, 0],
            vec![0xFF, 0xD8, 0xFF, 0xC0, 0, 11, 8, 0xFF, 0xFF, 0xFF, 0xFF, 1, 1, 0x11, 0, 0xFF, 0xDA],
            vec![0xFF, 0xD8, 0xFF, 0xDB, 0, 3, 0x05],
            vec![
                0xFF, 0xD8, 0xFF, 0xC4, 0, 19, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255,
            ],
        ];
        for c in &cases {
            let _ = decode(c, 1 << 20);
            let _ = dimensions(c);
        }
        // A frame too large for the allowance is refused before anything is allocated.
        let big = [0xFF, 0xD8, 0xFF, 0xC0, 0, 11, 8, 0xFF, 0xFF, 0xFF, 0xFF, 1, 1, 0x11, 0];
        assert!(decode(&big, 1 << 20).is_none());
    }

    fn sample(w: u32, h: u32, quality: u8) -> Vec<u8> {
        let img =
            image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 9) as u8, (y * 5) as u8, ((x ^ y) * 31) as u8]));
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
            .encode_image(&image::DynamicImage::ImageRgb8(img))
            .unwrap();
        out
    }

    /// A JPEG from another encoder decodes to its size, close to its source.
    #[test]
    fn an_encoded_picture_comes_back() {
        let bytes = sample(37, 21, 95);
        assert_eq!(dimensions(&bytes), Some((37, 21)));
        let img = decode(&bytes, 1 << 20).unwrap();
        assert_eq!((img.width, img.height, img.channels, img.partial), (37, 21, 3, false));
        let px = &img.data[(5 * 37 + 10) * 3..][..3];
        let want = [90u8, 25, ((10 ^ 5) * 31) as u8];
        for (a, b) in px.iter().zip(want) {
            assert!((*a as i32 - b as i32).abs() < 24, "{px:?} {want:?}");
        }
        assert!(average(&bytes, 1 << 20).is_some());
    }

    /// Every truncation and thousands of damaged copies of a JPEG give an image or `None`,
    /// never a panic.
    #[test]
    fn damaged_jpegs_do_not_panic() {
        let good = sample(40, 24, 80);
        for cut in 0..good.len() {
            let _ = decode(&good[..cut], 1 << 22);
        }
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..3000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let mut b = good.clone();
            for k in 0..1 + (seed % 4) as usize {
                let at = (seed.rotate_left(k as u32 * 11) % b.len() as u64) as usize;
                b[at] ^= (seed >> (8 * k)) as u8 | 1;
            }
            let _ = decode(&b, 1 << 22);
        }
    }
}
