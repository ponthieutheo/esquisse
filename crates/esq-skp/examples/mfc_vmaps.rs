//! Format study: the version maps (`CVersionMap`) of MFC archives, as written, one line per
//! file; with `--declared`, the schema each class is declared with (`FF FF schema len name`)
//! across the files.

use std::collections::BTreeMap;

use esq_skp::native::container::{self, Body};

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let declared = args.iter().position(|a| a == "--declared").map(|i| args.remove(i)).is_some();
    let mut seen: BTreeMap<String, BTreeMap<u16, usize>> = BTreeMap::new();
    for p in &args {
        let data = std::fs::read(p)?;
        let f = container::open(&data)?;
        let Body::Legacy { rest, .. } = &f.body else { continue };
        let (ar, map) = esq_skp::native::legacy::header(rest)?;
        if declared {
            for i in 0..rest.len().saturating_sub(8) {
                if rest[i] == 0xFF && rest[i + 1] == 0xFF {
                    let schema = u16::from_le_bytes([rest[i + 2], rest[i + 3]]);
                    let len = u16::from_le_bytes([rest[i + 4], rest[i + 5]]) as usize;
                    if (4..0x40).contains(&len) && i + 6 + len <= rest.len() {
                        let name = &rest[i + 6..i + 6 + len];
                        if name[0] == b'C' && name.iter().all(|c| c.is_ascii_alphanumeric()) {
                            let name = String::from_utf8_lossy(name).into_owned();
                            *seen.entry(name).or_default().entry(schema).or_default() += 1;
                        }
                    }
                }
            }
            continue;
        }
        let list: Vec<String> = map.iter().map(|(c, v)| format!("(\"{c}\", {v})")).collect();
        println!("{:?} {} classes, modèle à l'octet {}: {}", f.version, map.len(), ar.pos, list.join(", "));
    }
    for (c, s) in &seen {
        println!("{c}: {s:?}");
    }
    Ok(())
}
