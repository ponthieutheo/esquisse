//! Format study: the properties of the styles (`CSkpStyle`) of MFC archives:
//! key, type, value, one line per style.

fn main() -> anyhow::Result<()> {
    for p in std::env::args().skip(1) {
        let d = std::fs::read(&p)?;
        // The name « Style » as a CString, after the properties' version.
        let pat: Vec<u8> =
            [0xFF, 0xFE, 0xFF, 5].into_iter().chain("Style".encode_utf16().flat_map(|u| u.to_le_bytes())).collect();
        let mut at = 0;
        while let Some(i) = d[at..].windows(pat.len()).position(|w| w == pat.as_slice()).map(|i| i + at) {
            at = i + pat.len();
            let mut q = i - 4;
            let u32_at = |q: usize| u32::from_le_bytes(d[q..q + 4].try_into().unwrap());
            let ver = u32_at(q);
            q = i + pat.len();
            // The second string.
            let (_, n) = esq_skp::native::container::mfc_string(&d[q..])?;
            q += n;
            if ver == 1 {
                print!(" [image {}]", d[q]);
                q += 1;
            }
            let count = u32_at(q);
            q += 4;
            let mut items = Vec::new();
            for _ in 0..count {
                let key = u32_at(q);
                q += 4;
                if key == 0x3f5 {
                    items.push(format!("{key}:image"));
                    break;
                }
                if key == 0x1389 {
                    let n = if ver > 2 { u32_at(q) } else { 0 };
                    if ver > 2 {
                        q += 4;
                    }
                    items.push(format!("{key}:wm{n}"));
                    continue;
                }
                let a = u32_at(q);
                let t = u32_at(q + 4);
                q += 8;
                let size = match t {
                    0 => 0,
                    1 | 2 => 1,
                    3 => 2,
                    4..=6 => 4,
                    7 | 0xf => 8,
                    9..=0xb => 24,
                    0xc => 104,
                    0xe => 4,
                    _ => 0,
                };
                let v = &d[q..q + size];
                q += size;
                let val = match (t, size) {
                    (7, 8) => format!("{}", f64::from_le_bytes(v.try_into().unwrap())),
                    (6, 4) => format!("{}f", f32::from_le_bytes(v.try_into().unwrap())),
                    (_, 4) => format!("{}", i32::from_le_bytes(v.try_into().unwrap())),
                    (_, 1) => format!("{}", v[0]),
                    _ => format!("{v:02x?}"),
                };
                items.push(format!("({key}, {a}, {t}, {val})"));
            }
            println!("{p} v{ver} n={count}: {}", items.join(" "));
        }
    }
    Ok(())
}
