//! The R13–R2000 file layout (spec chapter 3): a file header with five section locators,
//! then the header variables, the classes, 0x200 bytes of padding, the objects, the
//! object map, the "unknown" section (chapter 25), the template and the preview. Object
//! map offsets are file offsets.

use super::DwgVersion;
use super::bits::crc16;
use super::objects::{Built, object_map};

const FILE_HEADER_END: [u8; 16] =
    [0x95, 0xA0, 0x4E, 0x28, 0x99, 0x82, 0x1A, 0xE5, 0x5E, 0x41, 0xE0, 0x5F, 0x9D, 0x3A, 0x4D, 0x00];

/// The "known to be valid" content of the unknown section (spec chapter 25).
const UNKNOWN_SECTION: [u8; 45] = [
    0xA7, 0x62, 0x25, 0x00, 0xF6, 0xAF, 0x25, 0x02, 0x3B, 0x04, 0x00, 0x00, 0x04, 0x32, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF,
    0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00,
];

pub(super) fn write(b: &Built) -> Vec<u8> {
    const RECORDS: usize = 5;
    let mut f = vec![0u8; 0x19 + 9 * RECORDS + 2 + 16];
    let header_at = f.len();
    f.extend_from_slice(&b.header);
    let classes_at = f.len();
    f.extend_from_slice(&b.classes);
    f.extend_from_slice(&[0u8; 0x200]);
    let objects_at = f.len();
    let mut map: Vec<(u64, u64)> = Vec::with_capacity(b.objects.len());
    for (h, bytes) in &b.objects {
        map.push((*h, f.len() as u64));
        f.extend_from_slice(bytes);
    }
    let map_at = f.len();
    f.extend(object_map(&map));
    let map_size = f.len() - map_at;
    let unknown_at = f.len();
    f.extend(0u32.to_le_bytes());
    f.extend((map.len() as u32 + 3).to_le_bytes());
    let mut unknown = UNKNOWN_SECTION;
    unknown[8..12].copy_from_slice(&(objects_at as u32).to_le_bytes());
    f.extend_from_slice(&unknown);
    let template_at = f.len();
    let template = b.template();
    f.extend_from_slice(&template);
    let image_at = f.len();
    f.extend(b.preview());

    // File header (spec §3.2).
    f[..6].copy_from_slice(b.version.magic().as_bytes());
    f[0x0C] = 1;
    f[0x0D..0x11].copy_from_slice(&(image_at as u32).to_le_bytes());
    f[0x13..0x15].copy_from_slice(&30u16.to_le_bytes()); // ANSI_1252
    f[0x15..0x19].copy_from_slice(&(RECORDS as u32).to_le_bytes());
    let records = [
        (header_at, b.header.len()),
        (classes_at, b.classes.len()),
        (map_at, map_size),
        (unknown_at, 53),
        (template_at, template.len()),
    ];
    for (i, (at, size)) in records.iter().enumerate() {
        let r = 0x19 + 9 * i;
        f[r] = i as u8;
        f[r + 1..r + 5].copy_from_slice(&(*at as u32).to_le_bytes());
        f[r + 5..r + 9].copy_from_slice(&(*size as u32).to_le_bytes());
    }
    let crc_at = 0x19 + 9 * RECORDS;
    let crc = crc16(0, &f[..crc_at]) ^ 0x3CC4;
    f[crc_at..crc_at + 2].copy_from_slice(&crc.to_le_bytes());
    f[crc_at + 2..crc_at + 18].copy_from_slice(&FILE_HEADER_END);
    debug_assert!(matches!(b.version, DwgVersion::R13 | DwgVersion::R14 | DwgVersion::R2000));
    f
}
