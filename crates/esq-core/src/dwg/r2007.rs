//! The R2007 file layout (spec chapter 5): 0x80 bytes of metadata, the file header at
//! 0x80 (0x400 bytes, Reed-Solomon coded, with check data), then the pages — the page
//! map and its copy, the data pages, the section map and its copy — and the file header
//! again at the end. Page offsets count from 0x480.
//!
//! Every page is Reed-Solomon coded, and compressed with the R2007 LZ77 variant when that
//! makes it shorter ("otherwise the original data is used", §5.3, §5.4). The file header
//! record is always compressed: SketchUp's importer (the ODA's libraries) refuses a stored
//! one, with the negative length the specification describes.
//! The data sections are those of R2004 ([`super::r2004::data_sections`]), with the
//! hash codes, page sizes and encodings of the table of §5.2.

use super::objects::Built;
use super::r2004::data_sections;
use super::rs::{
    Code, DATA_K, N, Random, SYSTEM_K, compress, crc64_mirrored, crc64_normal, page_checksum, rotate, update_seed1,
    update_seed2,
};

/// The seed of the random encoder (file header `RandomSeed`) and the CRC seed.
const RANDOM_SEED: u64 = 0;
const CRC_SEED: u64 = 0;

/// Hash code, largest page, encoding (4: interleaved, 1: not) of a section (§5.2).
fn properties(name: &str) -> (u64, usize, u64) {
    match name {
        "AcDb:SummaryInfo" => (0x717A_060F, 0x80, 1),
        "AcDb:Preview" => (0x40AA_0473, 0x400, 1),
        "AcDb:RevHistory" => (0x60A2_05B3, 0x1000, 4),
        "AcDb:AcDbObjects" => (0x674C_05A9, 0xF800, 4),
        "AcDb:ObjFreeSpace" => (0x77E2_061F, 0xF800, 4),
        "AcDb:Template" => (0x4A14_04CE, 0x400, 4),
        "AcDb:Handles" => (0x3F6E_0450, 0xF800, 4),
        "AcDb:Classes" => (0x3F54_045F, 0xF800, 4),
        "AcDb:AuxHeader" => (0x54F0_050A, 0x800, 4),
        _ => (0x32B8_03D9, 0x800, 4), // AcDb:Header
    }
}

/// The order of the section map (§5.2).
const MAP_ORDER: [&str; 10] = [
    "AcDb:Preview",
    "AcDb:SummaryInfo",
    "AcDb:RevHistory",
    "AcDb:AcDbObjects",
    "AcDb:ObjFreeSpace",
    "AcDb:Template",
    "AcDb:Handles",
    "AcDb:Classes",
    "AcDb:AuxHeader",
    "AcDb:Header",
];

fn align(n: usize, to: usize) -> usize {
    n.div_ceil(to) * to
}

/// A coded system page (§5.3): the bytes, their CRCs and how many times the data repeat.
struct System {
    bytes: Vec<u8>,
    size: usize,
    /// Bytes stored: compressed or not.
    stored: usize,
    crc_data: u64,
    crc_stored: u64,
    repeat: u64,
}

/// The page size of §5.3.1: room for the aligned data twice once coded, at least 0x400.
fn system_page_size(len: usize) -> usize {
    let aligned = align(len, 8);
    let size = (aligned * 2).div_ceil(SYSTEM_K) * N;
    if size < 0x400 { 0x400 } else { align(size, 0x20) }
}

/// `data` compressed when that is shorter.
fn packed(data: &[u8]) -> Vec<u8> {
    let c = compress(data);
    if c.len() < data.len() { c } else { data.to_vec() }
}

fn system_page(data: &[u8]) -> System {
    let crc_data = crc64_mirrored(update_seed1(CRC_SEED, data.len()), data);
    let stored = packed(data);
    let crc_stored = crc64_mirrored(update_seed1(CRC_SEED, stored.len()), &stored);
    let size = system_page_size(data.len());
    let aligned = align(stored.len(), 8);
    let repeat = ((size / N) * SYSTEM_K / aligned.max(1)).max(1);
    let mut buf = Vec::with_capacity(aligned * repeat);
    for _ in 0..repeat {
        buf.extend_from_slice(&stored);
        buf.resize(buf.len() + aligned - stored.len(), 0);
    }
    let mut bytes = Code::new(SYSTEM_K).interleaved(&buf);
    bytes.resize(size, 0);
    System { bytes, size, stored: stored.len(), crc_data, crc_stored, repeat: repeat as u64 }
}

pub(super) fn write(b: &Built) -> Vec<u8> {
    write_sections(b.version, data_sections(b))
}

/// Lays out `sections` (in the order of the file).
pub(super) fn write_sections(v: super::DwgVersion, sections: Vec<super::r2004::DataSection>) -> Vec<u8> {
    let data_code = Code::new(DATA_K);
    let padding = Random::new(RANDOM_SEED).padding(N);

    // Data pages, coded, with their ids from 3 on (the page map and its copy are 1, 2).
    struct DataPage {
        id: u64,
        bytes: Vec<u8>,
    }
    let mut pages: Vec<DataPage> = Vec::new();
    let mut map_entries: Vec<(&'static str, Vec<u8>)> = Vec::new();
    let mut first_page: std::collections::HashMap<&str, u64> = std::collections::HashMap::new();
    let mut next_id = 3u64;
    for s in sections {
        let (hash, page, encoding) = properties(s.name);
        let mut entry = Vec::new();
        let mut records = Vec::new();
        let mut start = 0;
        while start < s.data.len() {
            let end = (start + page).min(s.data.len());
            let chunk = &s.data[start..end];
            // Encoding 4 sections are compressed; then aligned on 8 bytes and coded.
            let stored = if encoding == 4 { packed(chunk) } else { chunk.to_vec() };
            let mut aligned = stored.clone();
            aligned.resize(align(stored.len(), 8), 0);
            let mut bytes =
                if encoding == 4 { data_code.interleaved(&aligned) } else { data_code.sequential(&aligned, &padding) };
            bytes.resize(align(bytes.len(), 0x20), 0);
            first_page.entry(s.name).or_insert(next_id);
            let crc = crc64_mirrored(update_seed1(CRC_SEED, stored.len()), &stored);
            for x in [start as u64, page as u64, next_id, chunk.len() as u64, stored.len() as u64] {
                records.extend(x.to_le_bytes());
            }
            records.extend(u64::from(page_checksum(0, chunk)).to_le_bytes());
            records.extend(crc.to_le_bytes());
            pages.push(DataPage { id: next_id, bytes });
            next_id += 1;
            start = end;
        }
        let name: Vec<u8> = s.name.encode_utf16().chain([0]).flat_map(|u| u.to_le_bytes()).collect();
        let count = records.len() / 56;
        for x in [s.data.len() as u64, page as u64, 0, hash, name.len() as u64, 0, encoding, count as u64] {
            entry.extend(x.to_le_bytes());
        }
        entry.extend(name);
        entry.extend(records);
        map_entries.push((s.name, entry));
    }
    let mut section_map = Vec::new();
    for name in MAP_ORDER {
        if let Some((_, e)) = map_entries.iter().find(|(n, _)| *n == name) {
            section_map.extend_from_slice(e);
        }
    }
    // The empty section ends the map, as the ODA writes it; it is counted.
    for x in [0u64, 0xF800, 0, 0, 0, 0, 4, 0] {
        section_map.extend(x.to_le_bytes());
    }
    let sections_amount = map_entries.len() as u64 + 1;
    let section_map_page = system_page(&section_map);
    let (section_map_id, section_map2_id) = (next_id, next_id + 1);

    // The page map: every page in the order of the file.
    let page_count = 2 + pages.len() + 2;
    let page_map_len = page_count * 16;
    let page_map_size = system_page_size(page_map_len);
    let mut page_map = Vec::with_capacity(page_map_len);
    let mut entry = |size: usize, id: u64| {
        page_map.extend((size as u64).to_le_bytes());
        page_map.extend(id.to_le_bytes());
    };
    entry(page_map_size, 1);
    entry(page_map_size, 2);
    for p in &pages {
        entry(p.bytes.len(), p.id);
    }
    entry(section_map_page.size, section_map_id);
    entry(section_map_page.size, section_map2_id);
    let page_map_page = system_page(&page_map);
    debug_assert_eq!(page_map_page.size, page_map_size);

    // The file.
    let mut f = vec![0u8; 0x480];
    let mut offset_of = std::collections::HashMap::new();
    for _ in 0..2 {
        f.extend_from_slice(&page_map_page.bytes);
    }
    for p in &pages {
        offset_of.insert(p.id, f.len());
        f.extend_from_slice(&p.bytes);
    }
    for _ in 0..2 {
        f.extend_from_slice(&section_map_page.bytes);
    }
    let header2 = f.len();
    let file_size = header2 + 0x400;

    // The file header (§5.2).
    let mut h = [0u64; 34];
    let set = |h: &mut [u64; 34], at: usize, x: u64| h[at / 8] = x;
    set(&mut h, 0x00, 0x70);
    set(&mut h, 0x08, file_size as u64);
    set(&mut h, 0x10, page_map_page.crc_stored);
    set(&mut h, 0x18, page_map_page.repeat);
    set(&mut h, 0x28, page_map_size as u64); // the copy, right after
    set(&mut h, 0x30, 2);
    set(&mut h, 0x38, 0);
    set(&mut h, 0x40, 1);
    set(&mut h, 0x48, (header2 - 0x480) as u64);
    set(&mut h, 0x50, page_map_page.stored as u64);
    set(&mut h, 0x58, page_map.len() as u64);
    set(&mut h, 0x60, page_count as u64);
    set(&mut h, 0x68, section_map2_id);
    set(&mut h, 0x70, 0x20);
    set(&mut h, 0x78, 0x40);
    set(&mut h, 0x80, page_map_page.crc_data);
    set(&mut h, 0x88, 0xF800);
    set(&mut h, 0x90, 4);
    set(&mut h, 0x98, 1);
    set(&mut h, 0xA0, sections_amount);
    set(&mut h, 0xA8, section_map_page.crc_data);
    set(&mut h, 0xB0, section_map_page.stored as u64);
    set(&mut h, 0xB8, section_map2_id);
    set(&mut h, 0xC0, section_map_id);
    set(&mut h, 0xC8, section_map.len() as u64);
    set(&mut h, 0xD0, section_map_page.crc_stored);
    set(&mut h, 0xD8, section_map_page.repeat);
    set(&mut h, 0xE8, 0x60100);
    set(&mut h, 0xF0, CRC_SEED);
    set(&mut h, 0x100, RANDOM_SEED);
    let page = header_page(h);
    f[0x80..0x480].copy_from_slice(&page);
    f.extend_from_slice(&page);

    // Metadata (§5.2).
    f[..6].copy_from_slice(v.magic().as_bytes());
    f[0x0B] = v.maintenance();
    f[0x0C] = 3;
    let at = |name: &str| first_page.get(name).and_then(|id| offset_of.get(id)).map_or(0, |&o| o as u32);
    f[0x0D..0x11].copy_from_slice(&at("AcDb:Preview").to_le_bytes());
    f[0x11] = v.number();
    f[0x13..0x15].copy_from_slice(&30u16.to_le_bytes()); // ANSI_1252
    f[0x20..0x24].copy_from_slice(&at("AcDb:SummaryInfo").to_le_bytes());
    f[0x28..0x2C].copy_from_slice(&0x80u32.to_le_bytes());
    f
}

/// The file header page (§5.2.1) of the record `h` (its seeds, check data and CRC are
/// filled here).
pub(super) fn header_page(mut h: [u64; 34]) -> Vec<u8> {
    let set = |h: &mut [u64; 34], at: usize, x: u64| h[at / 8] = x;
    let crc_seed = h[0xF0 / 8];
    // The encoded seeds and the check data, in the order of §5.2.1.1.
    let mut rng = Random::new(h[0x100 / 8]);
    set(&mut h, 0xE0, rng.encode(crc_seed as u32));
    set(&mut h, 0x20, rng.encode(crc_seed as u32));
    let r1 = rng.next();
    let r2 = rng.next();
    let encoded_seed = rng.encode(crc_seed as u32);
    let words = |b: [u64; 8]| b.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
    let normal = {
        let mut b = [0u64; 8];
        b[0] = rotate(r1, r2);
        b[1] = rotate(b[0], b[0]);
        b[2] = rotate(r2, b[1]);
        b[3] = rotate(b[2], b[2]);
        b[4] = rotate(r1, b[3]);
        b[5] = rotate(b[4], b[4]);
        b[6] = rotate(b[5], b[5]);
        b[7] = rotate(b[6], b[6]);
        crc64_normal(!r2, &words(b))
    };
    let mirrored = {
        let mut b = [0u64; 8];
        b[0] = rotate(r1, r2);
        b[1] = rotate(normal, b[0]);
        b[2] = rotate(r2, b[1]);
        b[3] = rotate(normal, b[2]);
        b[4] = rotate(r1, b[3]);
        b[5] = rotate(normal, b[4]);
        b[6] = rotate(r2, b[5]);
        b[7] = rotate(b[6], b[6]);
        crc64_mirrored(!r1, &words(b))
    };
    set(&mut h, 0xF8, rng.encode(crc_seed as u32));
    set(&mut h, 0x108, 0);
    let mut record: Vec<u8> = h.iter().flat_map(|x| x.to_le_bytes()).collect();
    let header_crc = crc64_normal(update_seed2(0, record.len()), &record);
    record[0x108..0x110].copy_from_slice(&header_crc.to_le_bytes());

    // The header page (§5.2.1.3–7): stored uncompressed (a negative length), its CRC, the
    // checking sequence, repeated as often as it fits in 3 × 239 bytes, coded, padded,
    // the check data at the end.
    // Compressed when that is shorter, else stored with a negative length (§5.2.1.3).
    let comp = compress(&record);
    let (packed, len) = if comp.len() < record.len() {
        (comp.clone(), comp.len() as i32)
    } else {
        (record.clone(), -(record.len() as i32))
    };
    let stored_crc = crc64_normal(update_seed2(0, packed.len()), &packed);
    let seq1 = rng.next();
    let seq2 = rotate(seq1, seq1);
    let seq: Vec<u8> = [seq1, seq2].iter().flat_map(|x| x.to_le_bytes()).collect();
    let seq_crc = crc64_normal(update_seed1(0, seq.len()), &seq);
    let mut block = Vec::new();
    block.extend(seq_crc.to_le_bytes());
    block.extend(seq1.to_le_bytes());
    block.extend(stored_crc.to_le_bytes());
    block.extend(len.to_le_bytes());
    block.extend(0i32.to_le_bytes());
    block.extend_from_slice(&packed);
    let pad = align(block.len(), 8) - block.len();
    block.extend(rng.padding(pad));
    let room = 3 * SYSTEM_K;
    let mut pre = Vec::with_capacity(room);
    while pre.len() + block.len() <= room {
        pre.extend_from_slice(&block);
    }
    pre.extend(rng.padding(room - pre.len()));
    let mut page = Code::new(SYSTEM_K).interleaved(&pre);
    page.extend(rng.padding(0x3D8 - page.len()));
    for x in [normal, mirrored, r1, r2, encoded_seed] {
        page.extend(x.to_le_bytes());
    }
    debug_assert_eq!(page.len(), 0x400);
    page
}
