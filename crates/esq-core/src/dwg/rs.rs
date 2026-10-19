//! What R2007 adds to the bit-level tools (spec §5.11–§5.13): Reed-Solomon coding of
//! pages, the two 64-bit CRCs, the CRC random encoder, and the page checksum of §5.4.1.
//!
//! The Reed-Solomon codes are RS(255, 251) for data pages and RS(255, 239) for system
//! pages, systematic: the data bytes of a block, highest degree first, then the parity.
//! The roots of the generator are α^k … α^254 (k = 251 or 239), which is what the
//! encoder of the specification (Rockliff's, data processed from the last byte) gives
//! once the bytes are stored in order.

/// Code word size.
pub const N: usize = 255;
/// Data bytes per block: data pages, system pages.
pub const DATA_K: usize = 251;
pub const SYSTEM_K: usize = 239;

/// GF(2^8) of a primitive polynomial: exponentials and logarithms.
struct Field {
    exp: [u8; 510],
    log: [u8; 256],
}

impl Field {
    fn new(poly: u16) -> Field {
        let (mut exp, mut log) = ([0u8; 510], [0u8; 256]);
        let mut x: u16 = 1;
        for (i, e) in exp.iter_mut().take(255).enumerate() {
            *e = x as u8;
            log[x as usize] = i as u8;
            x <<= 1;
            if x & 0x100 != 0 {
                x ^= poly;
            }
        }
        exp.copy_within(..255, 255);
        Field { exp, log }
    }

    fn mul(&self, a: u8, b: u8) -> u8 {
        if a == 0 || b == 0 { 0 } else { self.exp[self.log[a as usize] as usize + self.log[b as usize] as usize] }
    }
}

/// A Reed-Solomon code: its field and generator (highest degree first).
pub struct Code {
    field: Field,
    generator: Vec<u8>,
    k: usize,
}

impl Code {
    /// The primitive polynomials of §5.13: 1 + x² + x³ + x⁴ + x⁸ for data pages,
    /// 1 + x³ + x⁵ + x⁶ + x⁸ for system pages.
    pub fn new(k: usize) -> Code {
        let field = Field::new(if k == DATA_K { 0x11D } else { 0x169 });
        let nsym = N - k;
        let mut generator = vec![0u8; nsym + 1];
        generator[0] = 1;
        for i in 0..nsym {
            let root = field.exp[(k + i) % N];
            for j in (1..=i + 1).rev() {
                generator[j] ^= field.mul(generator[j - 1], root);
            }
        }
        Code { field, generator, k }
    }

    /// The parity of one block of `k` bytes.
    pub fn parity(&self, block: &[u8]) -> Vec<u8> {
        let nsym = N - self.k;
        let mut reg = vec![0u8; nsym];
        for i in 0..self.k {
            let feedback = block.get(i).copied().unwrap_or(0) ^ reg[0];
            for j in 0..nsym - 1 {
                reg[j] = reg[j + 1] ^ self.field.mul(feedback, self.generator[j + 1]);
            }
            reg[nsym - 1] = self.field.mul(feedback, self.generator[nsym]);
        }
        reg
    }

    /// Interleaved coding (§5.13.2): `data` cut into `blocks` blocks of `k` bytes
    /// (zeros complete the last one); byte `j` of code word `i` at `j × blocks + i`.
    pub fn interleaved(&self, data: &[u8]) -> Vec<u8> {
        let blocks = data.len().div_ceil(self.k).max(1);
        let mut out = vec![0u8; blocks * N];
        for b in 0..blocks {
            let start = (b * self.k).min(data.len());
            let mut block = data[start..(start + self.k).min(data.len())].to_vec();
            block.resize(self.k, 0);
            let parity = self.parity(&block);
            for (j, &x) in block.iter().chain(&parity).enumerate() {
                out[j * blocks + b] = x;
            }
        }
        out
    }

    /// Non-interleaved coding (§5.13.1): the data, then the parity of each block, the
    /// last block completed by `fill` (bytes that are not stored).
    pub fn sequential(&self, data: &[u8], fill: &[u8]) -> Vec<u8> {
        let blocks = data.len().div_ceil(self.k).max(1);
        let mut padded = data.to_vec();
        padded.extend(fill.iter().copied().chain(std::iter::repeat(0)).take(blocks * self.k - data.len()));
        let mut out = data.to_vec();
        for b in 0..blocks {
            out.extend(self.parity(&padded[b * self.k..(b + 1) * self.k]));
        }
        out
    }
}

// ---------------------------------------------------------------------------------------
// 64-bit CRCs (§5.12).

/// The byte order of the CRC64s and of the page checksum (§5.12): in each 8-byte block
/// the 16-bit words from the last to the first; the 1–7 bytes left as the table says.
pub fn reordered(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let (chunks, r) = data.as_chunks::<8>();
    for c in chunks {
        out.extend_from_slice(&[c[6], c[7], c[4], c[5], c[2], c[3], c[0], c[1]]);
    }
    if r.len() >= 4 {
        out.extend_from_slice(&[r[2], r[3], r[0], r[1]]);
        out.extend_from_slice(&r[4..]);
    } else {
        out.extend_from_slice(r);
    }
    out
}

/// The "normal" CRC64 (ECMA-182 polynomial, most significant bit first), inverted at
/// the end.
pub fn crc64_normal(init: u64, data: &[u8]) -> u64 {
    let mut table = [0u64; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = (i as u64) << 56;
        for _ in 0..8 {
            c = if c & (1 << 63) != 0 { (c << 1) ^ 0x42F0_E1EB_A9EA_3693 } else { c << 1 };
        }
        *t = c;
    }
    let mut crc = init;
    for b in reordered(data) {
        crc = table[((u64::from(b) ^ (crc >> 56)) & 0xFF) as usize] ^ (crc << 8);
    }
    !crc
}

/// The "mirrored" CRC64 (least significant bit first; its table is that of the spec,
/// whose entry 128 is the polynomial 0x95AC9329AC4BC9B5), not inverted.
pub fn crc64_mirrored(init: u64, data: &[u8]) -> u64 {
    let mut table = [0u64; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u64;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0x95AC_9329_AC4B_C9B5 } else { c >> 1 };
        }
        *t = c;
    }
    let mut crc = init;
    for b in reordered(data) {
        crc = table[((crc ^ u64::from(b)) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc
}

/// UpdateSeed1 of §5.12.
pub fn update_seed1(seed: u64, len: usize) -> u64 {
    let mut s = seed.wrapping_add(len as u64).wrapping_mul(0x343FD).wrapping_add(0x269EC3);
    s |= s.wrapping_mul(0x343FDu64 << 32).wrapping_add(0x269EC3u64 << 32);
    !s
}

/// UpdateSeed2 of §5.12.
pub fn update_seed2(seed: u64, len: usize) -> u64 {
    let len = len as u64;
    let s = seed.wrapping_add(len).wrapping_mul(0x343FD).wrapping_add(0x269EC3);
    !(s.wrapping_mul((1u64 << 32) + 0x343FD).wrapping_add(len + 0x269EC3))
}

/// The page checksum of §5.4.1: the R2004 one with a seed made from the length and the
/// bytes in the order of the CRC64s.
pub fn page_checksum(seed: u64, data: &[u8]) -> u32 {
    let s = seed.wrapping_add(data.len() as u64).wrapping_mul(0x343FD).wrapping_add(0x269EC3);
    let (mut s1, mut s2) = ((s & 0xFFFF) as u32, ((s >> 16) & 0xFFFF) as u32);
    for chunk in data.chunks(0x15B0) {
        for b in reordered(chunk) {
            s1 += u32::from(b);
            s2 += s1;
        }
        s1 %= 0xFFF1;
        s2 %= 0xFFF1;
    }
    (s2 << 16) | (s1 & 0xFFFF)
}

// ---------------------------------------------------------------------------------------
// The CRC random encoder (§5.11).

pub struct Random {
    table: [u32; 0x270],
    index: usize,
    padding: Vec<u8>,
}

impl Random {
    pub fn new(seed: u64) -> Random {
        let mut table = [0u32; 0x270];
        table[0] = (seed as u32).wrapping_mul(0x343FD).wrapping_add(0x269EC3);
        table[1] = ((seed >> 32) as u32).wrapping_mul(0x343FD).wrapping_add(0x269EC3);
        let mut value = table[1];
        for (i, t) in table.iter_mut().enumerate().skip(2) {
            value = ((value >> 30) ^ value).wrapping_mul(0x6C07_8965).wrapping_add(i as u32);
            *t = value;
        }
        let mut r = Random { table, index: 0, padding: Vec::new() };
        for _ in 0..0x80 {
            r.wrap();
            let x = r.table[r.index];
            r.padding.extend(x.to_le_bytes());
            r.index += 1;
        }
        r
    }

    fn wrap(&mut self) {
        if self.index >= 0x270 {
            self.index = 0;
        }
    }

    pub fn next(&mut self) -> u64 {
        self.index += 2;
        if self.index + 1 >= 0x270 {
            self.index = 0;
        }
        u64::from(self.table[self.index]) | (u64::from(self.table[self.index + 1]) << 32)
    }

    /// Spreads the 10 low bits of `value` over random bits.
    pub fn encode(&mut self, value: u32) -> u64 {
        let r = self.next();
        let mut lo = (r as u32) & 0xDF7D_F7DF;
        let mut hi = ((r >> 32) as u32) & 0xF7DF_7DF7;
        for (bit, mask) in [(0x200, 0x20), (0x100, 0x800), (0x80, 0x20000), (0x40, 0x80_0000), (0x20, 0x2000_0000)] {
            if value & bit != 0 {
                lo |= mask;
            }
        }
        for (bit, mask) in [(0x10, 0x08), (0x8, 0x200), (0x4, 0x8000), (0x2, 0x20_0000), (0x1, 0x800_0000)] {
            if value & bit != 0 {
                hi |= mask;
            }
        }
        u64::from(lo) | (u64::from(hi) << 32)
    }

    /// Padding bytes, from the start of the padding table.
    pub fn padding(&self, n: usize) -> Vec<u8> {
        self.padding.iter().copied().cycle().take(n).collect()
    }
}

// ---------------------------------------------------------------------------------------
// The R2007 LZ77 variant (§5.10), literals only.

/// The byte order of a literal run of `n` (1–32) bytes (§5.10.1): destination byte `i`
/// comes from source byte `ORDER[n][i]`, built from the table of copy blocks.
fn literal_order(n: usize) -> [usize; 32] {
    // (bytes, source, destination) of each copy; 2 and 3 copy reversed, 16 two halves swapped.
    let plan: &[(usize, usize, usize)] = match n {
        1 => &[(1, 0, 0)],
        2 => &[(2, 0, 0)],
        3 => &[(3, 0, 0)],
        4 => &[(4, 0, 0)],
        5 => &[(1, 4, 0), (4, 0, 1)],
        6 => &[(1, 5, 0), (4, 1, 1), (1, 0, 5)],
        7 => &[(2, 5, 0), (4, 1, 2), (1, 0, 6)],
        8 => &[(8, 0, 0)],
        9 => &[(1, 8, 0), (8, 0, 1)],
        10 => &[(1, 9, 0), (8, 1, 1), (1, 0, 9)],
        11 => &[(2, 9, 0), (8, 1, 2), (1, 0, 10)],
        12 => &[(4, 8, 0), (8, 0, 4)],
        13 => &[(1, 12, 0), (4, 8, 1), (8, 0, 5)],
        14 => &[(1, 13, 0), (4, 9, 1), (8, 1, 5), (1, 0, 13)],
        15 => &[(2, 13, 0), (4, 9, 2), (8, 1, 6), (1, 0, 14)],
        16 => &[(16, 0, 0)],
        17 => &[(8, 9, 0), (1, 8, 8), (8, 0, 9)],
        18 => &[(1, 17, 0), (16, 1, 1), (1, 0, 17)],
        19 => &[(3, 16, 0), (16, 0, 3)],
        20 => &[(4, 16, 0), (8, 8, 4), (8, 0, 12)],
        21 => &[(1, 20, 0), (4, 16, 1), (8, 8, 5), (8, 0, 13)],
        22 => &[(2, 20, 0), (4, 16, 2), (8, 8, 6), (8, 0, 14)],
        23 => &[(3, 20, 0), (4, 16, 3), (8, 8, 7), (8, 0, 15)],
        24 => &[(8, 16, 0), (16, 0, 8)],
        25 => &[(8, 17, 0), (1, 16, 8), (16, 0, 9)],
        26 => &[(1, 25, 0), (8, 17, 1), (1, 16, 9), (16, 0, 10)],
        27 => &[(2, 25, 0), (8, 17, 2), (1, 16, 10), (16, 0, 11)],
        28 => &[(4, 24, 0), (8, 16, 4), (8, 8, 12), (8, 0, 20)],
        29 => &[(1, 28, 0), (4, 24, 1), (8, 16, 5), (8, 8, 13), (8, 0, 21)],
        30 => &[(2, 28, 0), (4, 24, 2), (8, 16, 6), (8, 8, 14), (8, 0, 22)],
        31 => &[(1, 30, 0), (4, 26, 1), (8, 18, 5), (8, 10, 13), (8, 2, 21), (2, 0, 29)],
        _ => &[(16, 16, 0), (16, 0, 16)],
    };
    let mut row = [0usize; 32];
    for &(kind, s, d) in plan {
        match kind {
            1 => row[d] = s,
            2 => {
                row[d] = s + 1;
                row[d + 1] = s;
            }
            3 => {
                row[d] = s + 2;
                row[d + 1] = s + 1;
                row[d + 2] = s;
            }
            4 | 8 => {
                for i in 0..kind {
                    row[d + i] = s + i;
                }
            }
            _ => {
                for i in 0..8 {
                    row[d + i] = s + 8 + i;
                    row[d + 8 + i] = s + i;
                }
            }
        }
    }
    row
}

/// A literal length of 8 or more bytes: `n - 8` in the opcode, or 0x0F then a byte and,
/// past 0xFF, 16-bit words (§5.10.1).
fn literal_length(n: usize, out: &mut Vec<u8>) {
    if n < 0x17 {
        out.push((n - 8) as u8);
        return;
    }
    out.push(0x0F);
    let mut rest = n - 0x17;
    if rest < 0xFF {
        out.push(rest as u8);
        return;
    }
    out.push(0xFF);
    rest -= 0xFF;
    while rest >= 0xFFFF {
        out.extend(0xFFFFu16.to_le_bytes());
        rest -= 0xFFFF;
    }
    out.extend((rest as u16).to_le_bytes());
}

/// Literal bytes in the order the decoder shuffles them back (32 at a time).
fn literal_bytes(data: &[u8], out: &mut Vec<u8>) {
    for chunk in data.chunks(32) {
        let order = literal_order(chunk.len());
        let mut src = vec![0u8; chunk.len()];
        for (i, &b) in chunk.iter().enumerate() {
            src[order[i]] = b;
        }
        out.extend(src);
    }
}

/// A back-reference of `len` (3–255) bytes `dist` (1–0xFFFF) bytes back, followed by
/// `lit` (0–7) literals (§5.10.2).
fn back_reference(len: usize, dist: usize, lit: u8, out: &mut Vec<u8>) {
    if len <= 14 && dist <= 0x200 {
        let o = dist - 1;
        out.push(((len as u8) << 4) | (o & 0xF) as u8);
        out.push((((o >> 4) as u8) << 3) | lit);
    } else if len <= 18 && dist <= 0x2000 {
        let o = dist - 1;
        out.push(0x10 | (len - 3) as u8);
        out.push((o & 0xFF) as u8);
        out.push((((o >> 8) as u8) << 3) | lit);
    } else {
        out.push(0x20 | (len & 7) as u8);
        out.push((dist & 0xFF) as u8);
        out.push((dist >> 8) as u8);
        out.push((len & 0xF8) as u8 | lit);
    }
}

/// Compresses with the R2007 variant (§5.10): greedy matches of 3–255 bytes up to
/// 0xFFFF back, the stream opening with a literal run of at least 8 bytes.
pub fn compress(data: &[u8]) -> Vec<u8> {
    let n = data.len();
    let mut out = Vec::with_capacity(n / 2 + 16);
    if n < 8 {
        literal_length(8, &mut out);
        let mut d = data.to_vec();
        d.resize(8, 0);
        literal_bytes(&d, &mut out);
        return out;
    }
    let hash =
        |i: usize| ((usize::from(data[i]) << 10) ^ (usize::from(data[i + 1]) << 5) ^ usize::from(data[i + 2])) & 0x7FFF;
    let (mut head, mut prev) = (vec![usize::MAX; 0x8000], vec![usize::MAX; n]);
    let mut tokens: Vec<(usize, usize, usize, usize)> = Vec::new();
    let (mut lit_start, mut i) = (0, 0);
    while i < n {
        let mut best = (0, 0);
        if i >= 8 && i + 3 <= n {
            let mut j = head[hash(i)];
            let mut chain = 0;
            while j != usize::MAX && i - j <= 0xFFFF && chain < 48 {
                let mut l = 0;
                while i + l < n && l < 255 && data[j + l] == data[i + l] {
                    l += 1;
                }
                if l > best.0 {
                    best = (l, i - j);
                }
                j = prev[j];
                chain += 1;
            }
        }
        if best.0 >= 3 {
            tokens.push((lit_start, i, best.0, best.1));
            for k in i..i + best.0 {
                if k + 3 <= n {
                    prev[k] = head[hash(k)];
                    head[hash(k)] = k;
                }
            }
            i += best.0;
            lit_start = i;
        } else {
            if i + 3 <= n {
                prev[i] = head[hash(i)];
                head[hash(i)] = i;
            }
            i += 1;
        }
    }
    let first_end = tokens.first().map_or(n, |t| t.1);
    literal_length(first_end, &mut out);
    literal_bytes(&data[..first_end], &mut out);
    for (k, &(_, _, len, dist)) in tokens.iter().enumerate() {
        let (ls, le) = tokens.get(k + 1).map_or((lit_start, n), |t| (t.0, t.1));
        let lits = le - ls;
        back_reference(len, dist, if lits <= 7 { lits as u8 } else { 0 }, &mut out);
        if lits > 7 {
            literal_length(lits, &mut out);
        }
        literal_bytes(&data[ls..le], &mut out);
    }
    out
}

/// The rotation of the check data (§5.2.1.1.5).
pub fn rotate(value: u64, control: u64) -> u64 {
    value.rotate_left((control & 0x1F) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_tables_are_those_of_the_spec() {
        // The second entries of the two tables of §5.12, through a one-byte message.
        assert_eq!(!crc64_normal(0, &[1]), 0x42F0_E1EB_A9EA_3693);
        assert_eq!(crc64_mirrored(0, &[1]), 0x7AD8_70C8_3035_8979);
        assert_eq!(crc64_mirrored(0, &[0x80]), 0x95AC_9329_AC4B_C9B5);
        assert_eq!(reordered(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]), [6, 7, 4, 5, 2, 3, 0, 1, 10, 11, 8, 9, 12]);
        // UpdateSeed2 of the 0x110-byte file header, as AutoCAD's files have it.
        assert_eq!(update_seed2(0, 0x110), 0xFC61_189A_45A9_E6E5);
    }

    /// The decoder of §5.10.
    fn decompress(src: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        let mut p = 0;
        let mut op = src[p];
        p += 1;
        let mut length = 0;
        if op & 0xF0 == 0x20 {
            p += 3;
            length = usize::from(src[p - 1] & 7);
        }
        let literal_len = |op: u8, p: &mut usize| {
            let mut l = usize::from(op) + 8;
            if l == 0x17 {
                let n = usize::from(src[*p]);
                *p += 1;
                l += n;
                if n == 0xFF {
                    loop {
                        let w = usize::from(u16::from_le_bytes([src[*p], src[*p + 1]]));
                        *p += 2;
                        l += w;
                        if w != 0xFFFF {
                            break;
                        }
                    }
                }
            }
            l
        };
        let instruction = |op: &mut u8, p: &mut usize| -> (usize, usize) {
            match *op >> 4 {
                0 => {
                    let mut l = usize::from(*op & 0xF) + 0x13;
                    let low = usize::from(src[*p]);
                    *op = src[*p + 1];
                    *p += 2;
                    l += usize::from((*op >> 3) & 0x10);
                    (l, (usize::from(*op & 0x78) << 5) + 1 + low)
                }
                1 => {
                    let l = usize::from(*op & 0xF) + 3;
                    let low = usize::from(src[*p]);
                    *op = src[*p + 1];
                    *p += 2;
                    (l, (usize::from(*op & 0xF8) << 5) + 1 + low)
                }
                2 => {
                    let mut off = usize::from(src[*p]) | (usize::from(src[*p + 1]) << 8);
                    *p += 2;
                    let mut l = usize::from(*op & 7);
                    if *op & 8 == 0 {
                        *op = src[*p];
                        *p += 1;
                        l += usize::from(*op & 0xF8);
                    } else {
                        off += 1;
                        l += usize::from(src[*p]) << 3;
                        *op = src[*p + 1];
                        *p += 2;
                        l += (usize::from(*op & 0xF8) << 8) + 0x100;
                    }
                    (l, off)
                }
                _ => {
                    let l = usize::from(*op >> 4);
                    let low = usize::from(*op & 0xF);
                    *op = src[*p];
                    *p += 1;
                    (l, (usize::from(*op & 0xF8) << 1) + low + 1)
                }
            }
        };
        while p < src.len() {
            if length == 0 {
                length = literal_len(op, &mut p);
            }
            let mut s = p;
            while s < p + length {
                let n = (p + length - s).min(32);
                let order = literal_order(n);
                for &k in &order[..n] {
                    out.push(src[s + k]);
                }
                s += n;
            }
            p += length;
            if p >= src.len() {
                break;
            }
            op = src[p];
            p += 1;
            let (mut l, mut off) = instruction(&mut op, &mut p);
            loop {
                let start = out.len() - off;
                for k in 0..l {
                    out.push(out[start + k]);
                }
                length = usize::from(op & 7);
                if length != 0 || p >= src.len() {
                    break;
                }
                op = src[p];
                p += 1;
                if op >> 4 == 0 {
                    break;
                }
                if op >> 4 == 0xF {
                    op &= 0xF;
                }
                (l, off) = instruction(&mut op, &mut p);
            }
        }
        out
    }

    #[test]
    fn compression_round_trips() {
        let mut x: u32 = 99;
        let mut rnd = || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };
        let mut cases: Vec<Vec<u8>> = vec![vec![0; 0x110], (0..=255u8).cycle().take(3000).collect(), vec![7; 0xF800]];
        let mut t = Vec::new();
        for i in 0..0xF800usize {
            let r = rnd();
            t.push(if r % 4 == 0 {
                (r >> 8) as u8
            } else {
                t.get(i.saturating_sub((r as usize >> 12) % 0x9000 + 1)).copied().unwrap_or(b'q')
            });
        }
        cases.push(t);
        for len in [8, 9, 22, 23, 24, 300, 301] {
            let mut d: Vec<u8> = (0..len).map(|k| (k * 13) as u8).collect();
            d.extend((0..len).map(|k| (k * 13) as u8));
            d.extend(std::iter::repeat_n(1u8, len));
            cases.push(d);
        }
        for d in cases {
            let c = compress(&d);
            assert_eq!(decompress(&c), d, "{} bytes", d.len());
        }
        assert!(compress(&[0; 0x110]).len() < 20);
    }

    #[test]
    fn parity_of_a_file_made_by_autocad() {
        // The summary of an R2007 file and the four parity bytes stored after it, with
        // the RandomSeed of its file header (published with ACadSharp's test files): the
        // block is completed by the padding of the random encoder.
        let data = [
            0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00,
            0x00, 0x00, 0x09, 0x00, 0x41, 0x00, 0x75, 0x00, 0x74, 0x00, 0x6F, 0x00, 0x64, 0x00, 0x65, 0x00, 0x73, 0x00,
            0x6B, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xD2, 0x8E,
            0x0B, 0x00, 0xAF, 0x65, 0x25, 0x00, 0x7D, 0xB7, 0xD9, 0x03, 0x0F, 0x71, 0x25, 0x00, 0xF8, 0x06, 0x62, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let mut stored = data.to_vec();
        stored.resize(88, 0);
        let out = Code::new(DATA_K).sequential(&stored, &Random::new(15_793_935_738_994_308_941).padding(N));
        assert_eq!(out[..88], stored[..]);
        assert_eq!(out[88..], [0x05, 0xEF, 0x99, 0xA4]);
    }

    #[test]
    fn code_words_are_divisible_by_the_generator() {
        for k in [DATA_K, SYSTEM_K] {
            let code = Code::new(k);
            let block: Vec<u8> = (0..k).map(|i| (i * 37 + 11) as u8).collect();
            let parity = code.parity(&block);
            // Evaluate the code word (highest degree first) at every root: zero.
            let word: Vec<u8> = block.iter().chain(&parity).copied().collect();
            for i in 0..N - k {
                let root = code.field.exp[(k + i) % N];
                let mut acc = 0u8;
                for &c in &word {
                    acc = code.field.mul(acc, root) ^ c;
                }
                assert_eq!(acc, 0, "k {k} root {i}");
            }
            // Interleaving puts byte j of block i at j × blocks + i.
            let data: Vec<u8> = (0..2 * k as u32).map(|i| i as u8).collect();
            let out = code.interleaved(&data);
            assert_eq!(out.len(), 2 * N);
            assert_eq!((out[0], out[1], out[2], out[3]), (data[0], data[k], data[1], data[k + 1]));
        }
    }
}
