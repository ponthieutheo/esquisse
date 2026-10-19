//! Survey of the records of 2021+ files: for each class record, the sequences of child tags
//! met, with their counts (format study for the writer).
//!
//! `cargo run -p esq-skp --example vff_survey -- files…` (`--tag 0x7148` to show one class).

use std::collections::BTreeMap;

use esq_skp::native::container::{self, Body};
use esq_skp::native::tlv::{Records, is_records};
use esq_skp::native::vff;

fn walk(d: &[u8], depth: usize, out: &mut BTreeMap<u16, BTreeMap<String, usize>>, known: &dyn Fn(u16) -> bool) {
    for r in Records::new(d, 0) {
        if depth < 64 && is_records(r.data) {
            if known(r.tag) {
                let seq: Vec<String> = Records::new(r.data, 0).map(|c| format!("{:04x}", c.tag)).collect();
                *out.entry(r.tag).or_default().entry(seq.join(" ")).or_default() += 1;
            }
            walk(r.data, depth + 1, out, known);
        }
    }
}

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut only = None;
    if let Some(i) = args.iter().position(|a| a == "--tag") {
        only = Some(u16::from_str_radix(args[i + 1].trim_start_matches("0x"), 16)?);
        args.drain(i..i + 2);
    }
    let mut out: BTreeMap<u16, BTreeMap<String, usize>> = BTreeMap::new();
    let known = |t: u16| vff::CLASSES.iter().any(|c| c.0 == t) || (0x1f4..0x220).contains(&t) || only == Some(t);
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
    for p in &files {
        let data = std::fs::read(p)?;
        let f = container::open(&data)?;
        if let Body::Zip { archive, .. } = &f.body {
            walk(&archive.read_name("model.dat")?, 0, &mut out, &known);
        }
    }
    for (tag, seqs) in &out {
        if only.is_some_and(|o| o != *tag) {
            continue;
        }
        let name = vff::CLASSES.iter().find(|c| c.0 == *tag).map_or("", |c| c.1);
        println!("{tag:04x} {name}");
        for (s, n) in seqs {
            println!("   {n:>5}  {s}");
        }
    }
    Ok(())
}
