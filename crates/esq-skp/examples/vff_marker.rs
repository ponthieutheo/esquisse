//! Format study: the archive of 2021+ files — each entry (size, picture size of the PNGs) and
//! whether the checksum of the « VFF » marker is the one the native writer computes.
//!
//! `cargo run -p esq-skp --example vff_marker -- files…`

use esq_skp::native::container::{self, Body};

fn png_size(d: &[u8]) -> Option<(u32, u32)> {
    if d.len() < 24 || &d[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    Some((u32::from_be_bytes(d[16..20].try_into().ok()?), u32::from_be_bytes(d[20..24].try_into().ok()?)))
}

fn main() -> anyhow::Result<()> {
    for p in std::env::args().skip(1) {
        let data = std::fs::read(&p)?;
        let f = container::open(&data)?;
        let Body::Zip { archive, marker } = &f.body else { continue };
        let want = u32::from_le_bytes(marker[9..13].try_into()?);
        let crcs: Vec<u32> = archive.entries.iter().map(|x| x.crc).collect();
        let got = esq_skp::native::write::zip::marker_checksum(&crcs);
        println!("{p}: marqueur {want:08x} {}", if got == want { "=" } else { "≠" });
        for e in &archive.entries {
            let size = if e.name.ends_with(".png") {
                archive.read_name(&e.name).ok().and_then(|d| png_size(&d)).map(|(w, h)| format!(" {w}×{h}"))
            } else {
                None
            };
            println!("   {} {}{}", e.name, e.size, size.unwrap_or_default());
        }
    }
    Ok(())
}
