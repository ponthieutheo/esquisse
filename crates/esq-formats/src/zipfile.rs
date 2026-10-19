//! The formats that are ZIP packages: KMZ, IFCZIP and USDZ (whose entries must start on
//! 64-byte boundaries). They are read by the shared reader of `esq_core::zip` (checked
//! offsets, one decompression budget per archive), and written here.

use anyhow::Result;

pub use esq_core::zip::{Entry, is_zip};

/// Every file of the archive, in central-directory order (directories are skipped).
pub fn read(b: &[u8]) -> Result<Vec<Entry>> {
    esq_core::zip::read_all(b).map_err(anyhow::Error::msg)
}

/// The file named `name` (case-insensitive, `\` or `/`).
pub fn find<'a>(entries: &'a [Entry], name: &str) -> Option<&'a Entry> {
    let norm = |s: &str| s.replace('\\', "/").trim_start_matches("./").to_lowercase();
    let want = norm(name);
    entries.iter().find(|e| norm(&e.name) == want)
}

const fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}

static CRC: [u32; 256] = crc_table();

pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = CRC[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// Builds an archive.
#[derive(Default)]
pub struct Writer {
    buf: Vec<u8>,
    central: Vec<u8>,
    count: u16,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a file. `deflate` compresses it; `align` (when > 1, stored files only) pads the
    /// local header so the data starts on a multiple of `align` (USDZ wants 64).
    pub fn add(&mut self, name: &str, data: &[u8], deflate: bool, align: usize) {
        let name = name.replace('\\', "/");
        let crc = crc32(data);
        let (method, body) =
            if deflate { (8u16, miniz_oxide::deflate::compress_to_vec(data, 6)) } else { (0u16, data.to_vec()) };
        let offset = self.buf.len() as u32;
        let mut extra = Vec::new();
        if align > 1 && method == 0 {
            let start = self.buf.len() + 30 + name.len();
            let pad = (align - (start + 4) % align) % align;
            extra.extend_from_slice(&0xA11Eu16.to_le_bytes());
            extra.extend_from_slice(&(pad as u16).to_le_bytes());
            extra.extend(std::iter::repeat_n(0u8, pad));
        }
        // 1980-01-01 00:00 (reproducible output).
        let (time, date) = (0u16, 0x21u16);
        let flags = if name.is_ascii() { 0u16 } else { 0x0800 };
        let b = &mut self.buf;
        b.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        b.extend_from_slice(&20u16.to_le_bytes());
        b.extend_from_slice(&flags.to_le_bytes());
        b.extend_from_slice(&method.to_le_bytes());
        b.extend_from_slice(&time.to_le_bytes());
        b.extend_from_slice(&date.to_le_bytes());
        b.extend_from_slice(&crc.to_le_bytes());
        b.extend_from_slice(&(body.len() as u32).to_le_bytes());
        b.extend_from_slice(&(data.len() as u32).to_le_bytes());
        b.extend_from_slice(&(name.len() as u16).to_le_bytes());
        b.extend_from_slice(&(extra.len() as u16).to_le_bytes());
        b.extend_from_slice(name.as_bytes());
        b.extend_from_slice(&extra);
        b.extend_from_slice(&body);
        let c = &mut self.central;
        c.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        c.extend_from_slice(&20u16.to_le_bytes());
        c.extend_from_slice(&20u16.to_le_bytes());
        c.extend_from_slice(&flags.to_le_bytes());
        c.extend_from_slice(&method.to_le_bytes());
        c.extend_from_slice(&time.to_le_bytes());
        c.extend_from_slice(&date.to_le_bytes());
        c.extend_from_slice(&crc.to_le_bytes());
        c.extend_from_slice(&(body.len() as u32).to_le_bytes());
        c.extend_from_slice(&(data.len() as u32).to_le_bytes());
        c.extend_from_slice(&(name.len() as u16).to_le_bytes());
        c.extend_from_slice(&0u16.to_le_bytes());
        c.extend_from_slice(&0u16.to_le_bytes());
        c.extend_from_slice(&0u16.to_le_bytes());
        c.extend_from_slice(&0u16.to_le_bytes());
        c.extend_from_slice(&0u32.to_le_bytes());
        c.extend_from_slice(&offset.to_le_bytes());
        c.extend_from_slice(name.as_bytes());
        self.count += 1;
    }

    pub fn finish(mut self) -> Vec<u8> {
        let cd_off = self.buf.len() as u32;
        let cd_len = self.central.len() as u32;
        self.buf.extend_from_slice(&self.central);
        let b = &mut self.buf;
        b.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        b.extend_from_slice(&[0, 0, 0, 0]);
        b.extend_from_slice(&self.count.to_le_bytes());
        b.extend_from_slice(&self.count.to_le_bytes());
        b.extend_from_slice(&cd_len.to_le_bytes());
        b.extend_from_slice(&cd_off.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_alignment() {
        let mut w = Writer::new();
        w.add("doc.kml", b"<kml/>", true, 0);
        w.add("models/a.usdc", &[7u8; 100], false, 64);
        w.add("textures/é.png", &[1, 2, 3], false, 0);
        let z = w.finish();
        assert!(is_zip(&z));
        let e = read(&z).unwrap();
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].data, b"<kml/>");
        assert_eq!(find(&e, "MODELS\\A.usdc").unwrap().data, vec![7u8; 100]);
        assert_eq!(e[2].name, "textures/é.png");
        // The aligned entry's data starts on a multiple of 64.
        let pos = z.windows(100).position(|w| w == [7u8; 100]).unwrap();
        assert_eq!(pos % 64, 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    /// A KMZ whose directory names the same deflated megabyte 300 times: the old reader
    /// trusted each entry's declared size and inflated all of them (300 MB).
    #[test]
    fn a_kmz_bomb_is_refused() {
        let mut w = Writer::new();
        w.add("doc.kml", b"<kml/>", true, 0);
        w.add("models/a.dae", &vec![b' '; 1 << 20], true, 0);
        let z = w.finish();
        let cd = u32::from_le_bytes(z[z.len() - 6..z.len() - 2].try_into().unwrap()) as usize;
        let first = 46 + "doc.kml".len();
        let rec = z[cd + first..z.len() - 22].to_vec();
        let mut bomb = z[..cd + first].to_vec();
        for _ in 0..300 {
            bomb.extend(&rec);
        }
        bomb.extend(0x0605_4b50u32.to_le_bytes());
        bomb.extend([0; 4]);
        bomb.extend(301u16.to_le_bytes());
        bomb.extend(301u16.to_le_bytes());
        bomb.extend(((first + rec.len() * 300) as u32).to_le_bytes());
        bomb.extend((cd as u32).to_le_bytes());
        bomb.extend(0u16.to_le_bytes());
        assert!(bomb.len() < 100_000);
        assert!(read(&bomb).is_err());
        assert!(crate::kmz::import(&bomb, &Default::default()).is_err());
    }
}
