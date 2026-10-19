//! The ZIP archive of SketchUp 2021+ files, written as SketchUp.exe writes it
//! (`docs/skp-format.md`, §1.1): every entry deflated, version 4.5 (ZIP64) in the local
//! headers with an empty ZIP64 field, dates of 1980-01-00, offsets from the start of the
//! archive; then the « VFF » marker's checksum over the entries' CRCs.

use crate::native::zip::crc32;

pub struct Entry {
    pub name: String,
    pub crc: u32,
    pub size: u64,
    pub compressed: u64,
    pub local: u64,
}

#[derive(Default)]
pub struct ZipWriter {
    pub out: Vec<u8>,
    pub entries: Vec<Entry>,
}

/// General purpose flags SketchUp writes (deflate option bits 1 and 2).
const FLAGS: u16 = 6;
/// 1980-01-00: month 1, day 0.
const DATE: u16 = 0x0020;

impl ZipWriter {
    pub fn new() -> ZipWriter {
        ZipWriter::default()
    }

    /// Adds an entry, deflated.
    pub fn add(&mut self, name: &str, data: &[u8]) {
        let packed = miniz_oxide::deflate::compress_to_vec(data, 6);
        let crc = crc32(data);
        let local = self.out.len() as u64;
        let o = &mut self.out;
        o.extend(0x0403_4b50u32.to_le_bytes());
        o.extend(45u16.to_le_bytes());
        o.extend(FLAGS.to_le_bytes());
        o.extend(8u16.to_le_bytes());
        o.extend(0u16.to_le_bytes());
        o.extend(DATE.to_le_bytes());
        o.extend(crc.to_le_bytes());
        o.extend(sat32(packed.len() as u64).to_le_bytes());
        o.extend(sat32(data.len() as u64).to_le_bytes());
        o.extend((name.len() as u16).to_le_bytes());
        // An extra field: the ZIP64 sizes (left at zero, as SketchUp leaves them).
        o.extend(20u16.to_le_bytes());
        o.extend(name.as_bytes());
        o.extend(1u16.to_le_bytes());
        o.extend(16u16.to_le_bytes());
        o.extend([0u8; 16]);
        o.extend(&packed);
        self.entries.push(Entry {
            name: name.to_string(),
            crc,
            size: data.len() as u64,
            compressed: packed.len() as u64,
            local,
        });
    }

    /// The central directory and its end; returns the archive and the marker checksum.
    pub fn finish(mut self) -> (Vec<u8>, u32) {
        let start = self.out.len() as u64;
        let big = start > 0xFFFF_FFFE || self.entries.len() > 0xFFFE;
        for e in &self.entries {
            let o = &mut self.out;
            o.extend(0x0201_4b50u32.to_le_bytes());
            o.extend(0u16.to_le_bytes());
            o.extend(20u16.to_le_bytes());
            o.extend(FLAGS.to_le_bytes());
            o.extend(8u16.to_le_bytes());
            o.extend(0u16.to_le_bytes());
            o.extend(DATE.to_le_bytes());
            o.extend(e.crc.to_le_bytes());
            let far = e.size > 0xFFFF_FFFE || e.compressed > 0xFFFF_FFFE || e.local > 0xFFFF_FFFE;
            let mut extra = Vec::new();
            if far {
                extra.extend(1u16.to_le_bytes());
                extra.extend(24u16.to_le_bytes());
                extra.extend(e.size.to_le_bytes());
                extra.extend(e.compressed.to_le_bytes());
                extra.extend(e.local.to_le_bytes());
            }
            let (c, s, l) =
                if far { (u32::MAX, u32::MAX, u32::MAX) } else { (e.compressed as u32, e.size as u32, e.local as u32) };
            o.extend(c.to_le_bytes());
            o.extend(s.to_le_bytes());
            o.extend((e.name.len() as u16).to_le_bytes());
            o.extend((extra.len() as u16).to_le_bytes());
            // Comment, disk, internal and external attributes.
            o.extend([0u8; 6]);
            o.extend(0u32.to_le_bytes());
            o.extend(l.to_le_bytes());
            o.extend(e.name.as_bytes());
            o.extend(extra);
        }
        let size = self.out.len() as u64 - start;
        let n = self.entries.len() as u64;
        if big {
            let rec = self.out.len() as u64;
            let o = &mut self.out;
            o.extend(0x0606_4b50u32.to_le_bytes());
            o.extend(44u64.to_le_bytes());
            o.extend(45u16.to_le_bytes());
            o.extend(45u16.to_le_bytes());
            o.extend([0u8; 8]);
            o.extend(n.to_le_bytes());
            o.extend(n.to_le_bytes());
            o.extend(size.to_le_bytes());
            o.extend(start.to_le_bytes());
            o.extend(0x0706_4b50u32.to_le_bytes());
            o.extend(0u32.to_le_bytes());
            o.extend(rec.to_le_bytes());
            o.extend(1u32.to_le_bytes());
        }
        let o = &mut self.out;
        o.extend(0x0605_4b50u32.to_le_bytes());
        o.extend([0u8; 4]);
        let n16 = if big { 0xFFFF } else { n as u16 };
        o.extend(n16.to_le_bytes());
        o.extend(n16.to_le_bytes());
        o.extend(sat32(size).to_le_bytes());
        o.extend((if big { u32::MAX } else { start as u32 }).to_le_bytes());
        o.extend(0u16.to_le_bytes());
        let crcs: Vec<u32> = self.entries.iter().map(|e| e.crc).collect();
        (self.out, marker_checksum(&crcs))
    }
}

fn sat32(x: u64) -> u32 {
    u32::try_from(x).unwrap_or(u32::MAX)
}

/// The checksum of the « VFF » marker: the CRC-32 of the complements of the
/// CRCs of the even entries, then the CRCs of the odd entries less 42 (when above 42), each as
/// 4 bytes, in the order of the archive.
pub fn marker_checksum(crcs: &[u32]) -> u32 {
    let mut b = Vec::with_capacity(crcs.len() * 4);
    for c in crcs.iter().step_by(2) {
        b.extend((!c).to_le_bytes());
    }
    for c in crcs.iter().skip(1).step_by(2) {
        let v = if *c > 0x2a { c - 0x2a } else { *c };
        b.extend(v.to_le_bytes());
    }
    crc32(&b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::zip::Archive;

    #[test]
    fn the_archive_reads_back() {
        let mut z = ZipWriter::new();
        z.add("meta/meta.dat", b"abc");
        z.add("model.dat", &vec![7u8; 10_000]);
        let (bytes, _) = z.finish();
        let a = Archive::open(&bytes).unwrap();
        assert_eq!(a.read_name("meta/meta.dat").unwrap().as_slice(), b"abc");
        assert_eq!(a.read_name("model.dat").unwrap().len(), 10_000);
    }
}
