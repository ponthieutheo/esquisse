//! Data of a matched photo (Camera › Match New Photo): the image, how it was calibrated, and
//! the display options of its perspective grid. It lives in `Scene::photo`.
//!
//! The image file is kept as it was loaded (PNG or JPEG bytes, base64 in `.esq`), so the
//! document does not depend on the original file.

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::matchphoto::{PhotoCamera, Setup};
use crate::model::SceneCamera;

/// Binary data (an image file) stored in the model; cheap to clone, base64 in `.esq`.
#[derive(Clone, Default, PartialEq)]
pub struct Blob(pub Arc<Vec<u8>>);

impl fmt::Debug for Blob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Blob({} octets)", self.0.len())
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n =
            (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in text.bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = B64.iter().position(|&b| b == c)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits & 0xFF) as u8);
        }
    }
    Some(out)
}

impl Serialize for Blob {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base64_encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for Blob {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        let bytes = base64_decode(&text).ok_or_else(|| serde::de::Error::custom("base64 invalide"))?;
        Ok(Blob(Arc::new(bytes)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GridStyle {
    #[default]
    Lines,
    Crosses,
}

/// Perspective grid drawn over the photo while it is matched (`GridShowXY`, `GridShowXZ`,
/// `GridShowYZ`, `GridStyle`, `GridXSpacing`… in SketchUp's tool info).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhotoGrid {
    /// Planes shown: red–green (ground), red–blue, green–blue.
    pub show: [bool; 3],
    pub style: GridStyle,
    /// Spacing along the red, green and blue axes, in mm.
    pub spacing: [f64; 3],
}

impl Default for PhotoGrid {
    fn default() -> Self {
        PhotoGrid { show: [true, false, false], style: GridStyle::Lines, spacing: [1000.0; 3] }
    }
}

/// A photograph and the camera calibrated on it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchPhoto {
    /// File name the image was loaded from.
    pub name: String,
    pub image: Blob,
    /// Image size in pixels.
    pub size: [u32; 2],
    /// What the user gave to the calibration (to edit it again).
    pub setup: Setup,
    /// The calibrated camera.
    pub camera: PhotoCamera,
    #[serde(default)]
    pub grid: PhotoGrid,
}

impl MatchPhoto {
    /// Camera saved with the scene: looks at the origin of the photo.
    pub fn scene_camera(&self) -> SceneCamera {
        self.camera.scene_camera(self.camera.trans.length())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        for n in 0..9 {
            let data: Vec<u8> = (0..n).map(|i| (i * 37 + 5) as u8).collect();
            assert_eq!(base64_decode(&base64_encode(&data)).unwrap(), data);
        }
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
    }

    fn sample() -> MatchPhoto {
        use crate::matchphoto::{Line2, Scale};
        use glam::{DMat3, DVec2, DVec3};
        let l = |x: f64| Line2 { a: DVec2::new(x, 10.0), b: DVec2::new(x + 5.0, 90.0) };
        MatchPhoto {
            name: "mur.jpg".into(),
            image: Blob(Arc::new(vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3])),
            size: [200, 100],
            setup: Setup {
                size: DVec2::new(200.0, 100.0),
                principal: None,
                lines: [Some([l(1.0), l(2.0)]), Some([l(3.0), l(4.0)]), None],
                origin: DVec2::new(100.0, 70.0),
                scale: Scale::Distance(5000.0),
                variant: Some(2),
            },
            camera: PhotoCamera {
                rot: DMat3::IDENTITY,
                trans: DVec3::new(0.0, 0.0, 5000.0),
                focal: 180.0,
                principal: DVec2::new(100.0, 50.0),
                size: DVec2::new(200.0, 100.0),
            },
            grid: PhotoGrid::default(),
        }
    }

    #[test]
    fn photo_scene_survives_the_esq_round_trip() {
        use crate::model::{Model, Scene};
        let mut m = Model::default();
        let photo = sample();
        m.scenes.push(Scene {
            name: "Photo".into(),
            description: String::new(),
            camera: photo.scene_camera(),
            hidden_layers: None,
            photo: Some(photo.clone()),
            ext: Default::default(),
        });
        m.scenes.push(Scene {
            name: "Plain".into(),
            description: String::new(),
            camera: photo.scene_camera(),
            hidden_layers: None,
            photo: None,
            ext: Default::default(),
        });
        let path = std::env::temp_dir().join(format!("esq-photo-test-{}.esq", std::process::id()));
        crate::io::save_esq(&m, &path).unwrap();
        let back = crate::io::load_esq(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(back.scenes[0].photo.as_ref(), Some(&photo));
        assert!(back.scenes[1].photo.is_none());
    }

    #[test]
    fn old_scenes_without_a_photo_still_load() {
        let json =
            r#"{"name":"A","camera":{"eye":[0,0,0],"target":[1,0,0],"up":[0,0,1],"fov_deg":35,"perspective":true}}"#;
        let s: crate::model::Scene = serde_json::from_str(json).unwrap();
        assert!(s.photo.is_none());
    }
}
