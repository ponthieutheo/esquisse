//! Shows what a .skp holds, as the native reader sees it (format study, `docs/skp-format.md`).
//!
//! `cargo run -p esq-skp --example skp_dump -- file.skp [entry|--tree|--extract dir]`
//! - no option: header and ZIP entries, then the records of `model.dat` (2021+);
//! - `entry`: dumps one entry of the archive (records if it is made of records, else hex);
//! - `--extract dir`: writes every entry of the archive into `dir`.

use esq_skp::native::container::{self, Body};
use esq_skp::native::tlv::{Records, is_records};

fn hex(d: &[u8], max: usize) -> String {
    let mut s: String = d.iter().take(max).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
    if d.len() > max {
        s.push_str(" …");
    }
    s
}

fn value(d: &[u8]) -> String {
    let mut s = format!("[{}] {}", d.len(), hex(d, 48));
    match d.len() {
        1 => s.push_str(&format!("  = {}", d[0])),
        2 => s.push_str(&format!("  = {}", u16::from_le_bytes([d[0], d[1]]))),
        4 => s.push_str(&format!(
            "  = {} / {}f",
            i32::from_le_bytes(d.try_into().unwrap()),
            f32::from_le_bytes(d.try_into().unwrap())
        )),
        8 => s.push_str(&format!(
            "  = {} / {}",
            i64::from_le_bytes(d.try_into().unwrap()),
            f64::from_le_bytes(d.try_into().unwrap())
        )),
        n if n % 8 == 0 && n <= 128 => {
            let f: Vec<String> = d.as_chunks::<8>().0.iter().map(|c| format!("{}", f64::from_le_bytes(*c))).collect();
            s.push_str(&format!("  = ({})", f.join(", ")));
        }
        _ => {}
    }
    if d.len() >= 4 {
        let n = u32::from_le_bytes(d[..4].try_into().unwrap()) as usize;
        if n + 4 == d.len() && d[4..].iter().all(|&b| (0x20..0x7f).contains(&b) || b >= 0x80) {
            s.push_str(&format!("  = \"{}\"", String::from_utf8_lossy(&d[4..])));
        }
    }
    if d.len() > 1 && d.iter().all(|&b| (0x20..0x7f).contains(&b)) {
        s.push_str(&format!("  = '{}'", String::from_utf8_lossy(d)));
    }
    s
}

fn tree(out: &mut String, d: &[u8], base: usize, depth: usize) {
    use std::fmt::Write;
    let ind = "  ".repeat(depth);
    for r in Records::new(d, base) {
        if depth < 40 && is_records(r.data) {
            let _ = writeln!(out, "{ind}{:04x} @{:x} {{{}}}", r.tag, r.at, r.data.len());
            tree(out, r.data, r.at + 6, depth + 1);
        } else {
            let _ = writeln!(out, "{ind}{:04x} @{:x} {}", r.tag, r.at, value(r.data));
        }
    }
}

/// `--batch out_dir files…`: one dump of `model.dat` per file, named `<folder>_<stem>.txt`.
fn batch(args: &[String]) -> anyhow::Result<()> {
    let dir = std::path::PathBuf::from(&args[0]);
    std::fs::create_dir_all(&dir)?;
    for p in &args[1..] {
        let p = std::path::Path::new(p);
        let data = std::fs::read(p)?;
        let f = container::open(&data)?;
        let mut out = format!("{} {}\n", f.magic, f.version_text);
        match &f.body {
            Body::Zip { archive, .. } => {
                for e in &archive.entries {
                    out.push_str(&format!("  {:>9} {}\n", e.size, e.name));
                }
                tree(&mut out, &archive.read_name("model.dat")?, 0, 0);
            }
            Body::Legacy { rest, .. } => out.push_str(&format!("{} octets\n{}\n", rest.len(), hex(rest, 1 << 20))),
        }
        let folder = p.parent().and_then(|x| x.file_name()).and_then(|x| x.to_str()).unwrap_or("x");
        let stem = p.file_stem().and_then(|x| x.to_str()).unwrap_or("x");
        std::fs::write(dir.join(format!("{folder}_{stem}.txt")), out)?;
    }
    Ok(())
}

/// `--check files…`: one line per file, whether the native reader reads it (bytes left over
/// after the model of an MFC archive).
fn check(args: &[String]) -> anyhow::Result<()> {
    let (mut ok, mut total) = (0, 0);
    for p in args {
        total += 1;
        let data = std::fs::read(p)?;
        let f = container::open(&data)?;
        let res = match &f.body {
            Body::Legacy { guid, rest } => {
                esq_skp::native::legacy::read_counted(rest, guid, f.version, &Default::default()).map(|(_, l)| l)
            }
            Body::Zip { .. } => esq_skp::native::read_doc(&data, &Default::default()).map(|_| 0),
        };
        match res {
            Ok(left) => {
                ok += 1;
                println!("ok {:?} {left} {p}", f.version);
            }
            Err(e) => println!("ÉCHEC {:?} {p} : {e}", f.version),
        }
    }
    println!("{ok}/{total}");
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--batch") {
        return batch(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("--check") {
        return check(&args[1..]);
    }
    let path = args.first().ok_or_else(|| anyhow::anyhow!("chemin du .skp"))?;
    let data = std::fs::read(path)?;
    if args.get(1).map(String::as_str) == Some("--styles") {
        // The styles and watermarks as the native reader keeps them.
        let d = esq_skp::native::read_doc(&data, &Default::default())?;
        let st = &d.styles;
        let wm = |w: &Option<esq_skp::native::style::Wm>| match w {
            None => "<modèle>".to_string(),
            Some(w) => format!(
                "{:?} trouvé={} {:?} t={} pos={} ajust={} prop={} fond={} masque={} alpha={} échelle={} image={}o {:?}",
                w.name,
                w.found,
                w.file,
                w.time,
                w.position,
                w.fitting,
                w.keep_aspect,
                w.background,
                w.mask,
                w.alpha,
                w.scale,
                w.image.as_ref().map_or(0, |i| i.len()),
                w.image_name
            ),
        };
        let show = |s: &esq_skp::native::style::StyleRec| {
            println!(
                "  {} {:?} desc={:?} chemin={:?} dossier={:?} guid={} filigranes={:?}",
                s.id, s.name, s.description, s.path, s.folder, s.guid, s.watermark_ids
            );
            for it in &s.items {
                match &it.v {
                    esq_skp::native::style::ItemV::Watermarks(l) => {
                        println!("    {} filigranes :", it.id);
                        for w in l {
                            println!("      {}", wm(w));
                        }
                    }
                    esq_skp::native::style::ItemV::Npr(n) => println!(
                        "    {} esquisse {} {} {} {} {} {} {:?} {} images",
                        it.id,
                        n.dropout,
                        n.fade,
                        n.width,
                        n.halo,
                        n.halo_length,
                        n.protos,
                        n.lengths,
                        n.images.len()
                    ),
                    v => println!("    {} {:?} = {:?}", it.id, v, v.f64()),
                }
            }
        };
        println!("styles ({}), sélectionné {:?}, modifié {}", st.list.len(), st.selected, st.changed);
        for s in &st.list {
            show(s);
        }
        if let Some(a) = &st.active {
            println!("actif :");
            show(a);
        }
        println!("filigranes du modèle :");
        for w in &st.watermarks {
            println!("  {}", wm(w));
        }
        for s in &d.scenes {
            println!("scène {:?} style {:?} drapeaux {:#x}", s.name, s.style, s.flags);
        }
        return Ok(());
    }
    let f = container::open(&data)?;
    println!("{} {} {:?}", f.magic, f.version_text, f.version);
    match &f.body {
        Body::Zip { archive, marker } => {
            println!("marqueur : {}", hex(marker, 64));
            if args.get(1).map(String::as_str) == Some("--extract") {
                let dir = std::path::PathBuf::from(args.get(2).ok_or_else(|| anyhow::anyhow!("dossier"))?);
                for e in &archive.entries {
                    let p = dir.join(e.name.replace(['/', '\\'], std::path::MAIN_SEPARATOR_STR));
                    std::fs::create_dir_all(p.parent().unwrap())?;
                    std::fs::write(&p, archive.read(e)?)?;
                }
                return Ok(());
            }
            for e in &archive.entries {
                println!("  {:>9} {:>9} m{} {}", e.size, e.compressed, e.method, e.name);
            }
            let name = args.get(1).map(String::as_str).unwrap_or("model.dat");
            let bytes = archive.read_name(name)?;
            if is_records(&bytes) {
                let mut out = String::new();
                tree(&mut out, &bytes, 0, 0);
                print!("{out}");
            } else {
                println!("{}", hex(&bytes, 4096));
            }
        }
        Body::Legacy { guid, rest } => {
            println!("guid : {}", hex(guid, 16));
            match esq_skp::native::legacy::header(rest) {
                Ok((ar, map)) => {
                    let list: Vec<String> = map.iter().map(|(c, v)| format!("{c}={v}")).collect();
                    println!("versions : {}", list.join(" "));
                    println!("suite @{} : {}", ar.pos, hex(&rest[ar.pos..], 512));
                }
                Err(e) => println!("{e}"),
            }
            match esq_skp::native::legacy::read_counted(rest, guid, f.version, &Default::default()) {
                Ok((d, left)) => println!(
                    "lu ({left} octets restants) : {} matières, {} balises, {} définitions, {} arêtes, {} faces, {} scènes",
                    d.materials.len(),
                    d.layers.len(),
                    d.defs.len(),
                    d.root.edges.len(),
                    d.root.faces.len(),
                    d.scenes.len()
                ),
                Err(e) => {
                    println!("{e}");
                    // Bytes around the failure (`(octet N)` counts from the end of the GUID).
                    if let Some(at) =
                        e.0.rsplit("octet ").next().and_then(|x| x.trim_end_matches(')').parse::<usize>().ok())
                    {
                        let from = at.saturating_sub(64);
                        println!("@{from} : {}", hex(&rest[from.min(rest.len())..], 160));
                    }
                }
            }
        }
    }
    Ok(())
}
