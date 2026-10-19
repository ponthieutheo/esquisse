//! The style library: as many styles, in the same collections, as SketchUp 2026's
//! (`Styles\<collection>\*.style`), all ours: settings chosen here, papers generated here,
//! thumbnails rendered by Esquisse's own renderer.

use std::sync::Arc;

use esq_core::glam::{DVec2, DVec3};
use esq_core::model::Model;
use esq_core::photo::Blob;

use super::noise::{fbm, fbm2, image, jpeg, mix, png_rgba, rgb, scale, value, worley};
use crate::camera::Camera;
use crate::render::Lighting;
use crate::style::{EdgeColor, FaceStyle, ViewStyle, Watermark, WmLayout};
use crate::style_file::StyleFile;
use crate::tests::gpu::Gpu;

/// SketchUp 2026's style collections and how many styles each holds.
pub const COLLECTIONS: [(&str, usize); 8] = [
    ("Ambient Occlusion", 7),
    ("Assorted Styles", 13),
    ("Color Sets", 16),
    ("Default Styles", 14),
    ("Photo Modeling", 3),
    ("Sketchy Edges", 38),
    ("Straight Lines", 10),
    ("Style Builder Competition Winners", 10),
];

pub struct Entry {
    pub folder: &'static str,
    pub file: String,
    pub style: StyleFile,
}

type Tweak = Box<dyn Fn(&mut ViewStyle)>;

struct Def {
    name: String,
    desc: String,
    tweak: Tweak,
}

fn def(name: &str, desc: &str, f: impl Fn(&mut ViewStyle) + 'static) -> Def {
    Def { name: name.into(), desc: desc.into(), tweak: Box::new(f) }
}

// --------------------------------------------------------------------------- papers

#[derive(Clone, Copy, PartialEq)]
enum Paper {
    White,
    Cream,
    Kraft,
    Tracing,
    Grid,
    Grain,
    Slate,
    Greenboard,
    Chipboard,
    Canvas,
}

impl Paper {
    fn label(self) -> &'static str {
        match self {
            Paper::White => "papier blanc",
            Paper::Cream => "papier crème",
            Paper::Kraft => "kraft",
            Paper::Tracing => "calque",
            Paper::Grid => "papier quadrillé",
            Paper::Grain => "papier à grain",
            Paper::Slate => "ardoise",
            Paper::Greenboard => "tableau vert",
            Paper::Chipboard => "carton gris",
            Paper::Canvas => "toile",
        }
    }

    /// The paper's colour (the background under it).
    fn color(self) -> [u8; 3] {
        match self {
            Paper::White => [255, 255, 255],
            Paper::Cream => [244, 238, 222],
            Paper::Kraft => [190, 160, 120],
            Paper::Tracing => [234, 240, 242],
            Paper::Grid => [252, 252, 250],
            Paper::Grain => [247, 245, 238],
            Paper::Slate => [42, 46, 44],
            Paper::Greenboard => [36, 72, 56],
            Paper::Chipboard => [150, 140, 125],
            Paper::Canvas => [232, 224, 205],
        }
    }

    /// A tileable picture of the paper (JPEG), or none for plain white.
    fn picture(self) -> Option<Vec<u8>> {
        let n = 256;
        let base = rgb(self.color());
        let px = match self {
            Paper::White => return None,
            Paper::Cream | Paper::Tracing => image(n, |u, v| scale(base, 0.97 + 0.06 * fbm(u, v, 8, 4, 1001))),
            Paper::Kraft => image(n, |u, v| {
                let fiber = fbm2(u, v, 4, 64, 3, 1002);
                scale(base, 0.9 + 0.2 * fiber)
            }),
            Paper::Grid => image(n, |u, v| {
                let line = |t: f32| (t * 16.0).fract() < 0.06;
                if line(u) || line(v) { mix(base, rgb([140, 180, 220]), 0.6) } else { base }
            }),
            Paper::Grain => image(n, |u, v| {
                let (d, _, _) = worley(u, v, 32, 1003);
                scale(base, 0.94 + 0.08 * d.min(1.0))
            }),
            Paper::Slate | Paper::Greenboard => image(n, |u, v| {
                let dust = fbm(u, v, 4, 5, 1004);
                mix(base, rgb([200, 200, 195]), 0.12 * dust * dust)
            }),
            Paper::Chipboard => image(n, |u, v| {
                let fleck = value(u * 128.0, v * 128.0, 128, 1005);
                let c = scale(base, 0.92 + 0.12 * fbm(u, v, 8, 3, 1006));
                if fleck > 0.9 {
                    scale(c, 0.75)
                } else if fleck < 0.08 {
                    scale(c, 1.15)
                } else {
                    c
                }
            }),
            Paper::Canvas => image(n, |u, v| {
                let a = (u * 48.0 * std::f32::consts::TAU).sin().abs();
                let b = (v * 48.0 * std::f32::consts::TAU).sin().abs();
                scale(base, 0.9 + 0.06 * (a + b))
            }),
        };
        Some(jpeg(n, n, &px))
    }

    fn apply(self, s: &mut ViewStyle) {
        s.bg_color = self.color();
        s.sky = false;
        s.ground = false;
        if let Some(pic) = self.picture() {
            s.watermarks = true;
            s.watermark_list.push(Watermark {
                name: format!("Papier ({})", self.label()),
                image: Blob(Arc::new(pic)),
                background: true,
                blend: 1.0,
                mask: false,
                layout: WmLayout::Tiled,
                keep_aspect: true,
                scale: 0.5,
                position: 4,
                skp: Default::default(),
            });
        }
        if matches!(self, Paper::Slate | Paper::Greenboard) {
            s.front_color = [90, 96, 92];
            s.back_color = [60, 64, 62];
        }
    }
}

/// A white border over the picture, fading towards the middle (an overlay mask).
fn whiteout(s: &mut ViewStyle) {
    let n = 128u32;
    let mut px = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let (u, v) = ((x as f32 + 0.5) / n as f32 - 0.5, (y as f32 + 0.5) / n as f32 - 0.5);
            let edge = u.abs().max(v.abs()) * 2.0;
            let wobble = fbm(u + 0.5, v + 0.5, 6, 3, 1007) * 0.12;
            let k = 1.0 - super::noise::smoothstep(0.78, 0.95, edge + wobble);
            let g = (k * 255.0) as u8;
            px.extend([g, g, g, 255]);
        }
    }
    s.watermarks = true;
    s.watermark_list.push(Watermark {
        name: "Bordure".into(),
        image: Blob(Arc::new(png_rgba(n, n, px))),
        background: false,
        blend: 1.0,
        mask: true,
        layout: WmLayout::Stretched,
        keep_aspect: false,
        scale: 1.0,
        position: 4,
        skp: Default::default(),
    });
}

// --------------------------------------------------------------------------- drawing tools

#[derive(Clone, Copy)]
struct Tool {
    color: [u8; 3],
    jitter: bool,
    extension: u32,
    endpoints: u32,
    profile: u32,
    depth_cue: u32,
    by_material: bool,
}

const fn tool(color: [u8; 3], jitter: bool, extension: u32, endpoints: u32, profile: u32, depth_cue: u32) -> Tool {
    Tool { color, jitter, extension, endpoints, profile, depth_cue, by_material: false }
}

impl Tool {
    fn apply(self, s: &mut ViewStyle) {
        s.edges = true;
        s.edge_color = self.color;
        s.edge_color_mode = if self.by_material { EdgeColor::ByMaterial } else { EdgeColor::AllSame };
        s.jitter = self.jitter;
        s.extension = self.extension > 0;
        s.extension_length = self.extension.max(1);
        s.endpoints = self.endpoints > 0;
        s.endpoint_length = self.endpoints.max(1);
        s.profiles = self.profile > 1;
        s.profile_width = self.profile.max(1);
        s.depth_cue = self.depth_cue > 0;
        s.depth_cue_width = self.depth_cue.clamp(1, 4);
    }
}

const PENCIL: Tool = tool([80, 80, 86], true, 3, 0, 1, 0);
const PENCIL_ENDS: Tool = tool([80, 80, 86], true, 3, 5, 1, 0);
const CHARCOAL: Tool = tool([45, 45, 45], true, 4, 0, 2, 3);
const INK: Tool = tool([15, 15, 20], false, 3, 4, 2, 0);
const BALLPOINT: Tool = tool([35, 45, 120], true, 2, 0, 1, 0);
const FINELINER: Tool = tool([45, 45, 50], false, 2, 0, 3, 0);
const MARKER: Tool = tool([60, 60, 65], false, 1, 0, 4, 4);
const SANGUINE: Tool = tool([160, 65, 45], true, 4, 0, 2, 0);
const SEPIA: Tool = tool([110, 75, 45], true, 3, 4, 2, 0);
const CHALK: Tool = tool([238, 238, 232], true, 4, 0, 2, 0);
const PASTEL: Tool = Tool { by_material: true, ..tool([90, 90, 90], true, 2, 0, 2, 0) };
const QUILL: Tool = tool([20, 20, 25], false, 8, 8, 1, 0);
const TECH_PEN: Tool = tool([30, 30, 30], false, 2, 3, 2, 0);
const WATERCOLOR: Tool = Tool { by_material: true, ..tool([90, 90, 90], true, 1, 0, 2, 0) };
const BLUE_PENCIL: Tool = tool([70, 110, 180], true, 3, 0, 1, 0);
const RED_PENCIL: Tool = tool([185, 55, 50], true, 3, 0, 1, 0);
const GREY_MARKER: Tool = tool([120, 120, 125], false, 1, 0, 3, 0);
const BLUE_INK: Tool = tool([25, 60, 140], false, 3, 3, 2, 0);
const LEAD: Tool = tool([130, 130, 135], true, 2, 0, 1, 0);

fn sketch(tool_name: &str, t: Tool, paper: Paper, desc: &str) -> Def {
    def(&format!("{tool_name} sur {}", paper.label()), desc, move |s| {
        t.apply(s);
        paper.apply(s);
        if matches!(t.color, [238, 238, 232]) {
            s.front_color = [90, 96, 92];
        }
    })
}

// --------------------------------------------------------------------------- collections

fn default_styles() -> Vec<Def> {
    let white = |s: &mut ViewStyle| {
        s.bg_color = [255, 255, 255];
        s.sky = false;
        s.ground = false;
    };
    vec![
        def(
            "Style simple",
            "Couleurs de faces par défaut, ciel bleu, arrière-plan gris : le style des nouveaux modèles.",
            |_| {},
        ),
        def("Filaire", "Arêtes seules, sans faces, sur fond blanc.", move |s| {
            s.set_faces(FaceStyle::Wireframe);
            white(s);
        }),
        def(
            "Lignes cachées",
            "Faces de la couleur du fond : seules les arêtes visibles restent. Fond blanc.",
            move |s| {
                s.set_faces(FaceStyle::HiddenLine);
                white(s);
            },
        ),
        def("Ombré", "Faces ombrées dans la couleur moyenne de leur matière. Fond blanc.", move |s| {
            s.set_faces(FaceStyle::Shaded);
            white(s);
        }),
        def("Ombré avec textures", "Faces ombrées avec leurs textures. Fond blanc.", move |s| {
            s.set_faces(FaceStyle::ShadedWithTextures);
            white(s);
        }),
        def("Rayons X", "Faces transparentes, textures visibles. Fond blanc.", move |s| {
            s.xray = true;
            white(s);
        }),
        def("Monochrome", "Toutes les faces dans les couleurs de recto et de verso par défaut.", |s| {
            s.set_faces(FaceStyle::Monochrome);
        }),
        def("Architecture", "Profils de 2 pixels, ciel bleu pâle, arrière-plan gris clair.", |s| {
            s.sky_color = [150, 195, 225];
            s.bg_color = [205, 205, 200];
        }),
        def("Conception", "Profils épais de 3 pixels, textures, ciel et arrière-plan gris bleuté.", |s| {
            s.profile_width = 3;
            s.sky_color = [120, 160, 200];
            s.bg_color = [170, 180, 190];
        }),
        def("Plans d'exécution", "Profils, fond blanc, sans ciel : pour les plans et les coupes.", move |s| {
            white(s);
            s.profile_width = 2;
        }),
        def("Urbanisme", "Profils, ciel bleu, sol gris pour les grands ensembles.", |s| {
            s.ground = true;
            s.ground_color = [150, 150, 145];
            s.sky_color = [90, 160, 220];
        }),
        def("Paysage", "Profils, ciel gris bleu, sol vert foncé.", |s| {
            s.ground = true;
            s.ground_color = [70, 100, 60];
            s.sky_color = [140, 170, 195];
            s.bg_color = [90, 120, 80];
        }),
        def("Menuiserie", "Profils, fond gris neutre : pour le mobilier et les petits objets.", |s| {
            s.sky = false;
            s.bg_color = [175, 175, 175];
        }),
        def("Impression 3D", "Profils, ciel bleu vif, arrière-plan gris foncé.", |s| {
            s.sky_color = [30, 140, 230];
            s.bg_color = [80, 82, 85];
        }),
    ]
}

fn color_sets() -> Vec<Def> {
    let set = |name: &str, front: [u8; 3], back: [u8; 3], bg: [u8; 3], sky: [u8; 3]| {
        def(
            name,
            &format!("Recto et verso des faces, arrière-plan et ciel : gamme « {} ».", name.to_lowercase()),
            move |s| {
                s.front_color = front;
                s.back_color = back;
                s.bg_color = bg;
                s.sky_color = sky;
            },
        )
    };
    vec![
        set("Couleurs d'origine", [255, 255, 255], [162, 183, 198], [191, 191, 198], [0, 154, 217]),
        set("Terre cuite et ardoise", [205, 125, 90], [80, 90, 100], [200, 190, 175], [150, 185, 210]),
        set("Sable et océan", [228, 208, 165], [40, 95, 145], [215, 205, 180], [110, 175, 220]),
        set("Lavande", [205, 195, 228], [110, 95, 150], [215, 210, 225], [170, 160, 210]),
        set("Corail", [242, 145, 125], [95, 62, 60], [235, 215, 205], [250, 190, 160]),
        set("Forêt", [125, 155, 95], [60, 80, 50], [90, 110, 75], [150, 185, 160]),
        def("Nuit", "Faces bleu nuit, arêtes claires sur fond sombre.", |s| {
            s.front_color = [70, 80, 110];
            s.back_color = [30, 35, 50];
            s.bg_color = [25, 28, 40];
            s.sky_color = [40, 50, 85];
            s.edge_color = [220, 225, 235];
        }),
        set("Papier kraft", [205, 175, 130], [150, 120, 85], [220, 200, 170], [200, 185, 160]),
        set("Pastel", [246, 226, 232], [200, 230, 220], [240, 236, 228], [200, 220, 240]),
        set("Bleu glacier", [222, 236, 246], [130, 170, 200], [210, 220, 228], [170, 210, 235]),
        set("Olive", [175, 175, 115], [110, 110, 60], [190, 190, 160], [160, 180, 170]),
        set("Bordeaux et crème", [246, 236, 216], [112, 30, 46], [225, 215, 200], [180, 160, 170]),
        set("Menthe et chocolat", [192, 236, 212], [92, 62, 42], [220, 230, 220], [170, 220, 210]),
        set("Automne", [232, 172, 82], [150, 72, 40], [210, 185, 150], [230, 170, 120]),
        def("Contraste élevé", "Faces blanches, verso noir, arêtes noires épaisses sur fond blanc.", |s| {
            s.front_color = [255, 255, 255];
            s.back_color = [0, 0, 0];
            s.bg_color = [255, 255, 255];
            s.sky = false;
            s.profile_width = 3;
        }),
        def(
            "Daltonisme",
            "Fond sombre et arêtes blanches, faces grises : une base où les couleurs d'axes et d'inférences choisies dans Préférences › Accessibilité se voient bien.",
            |s| {
                s.front_color = [150, 150, 150];
                s.back_color = [90, 90, 90];
                s.bg_color = [40, 40, 40];
                s.sky = false;
                s.edge_color = [255, 255, 255];
            },
        ),
    ]
}

fn straight_lines() -> Vec<Def> {
    (1..=10)
        .map(|n| {
            let name = if n == 1 { "Traits droits 1 pixel".to_string() } else { format!("Traits droits {n} pixels") };
            def(
                &name,
                &format!(
                    "Arêtes droites, profils de {n} pixel{} de large, sur fond blanc.",
                    if n > 1 { "s" } else { "" }
                ),
                move |s| {
                    s.profiles = n > 1;
                    s.profile_width = n;
                    s.bg_color = [255, 255, 255];
                    s.sky = false;
                },
            )
        })
        .collect()
}

fn sketchy() -> Vec<Def> {
    use Paper::*;
    let d = |t: &str| format!("{t} Esquisse dessine des arêtes tremblées et prolongées (pas de traits scannés).");
    vec![
        sketch("Crayon", PENCIL, White, &d("Crayon gris tremblé.")),
        sketch("Crayon", PENCIL, Cream, &d("Crayon gris tremblé.")),
        sketch("Crayon", PENCIL, Tracing, &d("Crayon gris tremblé.")),
        sketch("Crayon", PENCIL, Grid, &d("Crayon gris tremblé.")),
        sketch("Crayon et extrémités", PENCIL_ENDS, White, &d("Crayon tremblé, extrémités appuyées.")),
        sketch("Crayon et extrémités", PENCIL_ENDS, Kraft, &d("Crayon tremblé, extrémités appuyées.")),
        sketch("Fusain", CHARCOAL, White, &d("Fusain épais, plus marqué au premier plan.")),
        sketch("Fusain", CHARCOAL, Grain, &d("Fusain épais, plus marqué au premier plan.")),
        sketch("Fusain", CHARCOAL, Kraft, &d("Fusain épais, plus marqué au premier plan.")),
        sketch("Encre de Chine", INK, White, &d("Encre noire, traits prolongés et extrémités.")),
        sketch("Encre de Chine", INK, Tracing, &d("Encre noire, traits prolongés et extrémités.")),
        sketch("Encre de Chine", INK, Cream, &d("Encre noire, traits prolongés et extrémités.")),
        sketch("Stylo bille", BALLPOINT, White, &d("Stylo bille bleu, trait léger.")),
        sketch("Stylo bille", BALLPOINT, Grid, &d("Stylo bille bleu, trait léger.")),
        sketch("Feutre fin", FINELINER, White, &d("Feutre fin, contours appuyés.")),
        sketch("Feutre fin", FINELINER, Tracing, &d("Feutre fin, contours appuyés.")),
        sketch("Marqueur", MARKER, White, &d("Marqueur large, contours épais.")),
        sketch("Marqueur", MARKER, Chipboard, &d("Marqueur large, contours épais.")),
        sketch("Sanguine", SANGUINE, Cream, &d("Craie sanguine.")),
        sketch("Sanguine", SANGUINE, Kraft, &d("Craie sanguine.")),
        sketch("Sépia", SEPIA, Cream, &d("Encre sépia, extrémités marquées.")),
        sketch("Sépia", SEPIA, Grain, &d("Encre sépia, extrémités marquées.")),
        sketch("Craie", CHALK, Slate, &d("Craie blanche sur fond sombre.")),
        sketch("Craie", CHALK, Greenboard, &d("Craie blanche sur fond vert.")),
        sketch("Pastel", PASTEL, Grain, &d("Arêtes de la couleur des matières.")),
        sketch("Pastel", PASTEL, Kraft, &d("Arêtes de la couleur des matières.")),
        sketch("Plume", QUILL, White, &d("Plume fine, longs prolongements.")),
        sketch("Plume", QUILL, Cream, &d("Plume fine, longs prolongements.")),
        sketch("Stylo technique", TECH_PEN, White, &d("Trait régulier, extrémités légères.")),
        sketch("Stylo technique", TECH_PEN, Tracing, &d("Trait régulier, extrémités légères.")),
        sketch("Aquarelle", WATERCOLOR, Grain, &d("Arêtes de la couleur des matières, contours doux.")),
        sketch("Aquarelle", WATERCOLOR, White, &d("Arêtes de la couleur des matières, contours doux.")),
        sketch("Crayon bleu", BLUE_PENCIL, White, &d("Crayon de couleur bleu.")),
        sketch("Crayon rouge", RED_PENCIL, White, &d("Crayon de couleur rouge.")),
        sketch("Feutre gris", GREY_MARKER, White, &d("Feutre gris moyen.")),
        sketch("Fusain", CHARCOAL, Chipboard, &d("Fusain épais, plus marqué au premier plan.")),
        sketch("Encre bleue", BLUE_INK, Grid, &d("Encre bleue, extrémités marquées.")),
        sketch("Mine de plomb", LEAD, Tracing, &d("Mine claire, trait léger.")),
    ]
}

fn assorted() -> Vec<Def> {
    vec![
        def(
            "Tirage de plan",
            "Faces et fond bleus, arêtes blanches fines, à la manière des anciens tirages.",
            |s| {
                s.front_color = [40, 80, 150];
                s.back_color = [30, 60, 120];
                s.bg_color = [25, 60, 125];
                s.sky = false;
                s.edge_color = [235, 240, 250];
                s.profiles = false;
            },
        ),
        def("Écran ambre", "Un moniteur monochrome d'autrefois : orange sur noir, filaire.", |s| {
            s.set_faces(FaceStyle::HiddenLine);
            s.bg_color = [10, 8, 4];
            s.sky = false;
            s.edge_color = [255, 160, 30];
        }),
        def("CAO sombre", "Faces grises sur fond anthracite, arêtes blanches, axes visibles.", |s| {
            s.front_color = [110, 112, 115];
            s.back_color = [70, 72, 75];
            s.bg_color = [45, 47, 50];
            s.sky = false;
            s.edge_color = [235, 235, 235];
            s.axes = true;
        }),
        def("Maquette en carton", "Faces couleur carton, arêtes fines brunes, fond clair.", |s| {
            s.set_faces(FaceStyle::Monochrome);
            s.front_color = [200, 170, 125];
            s.back_color = [160, 130, 90];
            s.edge_color = [95, 70, 45];
            s.bg_color = [235, 230, 220];
            s.sky = false;
        }),
        def(
            "Maquette blanche",
            "Tout en blanc avec une occlusion ambiante douce, comme une maquette en plâtre.",
            |s| {
                s.set_faces(FaceStyle::Monochrome);
                s.front_color = [250, 250, 248];
                s.back_color = [230, 230, 228];
                s.ambient_occlusion = true;
                s.profiles = false;
                s.edge_color = [150, 150, 150];
                s.bg_color = [240, 240, 240];
                s.sky = false;
            },
        ),
        def("Pâte à modeler", "Couleurs vives et douces, sans arêtes, occlusion légère.", |s| {
            s.edges = false;
            s.profiles = false;
            s.ambient_occlusion = true;
            s.front_color = [240, 200, 160];
            s.bg_color = [250, 240, 225];
            s.sky = false;
        }),
        def("Nuit étoilée", "Fond bleu nuit, faces sombres, arêtes claires.", |s| {
            s.front_color = [60, 66, 90];
            s.back_color = [40, 44, 60];
            s.bg_color = [12, 16, 36];
            s.sky_color = [25, 35, 70];
            s.edge_color = [200, 210, 240];
        }),
        def("Brouillard du matin", "Ciel pâle et brouillard : le lointain se perd.", |s| {
            s.fog = true;
            s.sky_color = [205, 215, 225];
            s.bg_color = [220, 222, 225];
        }),
        def("Néon", "Fond noir, faces sombres et arêtes cyan lumineuses.", |s| {
            s.front_color = [25, 25, 30];
            s.back_color = [15, 15, 20];
            s.bg_color = [0, 0, 0];
            s.sky = false;
            s.edge_color = [0, 230, 230];
            s.profile_width = 3;
        }),
        def("Sépia", "Faces crème, arêtes brunes, fond vieux papier.", |s| {
            s.set_faces(FaceStyle::Monochrome);
            s.front_color = [240, 225, 195];
            s.back_color = [200, 180, 150];
            s.edge_color = [100, 70, 45];
            Paper::Cream.apply(s);
        }),
        def("Lavis", "Faces aux couleurs claires des matières, arêtes fines, papier à grain.", |s| {
            s.edge_color_mode = EdgeColor::ByMaterial;
            Paper::Grain.apply(s);
        }),
        def("Vitrail", "Faces translucides aux couleurs des matières, arêtes noires épaisses.", |s| {
            s.xray = true;
            s.profile_width = 4;
            s.bg_color = [250, 250, 250];
            s.sky = false;
        }),
        def("Contre-jour", "Silhouettes sombres sur un ciel clair et chaud.", |s| {
            s.set_faces(FaceStyle::Monochrome);
            s.front_color = [55, 50, 60];
            s.back_color = [40, 36, 45];
            s.sky_color = [250, 200, 140];
            s.bg_color = [235, 160, 110];
            s.edge_color = [20, 18, 22];
        }),
    ]
}

fn photo_modeling() -> Vec<Def> {
    vec![
        def(
            "Modélisation d'après photo",
            "Rayons X et arêtes rouges : la photo reste visible à travers le modèle.",
            |s| {
                s.xray = true;
                s.edge_color = [220, 30, 30];
                s.profiles = false;
            },
        ),
        def(
            "Modélisation d'après photo, arêtes par axe",
            "Faces masquées, arêtes colorées selon l'axe qu'elles suivent.",
            |s| {
                s.set_faces(FaceStyle::Wireframe);
                s.edge_color_mode = EdgeColor::ByAxis;
                s.profiles = false;
            },
        ),
        def(
            "Modélisation d'après photo, arêtes claires",
            "Rayons X et arêtes blanches, pour les photos sombres.",
            |s| {
                s.xray = true;
                s.edge_color = [255, 255, 255];
                s.profiles = false;
            },
        ),
    ]
}

fn ambient_occlusion() -> Vec<Def> {
    let ao = |s: &mut ViewStyle, distance: f32, intensity: f32| {
        s.ambient_occlusion = true;
        s.ao_distance = distance;
        s.ao_intensity = intensity;
    };
    vec![
        def("Occlusion douce", "Faces par défaut, occlusion étendue et légère, sans profils.", move |s| {
            ao(s, 600.0, 0.4);
            s.profiles = false;
        }),
        def("Occlusion marquée", "Occlusion courte et forte, profils fins.", move |s| ao(s, 250.0, 0.9)),
        def("Argile", "Maquette d'argile : faces terre, sans arêtes, occlusion douce.", move |s| {
            ao(s, 500.0, 0.6);
            s.set_faces(FaceStyle::Monochrome);
            s.front_color = [196, 140, 100];
            s.back_color = [160, 110, 80];
            s.edges = false;
            s.profiles = false;
        }),
        def("Intérieur clair", "Fond blanc, sans axes, occlusion courte : pour les pièces.", move |s| {
            ao(s, 300.0, 0.6);
            s.bg_color = [255, 255, 255];
            s.sky = false;
            s.axes = false;
        }),
        def("Extérieur", "Textures, sol et ciel, occlusion marquée.", move |s| {
            ao(s, 800.0, 0.8);
            s.ground = true;
        }),
        def("Croquis ombré", "Crayon tremblé sur papier crème, occlusion riche.", move |s| {
            ao(s, 500.0, 0.8);
            PENCIL.apply(s);
            Paper::Cream.apply(s);
        }),
        def("Détails", "Pour les petits objets : axes, sans profils, occlusion très courte.", move |s| {
            ao(s, 80.0, 0.7);
            s.profiles = false;
            s.axes = true;
        }),
    ]
}

fn signature_styles() -> Vec<Def> {
    use Paper::*;
    let with = |t: Tool, p: Paper, border: bool| {
        move |s: &mut ViewStyle| {
            t.apply(s);
            p.apply(s);
            if border {
                whiteout(s);
            }
        }
    };
    vec![
        def(
            "Plume sur vélin",
            "Plume fine à longs prolongements sur papier crème, bordure blanchie.",
            with(QUILL, Cream, true),
        ),
        def("Crayon gras sur kraft", "Crayon épais tremblé sur papier kraft.", move |s| {
            Tool { profile: 3, ..PENCIL }.apply(s);
            Kraft.apply(s);
        }),
        def(
            "Encre et lavis",
            "Encre noire et faces aux couleurs claires des matières sur papier à grain.",
            with(INK, Grain, false),
        ),
        def("Croquis de voyage", "Encre sépia sur papier crème, bordure blanchie.", with(SEPIA, Cream, true)),
        def("Carnet quadrillé", "Stylo bille bleu sur papier quadrillé.", with(BALLPOINT, Grid, false)),
        def("Mine de plomb détourée", "Mine claire sur papier blanc, bordure blanchie.", with(LEAD, White, true)),
        def("Feutre sur calque", "Feutre fin sur calque, bordure blanchie.", with(FINELINER, Tracing, true)),
        def("Esquisse au fusain", "Fusain épais sur papier à grain, bordure blanchie.", with(CHARCOAL, Grain, true)),
        def(
            "Trait d'architecte",
            "Stylo technique, extrémités et prolongements, sur toile.",
            with(TECH_PEN, Canvas, false),
        ),
        def("Plan d'atelier", "Encre bleue en lignes cachées sur papier quadrillé.", move |s| {
            BLUE_INK.apply(s);
            Grid.apply(s);
            s.set_faces(FaceStyle::HiddenLine);
        }),
    ]
}

// --------------------------------------------------------------------------- thumbnails

/// A little house with a roof, a door and a tree-like block: what the thumbnails show.
fn scene() -> Model {
    let mut m = Model::new();
    let roof = m.add_material("Toit", [150, 70, 50]);
    let door = m.add_material("Porte", [90, 110, 140]);
    let leaf = m.add_material("Feuillage", [80, 130, 60]);
    let quad = |m: &mut Model, pts: [DVec3; 4], mat: Option<esq_core::model::MaterialId>| {
        let v: Vec<_> = pts.iter().map(|p| m.root.add_vertex_raw(*p)).collect();
        if let Some(f) = m.root.add_face_raw(v, Vec::new()) {
            m.root.faces[f].front = mat;
            m.root.faces[f].back = mat;
        }
    };
    let p = DVec3::new;
    let (w, d, h, ridge) = (4000.0, 3000.0, 2500.0, 3800.0);
    // Walls.
    quad(&mut m, [p(0.0, 0.0, 0.0), p(w, 0.0, 0.0), p(w, 0.0, h), p(0.0, 0.0, h)], None);
    quad(&mut m, [p(w, 0.0, 0.0), p(w, d, 0.0), p(w, d, h), p(w, 0.0, h)], None);
    quad(&mut m, [p(w, d, 0.0), p(0.0, d, 0.0), p(0.0, d, h), p(w, d, h)], None);
    quad(&mut m, [p(0.0, d, 0.0), p(0.0, 0.0, 0.0), p(0.0, 0.0, h), p(0.0, d, h)], None);
    // Gables and roof.
    for x in [0.0, w] {
        let v: Vec<_> =
            [p(x, 0.0, h), p(x, d, h), p(x, d / 2.0, ridge)].iter().map(|q| m.root.add_vertex_raw(*q)).collect();
        m.root.add_face_raw(v, Vec::new());
    }
    quad(
        &mut m,
        [
            p(-200.0, -300.0, h - 200.0),
            p(w + 200.0, -300.0, h - 200.0),
            p(w + 200.0, d / 2.0, ridge),
            p(-200.0, d / 2.0, ridge),
        ],
        Some(roof),
    );
    quad(
        &mut m,
        [
            p(w + 200.0, d + 300.0, h - 200.0),
            p(-200.0, d + 300.0, h - 200.0),
            p(-200.0, d / 2.0, ridge),
            p(w + 200.0, d / 2.0, ridge),
        ],
        Some(roof),
    );
    // Door.
    quad(
        &mut m,
        [p(1600.0, -10.0, 0.0), p(2400.0, -10.0, 0.0), p(2400.0, -10.0, 2000.0), p(1600.0, -10.0, 2000.0)],
        Some(door),
    );
    // A block of foliage beside the house.
    let (x0, y0, s, z) = (4800.0, 800.0, 1400.0, 2600.0);
    quad(&mut m, [p(x0, y0, 0.0), p(x0 + s, y0, 0.0), p(x0 + s, y0, z), p(x0, y0, z)], Some(leaf));
    quad(&mut m, [p(x0 + s, y0, 0.0), p(x0 + s, y0 + s, 0.0), p(x0 + s, y0 + s, z), p(x0 + s, y0, z)], Some(leaf));
    quad(&mut m, [p(x0, y0, z), p(x0 + s, y0, z), p(x0 + s, y0 + s, z), p(x0, y0 + s, z)], Some(leaf));
    m.rebuild_topology();
    m
}

/// The thumbnail: the scene drawn at twice the size, then reduced (finer lines), without the
/// axes.
fn thumbnail(gpu: &mut Gpu, m: &Model, s: &ViewStyle) -> Vec<u8> {
    let (big, small) = (192u32, 96u32);
    let mut cam = Camera { size: DVec2::new(big as f64, big as f64), ..Default::default() };
    cam.set_view(-58.0, 16.0);
    cam.zoom_extents(DVec3::new(-200.0, -300.0, 0.0), DVec3::new(6200.0, 3300.0, 3800.0));
    cam.distance *= 0.82;
    let s = ViewStyle { axes: false, ..s.clone() };
    let shot = gpu.render(m, &s, &cam, &Lighting::default());
    let img = image::RgbaImage::from_raw(shot.w, shot.h, shot.px).expect("pixels");
    let img = image::imageops::resize(&img, small, small, image::imageops::FilterType::Triangle);
    png_rgba(small, small, img.into_raw())
}

/// Every style of the library, with its thumbnail.
pub fn all(gpu: &mut Gpu) -> Vec<Entry> {
    let groups: Vec<(&'static str, Vec<Def>)> = vec![
        ("Ambient Occlusion", ambient_occlusion()),
        ("Assorted Styles", assorted()),
        ("Color Sets", color_sets()),
        ("Default Styles", default_styles()),
        ("Photo Modeling", photo_modeling()),
        ("Sketchy Edges", sketchy()),
        ("Straight Lines", straight_lines()),
        ("Style Builder Competition Winners", signature_styles()),
    ];
    let scene = scene();
    let mut out = Vec::new();
    for (folder, defs) in groups {
        for d in defs {
            let mut s = ViewStyle::default();
            (d.tweak)(&mut s);
            let thumb = thumbnail(gpu, &scene, &s);
            let style = StyleFile {
                name: d.name.clone(),
                description: d.desc.trim().to_string(),
                ro: s.style_snapshot(),
                watermarks: s.watermark_list.clone(),
                thumbnail: Some(thumb),
                sketchy: false,
            };
            out.push(Entry { folder, file: super::materials::file_stem(&d.name), style });
        }
    }
    out
}
