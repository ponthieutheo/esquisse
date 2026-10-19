//! SketchUp's packaged files (`.style`, `.skm`, `.ske`…) are ZIP archives: they are read by
//! the shared reader of `esq_core::zip` (checked offsets, one decompression budget), and
//! written here.

use std::collections::BTreeMap;

/// The files of an archive, by path.
pub fn read(data: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    read_only(data, |_| true)
}

/// The files of an archive whose path `want` accepts (the others are not inflated).
pub fn read_only(data: &[u8], want: impl Fn(&str) -> bool) -> Result<BTreeMap<String, Vec<u8>>, String> {
    Ok(esq_core::zip::read_matching(data, want)?.into_iter().map(|e| (e.name, e.data)).collect())
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// An archive of these files (deflated), in this order.
pub fn write(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let packed = miniz_oxide::deflate::compress_to_vec(data, 6);
        let crc = crc32(data);
        let offset = out.len() as u32;
        let header = |sig: u32, central: bool| {
            let mut h = Vec::new();
            h.extend_from_slice(&sig.to_le_bytes());
            if central {
                h.extend_from_slice(&20u16.to_le_bytes());
            }
            h.extend_from_slice(&20u16.to_le_bytes()); // version needed
            h.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
            h.extend_from_slice(&8u16.to_le_bytes()); // deflate
            h.extend_from_slice(&0u32.to_le_bytes()); // time, date
            h.extend_from_slice(&crc.to_le_bytes());
            h.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            h.extend_from_slice(&(data.len() as u32).to_le_bytes());
            h.extend_from_slice(&(name.len() as u16).to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes()); // extra
            if central {
                h.extend_from_slice(&[0; 6]); // comment length, disk, internal attributes
                h.extend_from_slice(&0u32.to_le_bytes()); // external attributes
                h.extend_from_slice(&offset.to_le_bytes());
            }
            h.extend_from_slice(name.as_bytes());
            h
        };
        out.extend(header(0x0403_4b50, false));
        out.extend_from_slice(&packed);
        central.extend(header(0x0201_4b50, true));
    }
    let start = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archives_round_trip() {
        let files =
            vec![("document.xml".to_string(), b"<a>bonjour</a>".repeat(50)), ("ref/x.png".to_string(), vec![1, 2, 3])];
        let back = read(&write(&files)).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back["ref/x.png"], vec![1, 2, 3]);
        assert_eq!(back["document.xml"], files[0].1);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert!(read(b"pas une archive").is_err());
    }

    /// 300 directory entries naming the same deflated megabyte: the old reader inflated each
    /// (300 MB from a file of a few tens of kilobytes); the shared budget refuses it.
    #[test]
    fn a_style_file_bomb_is_refused() {
        let mut files = vec![("document.xml".to_string(), b"<a/>".to_vec())];
        files.push(("ref/a.png".into(), vec![0u8; 1 << 20]));
        let one = write(&files);
        // The directory record of "ref/a.png", repeated.
        let cd = u32::from_le_bytes(one[one.len() - 6..one.len() - 2].try_into().unwrap()) as usize;
        let first = 46 + "document.xml".len();
        let rec = one[cd + first..one.len() - 22].to_vec();
        let mut bomb = one[..cd + first].to_vec();
        for _ in 0..300 {
            bomb.extend(&rec);
        }
        bomb.extend(0x0605_4b50u32.to_le_bytes());
        bomb.extend([0; 4]);
        bomb.extend(301u16.to_le_bytes());
        bomb.extend(301u16.to_le_bytes());
        bomb.extend(((first + rec.len() * 300) as u32).to_le_bytes());
        bomb.extend((cd as u32).to_le_bytes());
        bomb.extend(0u16.to_le_bytes());
        assert!(bomb.len() < 100_000, "{}", bomb.len());
        let e = read(&bomb).unwrap_err();
        assert!(e.contains("trop de données"), "{e}");
        // Only the wanted entries count.
        assert_eq!(read_only(&bomb, |n| n == "document.xml").unwrap()["document.xml"], b"<a/>");
    }
}
