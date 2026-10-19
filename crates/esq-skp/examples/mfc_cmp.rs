//! Format study: the MFC archive the native writer makes next to the one the SDK makes for the
//! same model and version; shows the first byte that differs and the object it is in.
//!
//! `cargo run -p esq-skp --example mfc_cmp -- [--version 2020] files…` (each file read
//! natively, written in its own version unless `--version`).

use esq_skp::SkpVersion;

fn hex(d: &[u8], from: usize, to: usize) -> String {
    d[from.min(d.len())..to.min(d.len())].iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut forced = None;
    if let Some(i) = args.iter().position(|a| a == "--version") {
        let v = args[i + 1].clone();
        forced = SkpVersion::ALL.into_iter().find(|x| x.name() == v);
        args.drain(i..i + 2);
    }
    let tmp = std::env::temp_dir().join(format!("esq-mfc-cmp-{}.skp", std::process::id()));
    let (mut same, mut n) = (0, 0);
    // Folders: every .skp below.
    let mut files = Vec::new();
    let mut stack: Vec<std::path::PathBuf> = args.iter().map(std::path::PathBuf::from).collect();
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            for e in std::fs::read_dir(&p)?.flatten() {
                stack.push(e.path());
            }
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("skp")) {
            files.push(p.to_string_lossy().into_owned());
        }
    }
    files.sort();
    for f in &files {
        let data = std::fs::read(f)?;
        let (m, view) = esq_skp::native::read_bytes(&data)?;
        let major = esq_skp::file_version(std::path::Path::new(f)).map_or(0, |v| v.0);
        let Some(v) = forced.or_else(|| SkpVersion::ALL.into_iter().find(|x| x.major() == major)) else { continue };
        if v == SkpVersion::Su2021 {
            continue;
        }
        n += 1;
        esq_skp::sdk_export_skp_version(&m, &tmp, view.as_ref(), Some(v))?;
        let a = std::fs::read(&tmp)?;
        let plan = esq_skp::native::write::plan::plan(&m, view.as_ref())?;
        let t = esq_skp::native::write::legacy::traced(&plan, v, true);
        let b = &t.bytes;
        let mut i = 0;
        let mut diff = None;
        while i < a.len().max(b.len()) {
            if let Some(&(o, len)) = t.variable.iter().find(|(o, len)| i >= *o && i < o + len) {
                i = o + len;
                continue;
            }
            if a.get(i) != b.get(i) {
                diff = Some(i);
                break;
            }
            i += 1;
        }
        match diff {
            None => {
                same += 1;
                println!("{f} : identique ({} octets)", b.len());
            }
            Some(i) => {
                let path: Vec<&str> = t.trace.iter().filter(|(o, _)| *o <= i).map(|(_, s)| s.as_str()).collect();
                let last: Vec<&str> = path.iter().rev().take(4).rev().copied().collect();
                let at = t.trace.iter().rev().find(|(o, _)| *o <= i).map_or(0, |x| x.0);
                println!(
                    "{f} : diffère à l'octet {i} (SDK {} / natif {} octets), dans {:?} (+{})",
                    a.len(),
                    b.len(),
                    last,
                    i - at
                );
                println!("   SDK   {}", hex(&a, i.saturating_sub(24), i + 40));
                println!("   natif {}", hex(b, i.saturating_sub(24), i + 40));
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    println!("{same}/{n} identiques");
    Ok(())
}
