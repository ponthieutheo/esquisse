//! The R2004 file layout (spec chapter 4), also that of 2010, 2013 and 2018 (chapters
//! 6–8): a 0x100-byte file header whose 0x6C bytes at 0x80 are masked, then pages
//! aligned on 0x20 bytes — the data sections cut into pages of at most 0x7400 bytes
//! (LZ77-compressed except the summary and the preview), the section map and the page
//! map — and a copy of the masked header at the end. Object map offsets are offsets in
//! the AcDb:AcDbObjects section.
//!
//! Where the specification and AutoCAD's files differ, the files are followed: the
//! "page size" of a data page header is the size of the page in the file, and its two
//! checksums are at 0x18 (header) and 0x1C (data).

use super::DwgVersion;
use super::bits::crc32;
use super::lz77;
use super::objects::{Built, object_map};

/// The masking sequence of §4.1 (and the padding between pages): the LCG of the spec.
pub(super) fn magic() -> [u8; 0x100] {
    let mut out = [0u8; 0x100];
    let mut seed: u32 = 1;
    for b in &mut out {
        seed = seed.wrapping_mul(0x343FD).wrapping_add(0x269EC3);
        *b = (seed >> 16) as u8;
    }
    out
}

/// The page checksum of §4.2.
pub(super) fn checksum(seed: u32, data: &[u8]) -> u32 {
    let (mut s1, mut s2) = (seed & 0xFFFF, seed >> 16);
    for chunk in data.chunks(0x15B0) {
        for &b in chunk {
            s1 += u32::from(b);
            s2 += s1;
        }
        s1 %= 0xFFF1;
        s2 %= 0xFFF1;
    }
    (s2 << 16) | (s1 & 0xFFFF)
}

/// The data sections, in the order of the file (§4.5).
pub(super) struct DataSection {
    pub name: &'static str,
    pub data: Vec<u8>,
    /// Largest page.
    pub page: usize,
    pub compressed: bool,
    /// Section id: 1 for the header, then upwards against the order of the file.
    pub id: u32,
}

/// The sections every file has, in their order in the file.
pub(super) fn data_sections(b: &Built) -> Vec<DataSection> {
    let v = b.version;
    let mut objects = 0x0DCAu32.to_le_bytes().to_vec(); // "meaning unknown" (§20)
    let mut map = Vec::with_capacity(b.objects.len());
    for (h, bytes) in &b.objects {
        map.push((*h, objects.len() as u64));
        objects.extend_from_slice(bytes);
    }
    let list: Vec<(&'static str, Vec<u8>, usize, bool)> = vec![
        ("AcDb:SummaryInfo", summary_info(b), 0x100, false),
        ("AcDb:Preview", b.preview(), 0x400, false),
        ("AcDb:RevHistory", rev_history(v), 0x7400, true),
        ("AcDb:AcDbObjects", objects, 0x7400, true),
        ("AcDb:ObjFreeSpace", obj_free_space(b), 0x7400, true),
        ("AcDb:Template", b.template(), 0x7400, true),
        ("AcDb:Handles", object_map(&map), 0x7400, true),
        ("AcDb:Classes", b.classes.clone(), 0x7400, true),
        ("AcDb:AuxHeader", aux_header(b), 0x7400, true),
        ("AcDb:Header", b.header.clone(), 0x7400, true),
    ];
    let n = list.len() as u32;
    list.into_iter()
        .enumerate()
        .map(|(i, (name, data, page, compressed))| DataSection { name, data, page, compressed, id: n - i as u32 })
        .collect()
}

/// AcDb:SummaryInfo (chapter 13): eight empty strings (a 16-bit length counting the
/// terminating zero, then the characters: 8-bit before 2007, UTF-16 from 2007), the
/// editing time, the creation and modification dates, no properties.
fn summary_info(b: &Built) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..8 {
        out.extend(1u16.to_le_bytes());
        if b.version >= DwgVersion::R2007 {
            out.extend(0u16.to_le_bytes());
        } else {
            out.push(0);
        }
    }
    out.extend([0u8; 8]); // total editing time
    for _ in 0..2 {
        out.extend(b.date.0.to_le_bytes());
        out.extend(b.date.1.to_le_bytes());
    }
    out.extend(0u16.to_le_bytes()); // properties
    out.extend([0u8; 8]);
    out
}

/// AcDb:RevHistory (chapter 18), as the ODA writes it.
fn rev_history(v: DwgVersion) -> Vec<u8> {
    let words: &[u32] = if v >= DwgVersion::R2007 { &[0, 0, 1, 0] } else { &[0, 0, 0] };
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// AcDb:ObjFreeSpace (chapter 21): the count of objects, the date, then four pairs as
/// the ODA writes them; 32-bit fields up to 2007, 64-bit ones from 2010 as AutoCAD writes them.
fn obj_free_space(b: &Built) -> Vec<u8> {
    let mut out = Vec::new();
    let count = b.objects.len() as u64;
    let pairs: [u64; 4] = [0x32, 0x64, 0x200, 0xFFFF_FFFF];
    if b.version >= DwgVersion::R2010 {
        out.extend(0u64.to_le_bytes());
        out.extend(count.to_le_bytes());
        out.extend(b.date.0.to_le_bytes());
        out.extend(b.date.1.to_le_bytes());
        out.push(4);
        for p in pairs {
            out.extend(p.to_le_bytes());
            out.extend(0u64.to_le_bytes());
        }
    } else {
        out.extend(0u32.to_le_bytes());
        out.extend((count as u32).to_le_bytes());
        out.extend(b.date.0.to_le_bytes());
        out.extend(b.date.1.to_le_bytes());
        out.extend(0u32.to_le_bytes()); // the objects start the AcDb:AcDbObjects section
        out.push(4);
        for p in pairs {
            out.extend((p as u32).to_le_bytes());
            out.extend(0u32.to_le_bytes());
        }
    }
    out
}

/// AcDb:AuxHeader (chapter 27). The maintenance releases are longs from 2018, as in
/// AutoCAD's files.
fn aux_header(b: &Built) -> Vec<u8> {
    let v = b.version;
    let mut out = vec![0xFF, 0x77, 0x01];
    let ver = u16::from(v.number());
    let maint = |out: &mut Vec<u8>| {
        if v >= DwgVersion::R2018 {
            out.extend(u32::from(v.maintenance()).to_le_bytes());
        } else {
            out.extend(u16::from(v.maintenance()).to_le_bytes());
        }
    };
    out.extend(ver.to_le_bytes());
    maint(&mut out);
    out.extend(1u32.to_le_bytes()); // saves
    out.extend((-1i32).to_le_bytes());
    out.extend(1u16.to_le_bytes()); // saves, part 1
    out.extend(0u16.to_le_bytes()); // part 2
    out.extend(0u32.to_le_bytes());
    for _ in 0..2 {
        out.extend(ver.to_le_bytes());
        maint(&mut out);
    }
    for w in [0x0005u16, 0x0893, 0x0005, 0x0893, 0x0000, 0x0001] {
        out.extend(w.to_le_bytes());
    }
    out.extend([0u8; 20]);
    for _ in 0..2 {
        out.extend(b.date.0.to_le_bytes()); // TDCREATE, TDUPDATE
        out.extend(b.date.1.to_le_bytes());
    }
    let seed = if b.handseed < 0x7FFF_FFFF { b.handseed as i32 } else { -1 };
    out.extend(seed.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // educational plot stamp
    out.extend(0u16.to_le_bytes());
    out.extend(1u16.to_le_bytes()); // saves part 1 - part 2
    out.extend([0u8; 12]);
    out.extend(1u32.to_le_bytes()); // saves
    out.extend([0u8; 16]);
    out
}

/// A page laid in the file, for the page map.
struct Page {
    number: u32,
    size: usize,
}

struct File {
    f: Vec<u8>,
    pages: Vec<Page>,
    magic: [u8; 0x100],
}

impl File {
    /// Pads to the next 0x20 boundary with the masking sequence (§4.3).
    fn align(&mut self) {
        let pad = (0x20 - self.f.len() % 0x20) % 0x20;
        let m = self.magic;
        self.f.extend_from_slice(&m[..pad]);
    }

    /// A data page (§4.6): masked header, data, padding.
    fn data_page(&mut self, section: u32, start: usize, payload: &[u8]) -> (u32, usize) {
        let at = self.f.len();
        let number = self.pages.len() as u32 + 1;
        let size = (32 + payload.len()).div_ceil(0x20) * 0x20;
        let data_sum = checksum(0, payload);
        let mut words = [0x4163_043B, section, payload.len() as u32, size as u32, start as u32, 0, 0, data_sum];
        let bytes = |w: &[u32; 8]| w.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        words[6] = checksum(data_sum, &bytes(&words));
        let mask = 0x4164_536B ^ at as u32;
        for w in &mut words {
            *w ^= mask;
        }
        self.f.extend(bytes(&words));
        self.f.extend_from_slice(payload);
        self.align();
        self.pages.push(Page { number, size });
        (number, payload.len())
    }

    /// A system page (§4.3): header, compressed data, the header again with no sizes
    /// and no checksum (as AutoCAD writes it), padding to `size` (at least).
    fn system_page(&mut self, kind: u32, number: u32, data: &[u8], size: usize) {
        let at = self.f.len();
        let comp = lz77::compress(data);
        let mut header = [kind, data.len() as u32, comp.len() as u32, 2, 0];
        let bytes = |w: &[u32; 5]| w.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        header[4] = checksum(checksum(0, &bytes(&header)), &comp);
        self.f.extend(bytes(&header));
        self.f.extend(&comp);
        self.f.extend(bytes(&[kind, 0, 0, 2, 0]));
        let m = self.magic;
        let pad = (at + size).saturating_sub(self.f.len());
        self.f.extend(m.iter().cycle().take(pad));
        self.align();
        self.pages.push(Page { number, size: self.f.len() - at });
    }
}

/// The size a system page of `data` takes.
fn system_size(data: &[u8]) -> usize {
    (0x28 + lz77::compress(data).len()).div_ceil(0x20) * 0x20
}

/// A page of a data section: its number, its size in the file, where its data starts.
type PageRef = (u32, usize, usize);

pub(super) fn write(b: &Built) -> Vec<u8> {
    let v = b.version;
    let mut file = File { f: vec![0u8; 0x100], pages: Vec::new(), magic: magic() };
    let sections = data_sections(b);

    // Data pages, and the section map entries.
    let mut entries: Vec<(&DataSection, Vec<PageRef>)> = Vec::new();
    let mut first_page_at = std::collections::HashMap::new();
    for s in &sections {
        let mut pages = Vec::new();
        let mut start = 0;
        while start < s.data.len() {
            let end = (start + s.page).min(s.data.len());
            let chunk = &s.data[start..end];
            let payload = if s.compressed {
                lz77::compress(chunk)
            } else {
                let mut p = chunk.to_vec();
                p.resize(s.page, 0);
                p
            };
            first_page_at.entry(s.name).or_insert(file.f.len());
            let (number, size) = file.data_page(s.id, start, &payload);
            pages.push((number, size, start));
            start = end;
        }
        entries.push((s, pages));
    }
    let data_pages = file.pages.len() as u32;

    // The section map (§4.5): the empty section, then the others by decreasing id.
    let mut map = Vec::new();
    let count = sections.len() as u32 + 1;
    for w in [count, 2, 0x7400, 0, count] {
        map.extend(w.to_le_bytes());
    }
    let mut describe = |name: &str, size: u64, pages: &[PageRef], page: usize, comp: bool, id: u32| {
        map.extend(size.to_le_bytes());
        map.extend((pages.len() as u32).to_le_bytes());
        map.extend((page as u32).to_le_bytes());
        map.extend(1u32.to_le_bytes());
        map.extend(if comp { 2u32 } else { 1 }.to_le_bytes());
        map.extend(id.to_le_bytes());
        map.extend(0u32.to_le_bytes()); // not encrypted
        let mut n = [0u8; 64];
        n[..name.len()].copy_from_slice(name.as_bytes());
        map.extend(n);
        for &(number, size, start) in pages {
            map.extend(number.to_le_bytes());
            map.extend((size as u32).to_le_bytes());
            map.extend((start as u64).to_le_bytes());
        }
    };
    describe("", 0, &[], 0x7400, true, 0);
    let mut by_id: Vec<&(&DataSection, Vec<PageRef>)> = entries.iter().collect();
    by_id.sort_by_key(|e| std::cmp::Reverse(e.0.id));
    for (s, pages) in by_id {
        describe(s.name, s.data.len() as u64, pages, s.page, s.compressed, s.id);
    }

    // The system pages: numbered after the data pages with a gap of one (§4.4).
    let section_map_id = data_pages + 2;
    let page_map_id = data_pages + 3;
    file.system_page(0x4163_003B, section_map_id, &map, 0);
    let page_map_at = file.f.len();
    // The page map lists its own page: grow its size until the data fits.
    let page_map = |pages: &[Page], own: usize| -> Vec<u8> {
        let mut d = Vec::new();
        for p in pages {
            d.extend(p.number.to_le_bytes());
            d.extend((p.size as u32).to_le_bytes());
        }
        d.extend(page_map_id.to_le_bytes());
        d.extend((own as u32).to_le_bytes());
        d
    };
    let mut own = 0x20;
    loop {
        let need = system_size(&page_map(&file.pages, own));
        if need <= own {
            break;
        }
        own = need;
    }
    let data = page_map(&file.pages, own);
    file.system_page(0x4163_0E3B, page_map_id, &data, own);
    let end = file.f.len();
    debug_assert_eq!(end - page_map_at, own);

    // The file header data (§4.1), at 0x80 and at the end.
    let mut h = vec![0u8; 0x6C];
    h[..12].copy_from_slice(b"AcFssFcAJMB\0");
    let put32 = |h: &mut Vec<u8>, at: usize, x: u32| h[at..at + 4].copy_from_slice(&x.to_le_bytes());
    let put64 = |h: &mut Vec<u8>, at: usize, x: u64| h[at..at + 8].copy_from_slice(&x.to_le_bytes());
    put32(&mut h, 0x10, 0x6C);
    put32(&mut h, 0x14, 0x04);
    put32(&mut h, 0x24, 1);
    put32(&mut h, 0x28, page_map_id); // last page
    put64(&mut h, 0x2C, (end - 0x100) as u64);
    put64(&mut h, 0x34, end as u64); // second header
    put32(&mut h, 0x40, file.pages.len() as u32);
    put32(&mut h, 0x44, 0x20);
    put32(&mut h, 0x48, 0x80);
    put32(&mut h, 0x4C, 0x40);
    put32(&mut h, 0x50, page_map_id);
    put64(&mut h, 0x54, (page_map_at - 0x100) as u64);
    put32(&mut h, 0x5C, section_map_id);
    put32(&mut h, 0x60, page_map_id);
    let crc = crc32(0, &h);
    put32(&mut h, 0x68, crc);
    let m = file.magic;
    for (x, k) in h.iter_mut().zip(m.iter()) {
        *x ^= k;
    }
    let mut f = file.f;
    f.extend_from_slice(&h);
    f[0x80..0x80 + 0x6C].copy_from_slice(&h);
    f[0xEC..0x100].copy_from_slice(&m[0xEC..0x100]);

    // The file header (§4.1).
    f[..6].copy_from_slice(v.magic().as_bytes());
    f[0x0B] = v.maintenance();
    f[0x0C] = 3;
    let preview = first_page_at.get("AcDb:Preview").map_or(0, |a| a + 0x20);
    f[0x0D..0x11].copy_from_slice(&(preview as u32).to_le_bytes());
    f[0x11] = v.number();
    f[0x12] = 0;
    f[0x13..0x15].copy_from_slice(&30u16.to_le_bytes()); // ANSI_1252
    let summary = first_page_at.get("AcDb:SummaryInfo").map_or(0, |a| a + 0x20);
    f[0x20..0x24].copy_from_slice(&(summary as u32).to_le_bytes());
    f[0x28..0x2C].copy_from_slice(&0x80u32.to_le_bytes());
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masking_sequence_and_checksum() {
        let m = magic();
        assert_eq!(m[..8], [0x29, 0x23, 0xBE, 0x84, 0xE1, 0x6C, 0xD6, 0xAE]);
        // The spec prints 0x5C bytes; its last row starts at 0x50.
        assert_eq!(m[0x50..0x5C], [0xAD, 0x4F, 0x14, 0xF2, 0x44, 0x40, 0x66, 0xD0, 0x6B, 0xC4, 0x30, 0xB7]);
        // Adler-32 with a zero seed differs from zlib's (seed 1) by the initial sums.
        assert_eq!(checksum(1, b"Wikipedia"), 0x11E6_0398);
    }
}
