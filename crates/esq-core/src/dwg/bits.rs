//! Bit streams, checksums and variable-length integers (spec chapter 2).

/// A bit stream: bits are written from the most significant one of each byte; raw
/// multi-byte values are little-endian.
#[derive(Clone, Default)]
pub(super) struct Bits {
    pub bytes: Vec<u8>,
    pub len: usize,
}

impl Bits {
    pub fn bit(&mut self, b: bool) {
        if self.len.is_multiple_of(8) {
            self.bytes.push(0);
        }
        if b {
            let i = self.len / 8;
            self.bytes[i] |= 0x80 >> (self.len % 8);
        }
        self.len += 1;
    }

    pub fn bits(&mut self, v: u64, n: u32) {
        for i in (0..n).rev() {
            self.bit((v >> i) & 1 != 0);
        }
    }

    /// B
    pub fn b(&mut self, v: bool) {
        self.bit(v);
    }

    /// BB
    pub fn bb(&mut self, v: u8) {
        self.bits(u64::from(v), 2);
    }

    /// RC
    pub fn rc(&mut self, v: u8) {
        if self.len.is_multiple_of(8) {
            self.bytes.push(v);
            self.len += 8;
        } else {
            self.bits(u64::from(v), 8);
        }
    }

    pub fn raw(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.rc(b);
        }
    }

    /// RS
    pub fn rs(&mut self, v: u16) {
        self.raw(&v.to_le_bytes());
    }

    /// RL
    pub fn rl(&mut self, v: u32) {
        self.raw(&v.to_le_bytes());
    }

    /// RD
    pub fn rd(&mut self, v: f64) {
        self.raw(&v.to_le_bytes());
    }

    /// BS: 00 short, 01 byte, 10 zero, 11 256.
    pub fn bs(&mut self, v: i16) {
        match v {
            0 => self.bb(2),
            256 => self.bb(3),
            1..=255 => {
                self.bb(1);
                self.rc(v as u8);
            }
            _ => {
                self.bb(0);
                self.rs(v as u16);
            }
        }
    }

    /// BL: 00 long, 01 byte, 10 zero.
    pub fn bl(&mut self, v: i32) {
        match v {
            0 => self.bb(2),
            1..=255 => {
                self.bb(1);
                self.rc(v as u8);
            }
            _ => {
                self.bb(0);
                self.rl(v as u32);
            }
        }
    }

    /// BLL: a 3-bit byte count, then the bytes.
    pub fn bll(&mut self, v: u64) {
        let n = (8 - v.leading_zeros() / 8) as u64;
        self.bits(n, 3);
        for i in 0..n {
            self.rc((v >> (8 * i)) as u8);
        }
    }

    /// BD: 00 double, 01 one, 10 zero.
    pub fn bd(&mut self, v: f64) {
        if v == 0.0 && v.is_sign_positive() {
            self.bb(2);
        } else if v == 1.0 {
            self.bb(1);
        } else {
            self.bb(0);
            self.rd(v);
        }
    }

    /// DD: a double that patches the bytes of `default` that differ.
    pub fn dd(&mut self, v: f64, default: f64) {
        let (vb, db) = (v.to_le_bytes(), default.to_le_bytes());
        if vb == db {
            self.bb(0);
        } else if vb[4..] == db[4..] {
            self.bb(1);
            self.raw(&vb[..4]);
        } else if vb[6..] == db[6..] {
            self.bb(2);
            self.raw(&vb[4..6]);
            self.raw(&vb[..4]);
        } else {
            self.bb(3);
            self.rd(v);
        }
    }

    pub fn bd2(&mut self, p: glam::DVec2) {
        self.bd(p.x);
        self.bd(p.y);
    }

    pub fn bd3(&mut self, p: glam::DVec3) {
        self.bd(p.x);
        self.bd(p.y);
        self.bd(p.z);
    }

    pub fn rd2(&mut self, p: glam::DVec2) {
        self.rd(p.x);
        self.rd(p.y);
    }

    pub fn dd2(&mut self, p: glam::DVec2, default: glam::DVec2) {
        self.dd(p.x, default.x);
        self.dd(p.y, default.y);
    }

    /// T: BS byte length, then the bytes (pre-2007 text in the drawing's code page).
    pub fn t8(&mut self, bytes: &[u8]) {
        self.bs(bytes.len() as i16);
        self.raw(bytes);
    }

    /// TU: BS character count (with the terminating zero), then UTF-16LE code units.
    pub fn tu(&mut self, s: &str) {
        let units: Vec<u16> = s.encode_utf16().take(32_000).collect();
        if units.is_empty() {
            self.bs(0);
            return;
        }
        self.bs(units.len() as i16 + 1);
        for u in units {
            self.rs(u);
        }
        self.rs(0);
    }

    /// H: code and counter in one byte, then the value's bytes, most significant first.
    pub fn h(&mut self, code: u8, v: u64) {
        let n = if v == 0 { 0 } else { (8 - v.leading_zeros() / 8) as u8 };
        self.rc(code << 4 | n);
        for i in (0..n).rev() {
            self.rc((v >> (8 * u32::from(i))) as u8);
        }
    }

    /// OT (R2010+ object type): a bit pair, then one or two bytes.
    pub fn ot(&mut self, kind: u16) {
        if kind < 0x100 {
            self.bb(0);
            self.rc(kind as u8);
        } else if (0x1F0..0x2F0).contains(&kind) {
            self.bb(1);
            self.rc((kind - 0x1F0) as u8);
        } else {
            self.bb(2);
            self.rs(kind);
        }
    }

    pub fn append(&mut self, o: &Bits) {
        if self.len.is_multiple_of(8) {
            self.bytes.extend_from_slice(&o.bytes);
            self.len += o.len;
            self.bytes.truncate(self.len.div_ceil(8));
        } else {
            for i in 0..o.len {
                self.bit(o.bytes[i / 8] & (0x80 >> (i % 8)) != 0);
            }
        }
    }

    /// Appends the R2007+ string stream of an object or section and the bit that says
    /// whether there is one (spec §20.1): the strings, their size in bits (one or two
    /// 16-bit words, the high one first), then the flag.
    pub fn string_stream(&mut self, strings: &Bits) {
        if strings.len == 0 {
            self.b(false);
            return;
        }
        self.append(strings);
        let n = strings.len;
        if n < 0x8000 {
            self.rs(n as u16);
        } else {
            self.rs((n >> 15) as u16);
            self.rs((n & 0x7FFF) as u16 | 0x8000);
        }
        self.b(true);
    }
}

/// The spec's "8-bit CRC": CRC-16 with polynomial 0xA001 (reflected).
pub fn crc16(seed: u16, data: &[u8]) -> u16 {
    let mut crc = seed;
    for &b in data {
        crc ^= u16::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xA001 } else { crc >> 1 };
        }
    }
    crc
}

/// The 32-bit CRC of R2004+ (spec §2.14.2), the usual reflected 0xEDB88320 one.
pub fn crc32(seed: u32, data: &[u8]) -> u32 {
    let mut crc = !seed;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// MS: 15 bits per little-endian 16-bit module, the high bit set when another follows.
pub fn modular_short(mut v: usize) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let m = (v & 0x7FFF) as u16;
        v >>= 15;
        if v == 0 {
            out.extend(m.to_le_bytes());
            return out;
        }
        out.extend((m | 0x8000).to_le_bytes());
    }
}

/// Unsigned modular char: 7 bits per byte, the high bit set when another follows.
pub fn modular_char_u(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let b = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

/// Signed modular char: the last byte holds 6 bits and the sign (0x40).
pub fn modular_char(v: i64, out: &mut Vec<u8>) {
    let (mut m, neg) = (v.unsigned_abs(), v < 0);
    while m >= 0x40 {
        out.push((m & 0x7F) as u8 | 0x80);
        m >>= 7;
    }
    out.push(m as u8 | if neg { 0x40 } else { 0 });
}

/// Text in the drawing's code page (ANSI 1252), at most 32 000 bytes: other characters
/// as `\U+XXXX`, which every AutoCAD reads back.
pub fn encode_ansi(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for c in s.chars() {
        match crate::vector2d::win_ansi(c) {
            Some(b) => out.push(b),
            None if (c as u32) < 0x10000 && !c.is_control() => {
                out.extend(format!("\\U+{:04X}", c as u32).bytes());
            }
            None => out.push(b'?'),
        }
        if out.len() > 32_000 {
            out.truncate(32_000);
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_of_the_spec_example() {
        // The APPID "ACAD" of the specification (§20.4.66.1): size, data, then CRC 8C E9.
        let obj = [
            0x13, 0x00, 0x50, 0xC0, 0x44, 0x67, 0x40, 0x00, 0x00, 0x09, 0x04, 0x41, 0x43, 0x41, 0x44, 0xC0, 0x0C, 0x10,
            0x83, 0x05, 0x0A,
        ];
        assert_eq!(crc16(0xC0C1, &obj).to_le_bytes(), [0x8C, 0xE9]);
        // The standard check value of CRC-32.
        assert_eq!(crc32(0, b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn bit_codes() {
        let mut b = Bits::default();
        b.bs(0);
        b.bs(256);
        b.bs(7);
        b.bl(0);
        assert_eq!(b.len, 2 + 2 + 10 + 2);
        assert_eq!(b.bytes, [0b1011_0100, 0b0001_1110]);
        let mut h = Bits::default();
        h.h(5, 0x1D);
        h.h(4, 0);
        h.h(3, 0x1234);
        assert_eq!(h.bytes, [0x51, 0x1D, 0x40, 0x32, 0x12, 0x34]);
        assert_eq!(modular_short(0x13), [0x13, 0x00]);
        assert_eq!(modular_short(0x8000), [0x00, 0x80, 0x01, 0x00]);
        let mut mc = Vec::new();
        modular_char(100, &mut mc);
        modular_char(-5, &mut mc);
        modular_char(63, &mut mc);
        assert_eq!(mc, [0xE4, 0x00, 0x45, 0x3F]);
        assert_eq!(encode_ansi("Façade €"), b"Fa\xE7ade \x80");
        assert_eq!(encode_ansi("Ω"), b"\\U+03A9");
        // OT: a byte, a byte above 0x1F0, a short.
        let mut o = Bits::default();
        o.ot(0x13);
        o.ot(0x1F3);
        o.ot(502);
        o.ot(0x300);
        assert_eq!(o.len, 10 + 10 + 10 + 18);
        // BLL: three bits of length, then the bytes.
        let mut l = Bits::default();
        l.bll(0);
        l.bll(0x1234);
        assert_eq!(l.len, 3 + 3 + 16);
        assert_eq!(l.bytes, [0x08, 0xD0, 0x48]);
        // The string stream: strings, size in bits, flag; an empty one is the flag alone.
        let mut s = Bits::default();
        s.tu("A");
        let mut d = Bits::default();
        d.string_stream(&s);
        assert_eq!(d.len, s.len + 16 + 1);
        let mut e = Bits::default();
        e.string_stream(&Bits::default());
        assert_eq!(e.len, 1);
    }
}
