//! The icon of `esquisse.exe`: a Windows resource file (`.res`) holding one `RT_ICON` per image
//! of `assets/logo/esquisse.ico` and the `RT_GROUP_ICON` that lists them. `build.rs` writes it
//! (this file is shared with it, standard library only) and hands it to link.exe, which takes
//! `.res` files as input: no resource compiler and no crate needed.
//!
//! Formats: ICO = ICONDIR (6 bytes) + 16-byte entries + images; `.res` = 32-bit resource file,
//! an empty first entry then, for each resource, a header (sizes, type and name as ordinals,
//! flags, language) and the data padded to 4 bytes. Images may be PNG (Windows Vista and later).

/// One image of an ICO file.
pub struct IcoImage<'a> {
    /// 256 is written 0 in the directories.
    pub width: u16,
    pub height: u16,
    pub colors: u8,
    pub planes: u16,
    pub bit_count: u16,
    pub data: &'a [u8],
}

pub const RT_ICON: u16 = 3;
pub const RT_GROUP_ICON: u16 = 14;
/// Name of the icon group (the first group of the exe is its icon in Explorer and the taskbar).
pub const GROUP_ID: u16 = 1;
const MOVEABLE_DISCARDABLE: u16 = 0x1010;
const MOVEABLE_PURE_DISCARDABLE: u16 = 0x1030;

fn u16_at(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| format!("ICO tronqué à l'octet {at}"))
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| format!("ICO tronqué à l'octet {at}"))
}

/// The images of an ICO file, every size and offset checked against the file.
pub fn parse_ico(ico: &[u8]) -> Result<Vec<IcoImage<'_>>, String> {
    if u16_at(ico, 0)? != 0 || u16_at(ico, 2)? != 1 {
        return Err("pas un fichier ICO".into());
    }
    let count = u16_at(ico, 4)? as usize;
    if count == 0 || count > 64 {
        return Err(format!("ICO : {count} images"));
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let e = 6 + 16 * i;
        let dim = |at: usize| ico.get(at).map(|&v| if v == 0 { 256 } else { v as u16 }).ok_or("ICO tronqué");
        let (width, height) = (dim(e)?, dim(e + 1)?);
        let size = u32_at(ico, e + 8)? as usize;
        let offset = u32_at(ico, e + 12)? as usize;
        let data = offset
            .checked_add(size)
            .and_then(|end| ico.get(offset..end))
            .ok_or_else(|| format!("ICO : l'image {i} sort du fichier"))?;
        out.push(IcoImage {
            width,
            height,
            colors: ico[e + 2],
            planes: u16_at(ico, e + 4)?,
            bit_count: u16_at(ico, e + 6)?,
            data,
        });
    }
    Ok(out)
}

/// An ICO file made of `images` (PNG data, 32 bits), in the given order.
pub fn write_ico(images: &[(u16, u16, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len();
    for &(w, h, data) in images {
        out.extend_from_slice(&[(w % 256) as u8, (h % 256) as u8, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += data.len();
    }
    for &(_, _, data) in images {
        out.extend_from_slice(data);
    }
    out
}

fn resource(out: &mut Vec<u8>, kind: u16, name: u16, flags: u16, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes()); // header size
    out.extend_from_slice(&[0xFF, 0xFF]);
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&[0xFF, 0xFF]);
    out.extend_from_slice(&name.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // data version
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // language: neutral
    out.extend_from_slice(&0u32.to_le_bytes()); // version
    out.extend_from_slice(&0u32.to_le_bytes()); // characteristics
    out.extend_from_slice(data);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

/// The `.res` file for the icon `ico`: images numbered 1, 2… and group [`GROUP_ID`].
pub fn icon_res(ico: &[u8]) -> Result<Vec<u8>, String> {
    let images = parse_ico(ico)?;
    let mut out = Vec::new();
    // The empty entry that opens every 32-bit resource file.
    resource(&mut out, 0, 0, 0, &[]);
    let mut group = Vec::new();
    group.extend_from_slice(&[0, 0, 1, 0]);
    group.extend_from_slice(&(images.len() as u16).to_le_bytes());
    for (i, img) in images.iter().enumerate() {
        let id = i as u16 + 1;
        resource(&mut out, RT_ICON, id, MOVEABLE_DISCARDABLE, img.data);
        group.extend_from_slice(&[(img.width % 256) as u8, (img.height % 256) as u8, img.colors, 0]);
        group.extend_from_slice(&img.planes.to_le_bytes());
        group.extend_from_slice(&img.bit_count.to_le_bytes());
        group.extend_from_slice(&(img.data.len() as u32).to_le_bytes());
        group.extend_from_slice(&id.to_le_bytes());
    }
    resource(&mut out, RT_GROUP_ICON, GROUP_ID, MOVEABLE_PURE_DISCARDABLE, &group);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A resource read back from a `.res` file.
    struct Res {
        kind: u16,
        name: u16,
        flags: u16,
        data: Vec<u8>,
    }

    /// Reads a 32-bit resource file the way link.exe's converter does, checking every header.
    fn read_res(b: &[u8]) -> Vec<Res> {
        let mut at = 0;
        let mut out = Vec::new();
        while at < b.len() {
            assert_eq!(at % 4, 0, "entrée non alignée à {at}");
            let size = u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize;
            let header = u32::from_le_bytes(b[at + 4..at + 8].try_into().unwrap()) as usize;
            assert_eq!(header, 32, "en-tête de {at}");
            let w = |o: usize| u16::from_le_bytes([b[at + o], b[at + o + 1]]);
            assert_eq!((w(8), w(12)), (0xFFFF, 0xFFFF), "type et nom en ordinaux");
            assert_eq!(u32::from_le_bytes(b[at + 16..at + 20].try_into().unwrap()), 0);
            out.push(Res { kind: w(10), name: w(14), flags: w(20), data: b[at + 32..at + 32 + size].to_vec() });
            at += 32 + size;
            at += (4 - at % 4) % 4;
        }
        assert_eq!(at, b.len());
        out
    }

    fn ico_file() -> Vec<u8> {
        crate::logo::draw::ico(crate::logo::draw::Logo::Compas)
    }

    #[test]
    fn the_res_holds_every_image_of_the_icon_and_its_group() {
        for logo in crate::logo::draw::Logo::ALL {
            check_res(&crate::logo::draw::ico(logo));
        }
    }

    fn check_res(ico: &[u8]) {
        let images = parse_ico(ico).unwrap();
        let sizes: Vec<u16> = images.iter().map(|i| i.width).collect();
        assert_eq!(sizes, [16, 24, 32, 48, 64, 128, 256]);
        let res = read_res(&icon_res(ico).unwrap());
        // The empty entry, one RT_ICON per image, then the group.
        assert_eq!(res.len(), images.len() + 2);
        assert!(res[0].kind == 0 && res[0].name == 0 && res[0].data.is_empty());
        for (i, img) in images.iter().enumerate() {
            let r = &res[i + 1];
            assert_eq!((r.kind, r.name, r.flags), (RT_ICON, i as u16 + 1, 0x1010));
            assert_eq!(r.data, img.data);
            // Each image is a PNG of its stated size.
            let png = image::load_from_memory_with_format(&r.data, image::ImageFormat::Png).unwrap();
            assert_eq!((png.width(), png.height()), (img.width as u32, img.height as u32));
        }
        let g = res.last().unwrap();
        assert_eq!((g.kind, g.name, g.flags), (RT_GROUP_ICON, GROUP_ID, 0x1030));
        // The group the window loads its icons from (`logo::use_exe_icon`).
        assert_eq!(GROUP_ID, crate::logo::EXE_ICON_GROUP);
        assert_eq!(&g.data[..6], &[0, 0, 1, 0, images.len() as u8, 0]);
        assert_eq!(g.data.len(), 6 + 14 * images.len());
        for (i, img) in images.iter().enumerate() {
            let e = &g.data[6 + 14 * i..6 + 14 * (i + 1)];
            assert_eq!(e[0] as u16, img.width % 256);
            assert_eq!(e[1] as u16, img.height % 256);
            assert_eq!(u16::from_le_bytes([e[6], e[7]]), 32);
            assert_eq!(u32::from_le_bytes(e[8..12].try_into().unwrap()) as usize, img.data.len());
            assert_eq!(u16::from_le_bytes([e[12], e[13]]), i as u16 + 1);
        }
    }

    /// build.rs wrote the icon of the logo it found (the file link.exe put in esquisse.exe), and
    /// nothing when there is no logo.
    #[test]
    fn build_script_res_is_the_icon_of_the_logo() {
        let built = std::fs::read(concat!(env!("OUT_DIR"), "/esquisse.res"));
        match crate::logo::files::ICO_SOURCE {
            Some(ico) => assert_eq!(built.unwrap(), icon_res(&std::fs::read(ico).unwrap()).unwrap()),
            None => assert!(built.is_err(), "un .res sans logo"),
        }
    }

    #[test]
    fn a_broken_ico_is_refused_not_read_past_its_end() {
        assert!(parse_ico(b"").is_err());
        assert!(parse_ico(&[0, 0, 2, 0, 1, 0]).is_err()); // a cursor, not an icon
        let ico = ico_file();
        assert!(parse_ico(&ico[..ico.len() - 1]).is_err());
        assert!(parse_ico(&ico[..30]).is_err());
        let mut bad = ico.clone();
        bad[6 + 12..6 + 16].copy_from_slice(&u32::MAX.to_le_bytes()); // offset beyond the file
        assert!(parse_ico(&bad).is_err());
        // write_ico gives back what parse_ico reads.
        let imgs = parse_ico(&ico).unwrap();
        let again: Vec<(u16, u16, &[u8])> = imgs.iter().map(|i| (i.width, i.height, i.data)).collect();
        assert_eq!(write_ico(&again), ico);
    }
}
