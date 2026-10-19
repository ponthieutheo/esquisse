//! The one ZIP reader of Esquisse's files (`.style`, `.skm`, `.skh`, `.skc`, `.ske`, `.rbz`,
//! KMZ, IFCZIP, USDZ): stored and deflated entries, ZIP64 sizes and offsets. Every length and
//! offset is checked, and everything an archive inflates is counted against one
//! [`Budget`]: a forged archive cannot make it panic, read out of bounds, or fill the
//! memory (deflate bombs, entries overlapping the same data). `.skp` files have their own
//! reader (`esq-skp`, `native::zip`), with the same rules.

use crate::untrusted::Budget;

/// The most entries an archive may list.
pub const MAX_ENTRIES: usize = 1 << 20;

/// One entry of the central directory.
#[derive(Clone, Debug)]
pub struct Header {
    pub name: String,
    pub method: u16,
    pub flags: u16,
    pub crc: u32,
    pub compressed: u64,
    pub size: u64,
    /// Offset of the local header.
    pub local: u64,
    /// "Version made by" (its high byte is the system: 3 = Unix).
    pub made_by: u16,
    pub external: u32,
}

impl Header {
    pub fn is_dir(&self) -> bool {
        self.name.ends_with('/') || self.name.ends_with('\\')
    }

    /// A symbolic link stored by a Unix archiver (its data is the target's path).
    pub fn is_symlink(&self) -> bool {
        self.made_by >> 8 == 3 && (self.external >> 16) & 0o170_000 == 0o120_000
    }

    pub fn is_encrypted(&self) -> bool {
        self.flags & 1 != 0
    }
}

/// A file of an archive.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub data: Vec<u8>,
}

pub struct Archive<'a> {
    data: &'a [u8],
    pub entries: Vec<Header>,
}

fn u16_at(d: &[u8], at: usize) -> Result<u16, String> {
    let b = d.get(at..at.checked_add(2).ok_or("archive ZIP abîmée")?).ok_or("archive ZIP tronquée")?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(d: &[u8], at: usize) -> Result<u32, String> {
    let b = d.get(at..at.checked_add(4).ok_or("archive ZIP abîmée")?).ok_or("archive ZIP tronquée")?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn u64_at(d: &[u8], at: usize) -> Result<u64, String> {
    let b = d.get(at..at.checked_add(8).ok_or("archive ZIP abîmée")?).ok_or("archive ZIP tronquée")?;
    Ok(u64::from_le_bytes(b.try_into().map_err(|_| "archive ZIP tronquée")?))
}

fn to_usize(x: u64) -> Result<usize, String> {
    usize::try_from(x).map_err(|_| "archive ZIP abîmée (décalage trop grand)".to_string())
}

/// Whether the bytes start like a ZIP archive.
pub fn is_zip(b: &[u8]) -> bool {
    b.starts_with(b"PK\x03\x04") || b.starts_with(b"PK\x05\x06")
}

impl<'a> Archive<'a> {
    /// Reads the central directory.
    pub fn open(data: &'a [u8]) -> Result<Archive<'a>, String> {
        // End of central directory: 22 bytes plus a comment of at most 65535 bytes.
        let lo = data.len().saturating_sub(22 + 65_535);
        let eocd = (lo..data.len().saturating_sub(21))
            .rev()
            .find(|&i| data[i..].starts_with(b"PK\x05\x06"))
            .ok_or("pas une archive ZIP (fin de répertoire introuvable)")?;
        let mut count = u16_at(data, eocd + 10)? as u64;
        let mut cd = u32_at(data, eocd + 16)? as u64;
        // ZIP64: the locator sits just before the end record.
        if eocd >= 20 && data[eocd - 20..].starts_with(b"PK\x06\x07") {
            let rec = to_usize(u64_at(data, eocd - 20 + 8)?)?;
            if data.get(rec..).is_some_and(|s| s.starts_with(b"PK\x06\x06")) {
                count = u64_at(data, rec + 32)?;
                cd = u64_at(data, rec + 48)?;
            }
        }
        let count = to_usize(count)?;
        // A directory record takes at least 46 bytes.
        if count > MAX_ENTRIES || count > data.len() / 46 + 1 {
            return Err("archive ZIP abîmée (trop d'entrées)".into());
        }
        let mut entries = Vec::with_capacity(count);
        let mut p = to_usize(cd)?;
        for _ in 0..count {
            if u32_at(data, p)? != 0x0201_4b50 {
                return Err("répertoire ZIP abîmé".into());
            }
            let made_by = u16_at(data, p + 4)?;
            let flags = u16_at(data, p + 8)?;
            let method = u16_at(data, p + 10)?;
            let crc = u32_at(data, p + 16)?;
            let mut compressed = u32_at(data, p + 20)? as u64;
            let mut size = u32_at(data, p + 24)? as u64;
            let nlen = u16_at(data, p + 28)? as usize;
            let xlen = u16_at(data, p + 30)? as usize;
            let clen = u16_at(data, p + 32)? as usize;
            let external = u32_at(data, p + 38)?;
            let mut local = u32_at(data, p + 42)? as u64;
            let name = data.get(p + 46..p + 46 + nlen).ok_or("nom ZIP tronqué")?;
            let name = String::from_utf8_lossy(name).into_owned();
            // ZIP64 extended information: the fields that are all ones, in this order.
            let extra = data.get(p + 46 + nlen..p + 46 + nlen + xlen).ok_or("champ ZIP tronqué")?;
            let mut x = 0usize;
            while x + 4 <= extra.len() {
                let id = u16_at(extra, x)?;
                let len = u16_at(extra, x + 2)? as usize;
                let body = extra.get(x + 4..x + 4 + len).ok_or("champ ZIP tronqué")?;
                if id == 1 {
                    let mut k = 0usize;
                    for field in [&mut size, &mut compressed, &mut local] {
                        if *field == 0xFFFF_FFFF {
                            *field = u64_at(body, k)?;
                            k += 8;
                        }
                    }
                }
                x += 4 + len;
            }
            entries.push(Header { name, method, flags, crc, compressed, size, local, made_by, external });
            p += 46 + nlen + xlen + clen;
        }
        Ok(Archive { data, entries })
    }

    /// The bytes of an entry, inflated within `budget` (and within the size the directory
    /// declares).
    pub fn read(&self, e: &Header, budget: &Budget) -> Result<Vec<u8>, String> {
        if e.is_encrypted() {
            return Err(format!("entrée ZIP chiffrée : {}", e.name));
        }
        // Deflate cannot expand more than 1032 times: a larger declared size is a forgery.
        if e.method == 8 && e.size > e.compressed.saturating_mul(1032).saturating_add(64) {
            return Err(format!("entrée ZIP trop grande pour ses données : {}", e.name));
        }
        let at = to_usize(e.local)?;
        if u32_at(self.data, at)? != 0x0403_4b50 {
            return Err(format!("en-tête local ZIP abîmé : {}", e.name));
        }
        let start = at + 30 + u16_at(self.data, at + 26)? as usize + u16_at(self.data, at + 28)? as usize;
        let end = start.checked_add(to_usize(e.compressed)?).ok_or("archive ZIP abîmée")?;
        let raw = self.data.get(start..end).ok_or_else(|| format!("données ZIP tronquées : {}", e.name))?;
        match e.method {
            0 => {
                budget.take(raw.len() as u64)?;
                Ok(raw.to_vec())
            }
            8 => budget
                .inflate(raw, (e.size > 0).then_some(e.size), false)
                .map_err(|err| format!("décompression de {} : {err}", e.name)),
            m => Err(format!("méthode de compression ZIP non prise en charge ({m}) : {}", e.name)),
        }
    }
}

/// The files of the archive `want` accepts, in directory order (folders are skipped), all
/// within one budget made for the archive's size.
pub fn read_matching(data: &[u8], want: impl Fn(&str) -> bool) -> Result<Vec<Entry>, String> {
    let a = Archive::open(data)?;
    let budget = Budget::for_input(data.len());
    let mut out = Vec::new();
    for e in a.entries.iter().filter(|e| !e.is_dir() && want(&e.name)) {
        out.push(Entry { name: e.name.clone(), data: a.read(e, &budget)? });
    }
    Ok(out)
}

/// Every file of the archive.
pub fn read_all(data: &[u8]) -> Result<Vec<Entry>, String> {
    read_matching(data, |_| true)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An archive of `(name, data, deflate)`, every entry pointing at its own data.
    pub fn archive(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data, deflate) in files {
            let packed = if *deflate { miniz_oxide::deflate::compress_to_vec(data, 9) } else { data.to_vec() };
            let off = out.len() as u32;
            let fields = |v: &mut Vec<u8>| {
                v.extend(20u16.to_le_bytes());
                v.extend(0u16.to_le_bytes());
                v.extend(if *deflate { 8u16 } else { 0u16 }.to_le_bytes());
                v.extend([0u8; 4]);
                v.extend(0u32.to_le_bytes());
                v.extend((packed.len() as u32).to_le_bytes());
                v.extend((data.len() as u32).to_le_bytes());
                v.extend((name.len() as u16).to_le_bytes());
                v.extend(0u16.to_le_bytes());
            };
            out.extend(0x0403_4b50u32.to_le_bytes());
            fields(&mut out);
            out.extend(name.as_bytes());
            out.extend(&packed);
            central.extend(0x0201_4b50u32.to_le_bytes());
            central.extend(20u16.to_le_bytes());
            fields(&mut central);
            // Comment length, disk, internal attributes; external attributes.
            central.extend([0u8; 6]);
            central.extend(0u32.to_le_bytes());
            central.extend(off.to_le_bytes());
            central.extend(name.as_bytes());
        }
        let cd = out.len() as u32;
        out.extend(&central);
        out.extend(0x0605_4b50u32.to_le_bytes());
        out.extend([0u8; 4]);
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((central.len() as u32).to_le_bytes());
        out.extend(cd.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out
    }

    /// An archive whose `n` directory entries all name the same deflated data (the
    /// "overlapping entries" bomb): each inflates to `size` bytes.
    pub fn overlapping_bomb(n: usize, size: usize) -> Vec<u8> {
        let one = archive(&[("a", &vec![0u8; size], true)]);
        // The single local header + data, then n copies of its directory record.
        let cd_at = u32::from_le_bytes(one[one.len() - 6..one.len() - 2].try_into().unwrap()) as usize;
        let rec = one[cd_at..one.len() - 22].to_vec();
        let mut out = one[..cd_at].to_vec();
        let cd = out.len() as u32;
        for _ in 0..n {
            out.extend(&rec);
        }
        out.extend(0x0605_4b50u32.to_le_bytes());
        out.extend([0u8; 4]);
        out.extend((n as u16).to_le_bytes());
        out.extend((n as u16).to_le_bytes());
        out.extend(((rec.len() * n) as u32).to_le_bytes());
        out.extend(cd.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out
    }

    #[test]
    fn reads_stored_and_deflated_entries() {
        let z = archive(&[("doc.xml", b"<a/>", true), ("ref/x.png", &[1, 2, 3], false), ("dir/", b"", false)]);
        let all = read_all(&z).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].data, b"<a/>");
        assert_eq!(all[1].data, [1, 2, 3]);
        assert_eq!(read_matching(&z, |n| n == "ref/x.png").unwrap().len(), 1);
        assert!(is_zip(&z));
        assert!(read_all(b"pas une archive").is_err());
    }

    #[test]
    fn bombs_are_refused() {
        // 4000 entries naming the same 1 MB (deflated to about 1 kB): 4 GB if all were read.
        let z = overlapping_bomb(4000, 1 << 20);
        assert!(z.len() < 300_000, "{}", z.len());
        let e = read_all(&z).unwrap_err();
        assert!(e.contains("beaucoup trop"), "{e}");
        // A declared size larger than deflate can make.
        let mut z = archive(&[("a", &[7u8; 100], true)]);
        let cd = u32::from_le_bytes(z[z.len() - 6..z.len() - 2].try_into().unwrap()) as usize;
        z[cd + 24..cd + 28].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_all(&z).is_err());
        // More data than declared.
        let mut z = archive(&[("a", &[7u8; 10_000], true)]);
        let cd = u32::from_le_bytes(z[z.len() - 6..z.len() - 2].try_into().unwrap()) as usize;
        z[cd + 24..cd + 28].copy_from_slice(&100u32.to_le_bytes());
        assert!(read_all(&z).is_err());
    }

    #[test]
    fn forged_offsets_and_counts_fail_cleanly() {
        let z = archive(&[("a", b"bonjour", false)]);
        for cut in 0..z.len() {
            let _ = read_all(&z[..cut]);
        }
        let mut z2 = z.clone();
        let n = z2.len();
        z2[n - 12..n - 10].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(read_all(&z2).is_err());
        for i in 0..z.len() {
            let mut m = z.clone();
            m[i] ^= 0xFF;
            let _ = read_all(&m);
        }
    }
}
