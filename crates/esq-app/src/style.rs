//! Display settings: what the View menu, the Styles panel and the Fog panel set. They are
//! SketchUp's rendering options (`Sketchup::RenderingOptions`): `ro_get` / `ro_set` read and
//! write them under SketchUp's key names, so that Ruby scripts, the styles of the model (a
//! snapshot of these keys) and the interface share the same settings.

use esq_core::photo::Blob;
use serde::{Deserialize, Serialize};
use serde_json::{Value as J, json};

/// View › Face Style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceStyle {
    Wireframe,
    HiddenLine,
    Shaded,
    ShadedWithTextures,
    Monochrome,
}

impl FaceStyle {
    pub const ALL: [FaceStyle; 5] = [
        FaceStyle::Wireframe,
        FaceStyle::HiddenLine,
        FaceStyle::Shaded,
        FaceStyle::ShadedWithTextures,
        FaceStyle::Monochrome,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FaceStyle::Wireframe => "Filaire",
            FaceStyle::HiddenLine => "Ligne cachée",
            FaceStyle::Shaded => "Ombré",
            FaceStyle::ShadedWithTextures => "Ombré avec textures",
            FaceStyle::Monochrome => "Monochrome",
        }
    }

    /// SketchUp's `RenderMode` (shaded with or without textures is the `Texture` flag).
    pub fn render_mode(self) -> i64 {
        match self {
            FaceStyle::Wireframe => 0,
            FaceStyle::HiddenLine => 1,
            FaceStyle::Shaded | FaceStyle::ShadedWithTextures => 2,
            FaceStyle::Monochrome => 4,
        }
    }
}

/// Edge settings › Color (SketchUp's `EdgeColorMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeColor {
    ByMaterial = 0,
    AllSame = 1,
    ByAxis = 2,
}

impl EdgeColor {
    /// In the order of the Styles panel's list.
    pub const ALL: [EdgeColor; 3] = [EdgeColor::AllSame, EdgeColor::ByMaterial, EdgeColor::ByAxis];

    pub fn label(self) -> &'static str {
        match self {
            EdgeColor::AllSame => "Toutes identiques",
            EdgeColor::ByMaterial => "Par matière",
            EdgeColor::ByAxis => "Par axe",
        }
    }

    pub fn of(n: i64) -> Option<EdgeColor> {
        Some(match n {
            0 => EdgeColor::ByMaterial,
            1 => EdgeColor::AllSame,
            2 => EdgeColor::ByAxis,
            _ => return None,
        })
    }
}

/// How a watermark covers the view (Styles › Edit › Watermark).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WmLayout {
    /// Stretched to fit the screen (optionally keeping its proportions).
    Stretched,
    /// Repeated across the screen, at `scale`.
    Tiled,
    /// Once, at one of nine places of the screen, at `scale`.
    Positioned,
}

/// A watermark of the style: an image drawn behind the model or over it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Watermark {
    pub name: String,
    /// The image file (PNG, JPEG).
    pub image: Blob,
    /// Behind the model (background) or over it (overlay).
    pub background: bool,
    /// Opacity, 0–1 (the Blend slider).
    pub blend: f64,
    /// Brightness is used as transparency: dark parts show the background colour, white
    /// parts are transparent.
    pub mask: bool,
    pub layout: WmLayout,
    /// Stretched: keep the image's proportions.
    pub keep_aspect: bool,
    /// Tiled and positioned: size relative to the screen.
    pub scale: f64,
    /// Positioned: 0–8, row by row from the top left (4 = centre).
    pub position: u8,
    /// What a .skp of SketchUp's says of it besides (the file it was loaded from, when),
    /// written back with it (`esq_skp::native::style::wm_json`).
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub skp: serde_json::Value,
}

/// The fog settings that are distances: unset (negative) until fog is first shown.
pub const FOG_UNSET: f64 = -1.0;

#[derive(Clone, Debug, PartialEq)]
pub struct ViewStyle {
    // ----------------------------------------------------------------- edges
    pub edges: bool,
    pub back_edges: bool,
    pub profiles: bool,
    /// Profiles width, pixels (`SilhouetteWidth`).
    pub profile_width: u32,
    pub extension: bool,
    /// Extension length, pixels (`LineExtension`).
    pub extension_length: u32,
    /// Edge style › Depth cue (SketchUp's `DrawDepthQue` / `DepthQueWidth`): edges nearer to
    /// the eye are drawn thicker, up to `depth_cue_width` pixels.
    pub depth_cue: bool,
    /// 1 to 4 in SketchUp's Styles panel (default 4).
    pub depth_cue_width: u32,
    /// Edge style › Endpoints (`DrawLineEnds`): the ends of edges are drawn thicker over
    /// `endpoint_length` pixels.
    pub endpoints: bool,
    pub endpoint_length: u32,
    /// Edge style › Jitter (`JitterEdges`): edges drawn several times, slightly offset.
    pub jitter: bool,
    /// Edge style › Dashes: edges show their tag's dash pattern.
    pub dashes: bool,
    pub edge_color_mode: EdgeColor,
    /// Edge colour when all the same (`ForegroundColor`).
    pub edge_color: [u8; 3],
    /// `EdgeType`, kept for the Ruby API.
    pub edge_type: i64,
    // ----------------------------------------------------------------- faces
    pub faces: FaceStyle,
    /// The `Texture` flag, kept apart from the face style as SketchUp does.
    pub texture: bool,
    /// X-ray (`ModelTransparency`) and the opacity it gives faces (0.2–0.8).
    pub xray: bool,
    pub xray_opacity: f64,
    pub front_color: [u8; 3],
    pub back_color: [u8; 3],
    /// Face settings › Enable transparency (`MaterialTransparency`) and its quality
    /// (`TransparencySort`: 0 faster, 1 nicer).
    pub transparency: bool,
    pub transparency_sort: i64,
    /// Face style › Ambient occlusion (`AmbientOcclusion*` rendering options).
    pub ambient_occlusion: bool,
    /// Occlusion radius in mm, strength and contrast (SketchUp's `…Distance`, `…Multiplier`,
    /// `…Intensity`).
    pub ao_distance: f32,
    pub ao_multiplier: f32,
    pub ao_intensity: f32,
    /// Face style › Photoreal materials: PBR shading (roughness, metalness, sky reflection).
    pub photoreal: bool,
    // ----------------------------------------------------------------- background
    pub bg_color: [u8; 3],
    /// Background › Sky (SketchUp's "DrawHorizon") and its colour.
    pub sky: bool,
    pub sky_color: [u8; 3],
    /// `HorizonColor`, kept for the Ruby API.
    pub horizon_color: [u8; 4],
    /// Background › Ground, its colour, transparency (0–100) and whether it shows from below.
    pub ground: bool,
    pub ground_color: [u8; 3],
    pub ground_transparency: u32,
    pub underground: bool,
    // ----------------------------------------------------------------- watermarks
    pub watermarks: bool,
    /// View › Grid: the modeling grid (`model_grid.rs`).
    pub modeling_grid: bool,
    pub watermark_list: Vec<Watermark>,
    // ----------------------------------------------------------------- modeling
    pub highlight_color: [u8; 3],
    pub locked_color: [u8; 3],
    pub guide_color: [u8; 3],
    pub section_active_color: [u8; 3],
    pub section_inactive_color: [u8; 3],
    pub section_cut_color: [u8; 3],
    pub section_fill_color: [u8; 3],
    pub section_cut_width: u32,
    pub hidden_geometry: bool,
    pub hidden_objects: bool,
    /// Modeling › Color by tag (`DisplayColorByLayer`).
    pub color_by_tag: bool,
    /// View › Section Planes, Section Cuts, Section Fill.
    pub section_planes: bool,
    pub section_cuts: bool,
    pub section_fill: bool,
    pub guides: bool,
    pub axes: bool,
    pub instance_axes: bool,
    /// Match Photo: the photo behind the model, the model over it, and their opacities.
    pub photo_bg: bool,
    pub photo_bg_opacity: f64,
    pub photo_fg: bool,
    pub photo_fg_opacity: f64,
    /// Fading of the rest of the model while editing a component (Model Info › Components).
    pub inactive_fade: f64,
    pub instance_fade: f64,
    // ----------------------------------------------------------------- fog
    pub fog: bool,
    pub fog_color: [u8; 3],
    pub fog_use_bg: bool,
    /// Fog starts and is complete at these distances from the eye (mm, `FOG_UNSET` until set).
    pub fog_start: f64,
    pub fog_end: f64,
    // ----------------------------------------------------------------- application only
    /// Component editing: hide the rest of the model, hide similar components.
    pub hide_rest: bool,
    pub hide_similar: bool,
    /// View › Scene Tabs.
    pub scene_tabs: bool,
}

impl Default for ViewStyle {
    /// The style of SketchUp 2026's "Simple" templates (rendering options read from the
    /// template through the SDK).
    fn default() -> Self {
        ViewStyle {
            edges: true,
            back_edges: false,
            profiles: true,
            profile_width: 2,
            extension: false,
            extension_length: 3,
            depth_cue: false,
            depth_cue_width: 4,
            endpoints: false,
            endpoint_length: 9,
            jitter: false,
            dashes: true,
            edge_color_mode: EdgeColor::AllSame,
            edge_color: [0, 0, 0],
            edge_type: 0,
            faces: FaceStyle::ShadedWithTextures,
            texture: true,
            xray: false,
            xray_opacity: 0.65,
            front_color: [255, 255, 255],
            back_color: [162, 183, 198],
            transparency: true,
            transparency_sort: 0,
            ambient_occlusion: false,
            // Not read from SketchUp (its defaults were not found): chosen by eye.
            ao_distance: 600.0,
            ao_multiplier: 1.0,
            ao_intensity: 1.0,
            photoreal: false,
            bg_color: [191, 191, 198],
            sky: true,
            sky_color: [0, 154, 217],
            horizon_color: [0, 0, 0, 0],
            ground: false,
            ground_color: [151, 170, 136],
            ground_transparency: 50,
            underground: true,
            watermarks: true,
            modeling_grid: false,
            watermark_list: Vec::new(),
            highlight_color: [0, 1, 255],
            locked_color: [255, 0, 0],
            guide_color: [0, 0, 0],
            section_active_color: [255, 135, 0],
            section_inactive_color: [112, 105, 97],
            section_cut_color: [0, 0, 0],
            section_fill_color: [0, 0, 0],
            section_cut_width: 3,
            hidden_geometry: false,
            hidden_objects: false,
            color_by_tag: false,
            // The Simple template hides section planes until one is placed.
            section_planes: false,
            section_cuts: true,
            section_fill: true,
            guides: true,
            axes: true,
            instance_axes: false,
            photo_bg: true,
            photo_bg_opacity: 1.0,
            photo_fg: true,
            photo_fg_opacity: 0.8,
            inactive_fade: 0.25,
            instance_fade: 0.5,
            fog: false,
            fog_color: [191, 191, 198],
            fog_use_bg: true,
            fog_start: FOG_UNSET,
            fog_end: FOG_UNSET,
            hide_rest: false,
            hide_similar: false,
            scene_tabs: true,
        }
    }
}

const INCH: f64 = 25.4;

fn color(c: [u8; 3]) -> J {
    json!([c[0], c[1], c[2], 255])
}

/// A colour given as `[r, g, b(, a)]` (Ruby's `Color#to_a`).
pub fn color_of(v: &J) -> Option<[u8; 3]> {
    let a = v.as_array().filter(|a| a.len() >= 3)?;
    let c = |i: usize| a[i].as_f64().map(|f| f.round().clamp(0.0, 255.0) as u8);
    Some([c(0)?, c(1)?, c(2)?])
}

/// The rendering options that `ViewStyle` holds, by SketchUp's key names. Those marked as
/// style settings make up a style (the others are the model's: fog, fading of inactive
/// geometry…).
pub const RO_KEYS: &[&str] = &[
    "AmbientOcclusion",
    "AmbientOcclusionDistance",
    "AmbientOcclusionIntensity",
    "AmbientOcclusionMultiplier",
    "BackgroundColor",
    "ConstructionColor",
    "DepthQueWidth",
    "DisplayColorByLayer",
    "DisplayFog",
    "DisplayInstanceAxes",
    "DisplaySectionCuts",
    "DisplaySectionPlanes",
    "DisplaySketchAxes",
    "DisplayWatermarks",
    "ModelingGrid",
    "DrawBackEdges",
    "DrawDepthQue",
    "DrawGround",
    "DrawHidden",
    "DrawHiddenGeometry",
    "DrawHiddenObjects",
    "DrawHorizon",
    "DrawLineEnds",
    "DrawSilhouettes",
    "DrawUnderground",
    "EdgeColorMode",
    "EdgeDisplayMode",
    "EdgeType",
    "ExtendLines",
    "FaceBackColor",
    "FaceFrontColor",
    "FogColor",
    "FogEndDist",
    "FogStartDist",
    "FogUseBkColor",
    "ForegroundColor",
    "GroundColor",
    "GroundTransparency",
    "HideConstructionGeometry",
    "HighlightColor",
    "HorizonColor",
    "InactiveFade",
    "InactiveHidden",
    "InstanceFade",
    "InstanceHidden",
    "JitterEdges",
    "LineEndWidth",
    "LineExtension",
    "LineStyleEdges",
    "LockedColor",
    "MaterialTransparency",
    "ModelTransparency",
    "PhotomatchBackgroundOpacity",
    "PhotomatchDrawBackground",
    "PhotomatchDrawOverlay",
    "PhotomatchOverlayOpacity",
    "RenderMode",
    "SectionActiveColor",
    "SectionCutFilled",
    "SectionCutWidth",
    "SectionDefaultCutColor",
    "SectionDefaultFillColor",
    "SectionInactiveColor",
    "SilhouetteWidth",
    "SkyColor",
    "Texture",
    "TransparencySort",
    "XRayOpacity",
];

/// The keys that belong to the model rather than to its style (a style does not change them).
pub const MODEL_KEYS: &[&str] = &[
    "DisplayFog",
    "FogColor",
    "FogEndDist",
    "FogStartDist",
    "FogUseBkColor",
    "InactiveFade",
    "InactiveHidden",
    "InstanceFade",
    "InstanceHidden",
    "DisplayInstanceAxes",
];

impl ViewStyle {
    /// Flags passed to the line shader.
    pub fn line_flags(&self) -> u32 {
        (self.edges as u32) | (self.profiles as u32) << 1 | (self.extension as u32) << 2
    }

    /// The colour fog fades to.
    pub fn fog_rgb(&self) -> [u8; 3] {
        if self.fog_use_bg { self.bg_color } else { self.fog_color }
    }

    /// A rendering option, as Ruby's `RenderingOptions#[]` gives it (colours as
    /// `[r, g, b, a]`, lengths in inches); `None` for a key this style does not hold.
    pub fn ro_get(&self, key: &str) -> Option<J> {
        let b = |v: bool| json!(v);
        let n = |v: i64| json!(v);
        Some(match key {
            "AmbientOcclusion" => b(self.ambient_occlusion),
            "AmbientOcclusionDistance" => json!(self.ao_distance as f64 / INCH),
            "AmbientOcclusionIntensity" => json!(self.ao_intensity as f64),
            "AmbientOcclusionMultiplier" => json!(self.ao_multiplier as f64),
            "BackgroundColor" => color(self.bg_color),
            "ConstructionColor" => color(self.guide_color),
            "DepthQueWidth" => n(self.depth_cue_width as i64),
            "DisplayColorByLayer" => b(self.color_by_tag),
            "DisplayFog" => b(self.fog),
            "DisplayInstanceAxes" => b(self.instance_axes),
            "DisplaySectionCuts" => b(self.section_cuts),
            "DisplaySectionPlanes" => b(self.section_planes),
            "DisplaySketchAxes" => b(self.axes),
            "DisplayWatermarks" => b(self.watermarks),
            "ModelingGrid" => b(self.modeling_grid),
            "DrawBackEdges" => b(self.back_edges),
            "DrawDepthQue" => b(self.depth_cue),
            "DrawGround" => b(self.ground),
            "DrawHidden" => b(self.hidden_geometry || self.hidden_objects),
            "DrawHiddenGeometry" => b(self.hidden_geometry),
            "DrawHiddenObjects" => b(self.hidden_objects),
            "DrawHorizon" => b(self.sky),
            "DrawLineEnds" => b(self.endpoints),
            "DrawSilhouettes" => b(self.profiles),
            "DrawUnderground" => b(self.underground),
            "EdgeColorMode" => n(self.edge_color_mode as i64),
            "EdgeDisplayMode" => n(self.edges as i64),
            "EdgeType" => n(self.edge_type),
            "ExtendLines" => b(self.extension),
            "FaceBackColor" => color(self.back_color),
            "FaceFrontColor" => color(self.front_color),
            "FogColor" => color(self.fog_color),
            "FogEndDist" => json!(if self.fog_end < 0.0 { FOG_UNSET } else { self.fog_end / INCH }),
            "FogStartDist" => json!(if self.fog_start < 0.0 { FOG_UNSET } else { self.fog_start / INCH }),
            "FogUseBkColor" => b(self.fog_use_bg),
            "ForegroundColor" => color(self.edge_color),
            "GroundColor" => color(self.ground_color),
            "GroundTransparency" => n(self.ground_transparency as i64),
            "HideConstructionGeometry" => b(!self.guides),
            "HighlightColor" => color(self.highlight_color),
            "HorizonColor" => json!(self.horizon_color),
            "InactiveFade" => json!(self.inactive_fade),
            "InactiveHidden" => b(self.hide_rest),
            "InstanceFade" => json!(self.instance_fade),
            "InstanceHidden" => b(self.hide_similar),
            "JitterEdges" => b(self.jitter),
            "LineEndWidth" => n(self.endpoint_length as i64),
            "LineExtension" => n(self.extension_length as i64),
            "LineStyleEdges" => b(self.dashes),
            "LockedColor" => color(self.locked_color),
            "MaterialTransparency" => b(self.transparency),
            "ModelTransparency" => b(self.xray),
            "PhotomatchBackgroundOpacity" => json!(self.photo_bg_opacity),
            "PhotomatchDrawBackground" => b(self.photo_bg),
            "PhotomatchDrawOverlay" => b(self.photo_fg),
            "PhotomatchOverlayOpacity" => json!(self.photo_fg_opacity),
            "RenderMode" => n(self.faces.render_mode()),
            "SectionActiveColor" => color(self.section_active_color),
            "SectionCutFilled" => b(self.section_fill),
            "SectionCutWidth" => n(self.section_cut_width as i64),
            "SectionDefaultCutColor" => color(self.section_cut_color),
            "SectionDefaultFillColor" => color(self.section_fill_color),
            "SectionInactiveColor" => color(self.section_inactive_color),
            "SilhouetteWidth" => n(self.profile_width as i64),
            "SkyColor" => color(self.sky_color),
            "Texture" => b(self.texture),
            "TransparencySort" => n(self.transparency_sort),
            "XRayOpacity" => json!(self.xray_opacity),
            _ => return None,
        })
    }

    /// Sets a rendering option. `Some(false)` when the value does not suit the key, `None`
    /// for a key the style does not hold.
    pub fn ro_set(&mut self, key: &str, v: &J) -> Option<bool> {
        let flag = v.as_bool();
        let int = v.as_f64().map(|f| f.round() as i64);
        let num = v.as_f64();
        let col = color_of(v);
        let px = |n: i64, lo: i64, hi: i64| n.clamp(lo, hi) as u32;
        Some(match key {
            "AmbientOcclusion" => flag.map(|f| self.ambient_occlusion = f).is_some(),
            "AmbientOcclusionDistance" => num.map(|n| self.ao_distance = (n * INCH) as f32).is_some(),
            "AmbientOcclusionIntensity" => num.map(|n| self.ao_intensity = n as f32).is_some(),
            "AmbientOcclusionMultiplier" => num.map(|n| self.ao_multiplier = n as f32).is_some(),
            "BackgroundColor" => col.map(|c| self.bg_color = c).is_some(),
            "ConstructionColor" => col.map(|c| self.guide_color = c).is_some(),
            "DepthQueWidth" => int.map(|n| self.depth_cue_width = px(n, 1, 4)).is_some(),
            "DisplayColorByLayer" => flag.map(|f| self.color_by_tag = f).is_some(),
            "DisplayFog" => flag.map(|f| self.fog = f).is_some(),
            "DisplayInstanceAxes" => flag.map(|f| self.instance_axes = f).is_some(),
            "DisplaySectionCuts" => flag.map(|f| self.section_cuts = f).is_some(),
            "DisplaySectionPlanes" => flag.map(|f| self.section_planes = f).is_some(),
            "DisplaySketchAxes" => flag.map(|f| self.axes = f).is_some(),
            "DisplayWatermarks" => flag.map(|f| self.watermarks = f).is_some(),
            "ModelingGrid" => flag.map(|f| self.modeling_grid = f).is_some(),
            "DrawBackEdges" => flag.map(|f| self.back_edges = f).is_some(),
            "DrawDepthQue" => flag.map(|f| self.depth_cue = f).is_some(),
            "DrawGround" => flag.map(|f| self.ground = f).is_some(),
            "DrawHidden" => flag.map(|f| (self.hidden_geometry, self.hidden_objects) = (f, f)).is_some(),
            "DrawHiddenGeometry" => flag.map(|f| self.hidden_geometry = f).is_some(),
            "DrawHiddenObjects" => flag.map(|f| self.hidden_objects = f).is_some(),
            "DrawHorizon" => flag.map(|f| self.sky = f).is_some(),
            "DrawLineEnds" => flag.map(|f| self.endpoints = f).is_some(),
            "DrawSilhouettes" => flag.map(|f| self.profiles = f).is_some(),
            "DrawUnderground" => flag.map(|f| self.underground = f).is_some(),
            "EdgeColorMode" => int.and_then(EdgeColor::of).map(|m| self.edge_color_mode = m).is_some(),
            "EdgeDisplayMode" => int.map(|n| self.edges = n != 0).is_some(),
            "EdgeType" => int.map(|n| self.edge_type = n).is_some(),
            "ExtendLines" => flag.map(|f| self.extension = f).is_some(),
            "FaceBackColor" => col.map(|c| self.back_color = c).is_some(),
            "FaceFrontColor" => col.map(|c| self.front_color = c).is_some(),
            "FogColor" => col.map(|c| self.fog_color = c).is_some(),
            "FogEndDist" => num.map(|n| self.fog_end = if n < 0.0 { FOG_UNSET } else { n * INCH }).is_some(),
            "FogStartDist" => num.map(|n| self.fog_start = if n < 0.0 { FOG_UNSET } else { n * INCH }).is_some(),
            "FogUseBkColor" => flag.map(|f| self.fog_use_bg = f).is_some(),
            "ForegroundColor" => col.map(|c| self.edge_color = c).is_some(),
            "GroundColor" => col.map(|c| self.ground_color = c).is_some(),
            "GroundTransparency" => int.map(|n| self.ground_transparency = px(n, 0, 100)).is_some(),
            "HideConstructionGeometry" => flag.map(|f| self.guides = !f).is_some(),
            "HighlightColor" => col.map(|c| self.highlight_color = c).is_some(),
            "HorizonColor" => col
                .map(|c| {
                    let a = v.as_array().and_then(|a| a.get(3)).and_then(J::as_f64).unwrap_or(255.0);
                    self.horizon_color = [c[0], c[1], c[2], a.clamp(0.0, 255.0) as u8];
                })
                .is_some(),
            "InactiveFade" => num.map(|n| self.inactive_fade = n.clamp(0.0, 1.0)).is_some(),
            "InactiveHidden" => flag.map(|f| self.hide_rest = f).is_some(),
            "InstanceFade" => num.map(|n| self.instance_fade = n.clamp(0.0, 1.0)).is_some(),
            "InstanceHidden" => flag.map(|f| self.hide_similar = f).is_some(),
            "JitterEdges" => flag.map(|f| self.jitter = f).is_some(),
            "LineEndWidth" => int.map(|n| self.endpoint_length = px(n, 1, 999)).is_some(),
            "LineExtension" => int.map(|n| self.extension_length = px(n, 1, 999)).is_some(),
            "LineStyleEdges" => flag.map(|f| self.dashes = f).is_some(),
            "LockedColor" => col.map(|c| self.locked_color = c).is_some(),
            "MaterialTransparency" => flag.map(|f| self.transparency = f).is_some(),
            "ModelTransparency" => flag.map(|f| self.xray = f).is_some(),
            "PhotomatchBackgroundOpacity" => num.map(|n| self.photo_bg_opacity = n.clamp(0.0, 1.0)).is_some(),
            "PhotomatchDrawBackground" => flag.map(|f| self.photo_bg = f).is_some(),
            "PhotomatchDrawOverlay" => flag.map(|f| self.photo_fg = f).is_some(),
            "PhotomatchOverlayOpacity" => num.map(|n| self.photo_fg_opacity = n.clamp(0.0, 1.0)).is_some(),
            "RenderMode" => match int {
                Some(0) => self.set_faces(FaceStyle::Wireframe),
                Some(1) => self.set_faces(FaceStyle::HiddenLine),
                Some(2) => self.set_faces(if self.texture { FaceStyle::ShadedWithTextures } else { FaceStyle::Shaded }),
                // Older scripts name "shaded with textures" 3.
                Some(3) => self.set_faces(FaceStyle::ShadedWithTextures),
                Some(4) => self.set_faces(FaceStyle::Monochrome),
                _ => false,
            },
            "SectionActiveColor" => col.map(|c| self.section_active_color = c).is_some(),
            "SectionCutFilled" => flag.map(|f| self.section_fill = f).is_some(),
            "SectionCutWidth" => int.map(|n| self.section_cut_width = px(n, 1, 999)).is_some(),
            "SectionDefaultCutColor" => col.map(|c| self.section_cut_color = c).is_some(),
            "SectionDefaultFillColor" => col.map(|c| self.section_fill_color = c).is_some(),
            "SectionInactiveColor" => col.map(|c| self.section_inactive_color = c).is_some(),
            "SilhouetteWidth" => int.map(|n| self.profile_width = px(n, 1, 999)).is_some(),
            "SkyColor" => col.map(|c| self.sky_color = c).is_some(),
            "Texture" => flag
                .map(|f| {
                    self.texture = f;
                    self.faces = match (f, self.faces) {
                        (true, FaceStyle::Shaded) => FaceStyle::ShadedWithTextures,
                        (false, FaceStyle::ShadedWithTextures) => FaceStyle::Shaded,
                        (_, other) => other,
                    }
                })
                .is_some(),
            "TransparencySort" => int.map(|n| self.transparency_sort = n.clamp(0, 2)).is_some(),
            "XRayOpacity" => num.map(|n| self.xray_opacity = n.clamp(0.2, 0.8)).is_some(),
            _ => return None,
        })
    }

    /// View › Face Style: also keeps the `Texture` flag in step.
    pub fn set_faces(&mut self, f: FaceStyle) -> bool {
        self.faces = f;
        match f {
            FaceStyle::ShadedWithTextures => self.texture = true,
            FaceStyle::Shaded => self.texture = false,
            _ => {}
        }
        true
    }

    /// Every rendering option this style holds, by key (what a style or a scene keeps).
    pub fn snapshot(&self) -> serde_json::Map<String, J> {
        let mut m = serde_json::Map::new();
        for k in RO_KEYS {
            if let Some(v) = self.ro_get(k) {
                m.insert(k.to_string(), v);
            }
        }
        m
    }

    /// The style settings only (the model's own settings, such as fog, are left out).
    pub fn style_snapshot(&self) -> serde_json::Map<String, J> {
        let mut m = self.snapshot();
        for k in MODEL_KEYS {
            m.remove(*k);
        }
        m
    }

    /// Sets every option of `h`; returns the keys this style does not hold (the caller keeps
    /// them with the model, for the Ruby API).
    pub fn restore(&mut self, h: &serde_json::Map<String, J>) -> serde_json::Map<String, J> {
        let mut rest = serde_json::Map::new();
        // RenderMode reads the Texture flag: set it first.
        if let Some(t) = h.get("Texture") {
            self.ro_set("Texture", t);
        }
        for (k, v) in h {
            if self.ro_set(k, v).is_none() {
                rest.insert(k.clone(), v.clone());
            }
        }
        rest
    }
}

/// Roughness (low byte) and metalness (next byte) of a material for the photoreal face
/// style, 0-255 each. The data model has no PBR fields yet, so they are guessed from the
/// material the way SketchUp's "auto-promote materials to PBR" does it from the material's
/// look: by name, and by transparency (glass is smooth).
pub fn pbr_bits(name: &str, opacity: f32) -> u32 {
    let n = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    let (rough, metal) = if opacity < 0.95 || has(&["verre", "glass", "vitre"]) {
        (0.08, 0.0)
    } else if has(&["acier", "steel", "inox", "alu", "chrome", "métal", "metal", "fer", "iron", "zinc", "titan"]) {
        (0.3, 1.0)
    } else if has(&["cuivre", "copper", "laiton", "brass", "bronze", "or ", "gold", "argent", "silver"]) {
        (0.25, 1.0)
    } else if has(&[
        "plastique",
        "plastic",
        "vernis",
        "varnish",
        "laque",
        "lacquer",
        "carrelage",
        "tile",
        "céram",
        "cerami",
    ]) {
        (0.35, 0.0)
    } else if has(&["béton", "beton", "concrete", "plâtre", "platre", "plaster", "brique", "brick", "pierre", "stone"])
    {
        (0.9, 0.0)
    } else {
        (PBR_DEFAULT_ROUGHNESS, 0.0)
    };
    (rough * 255.0f32).round() as u32 | ((metal * 255.0f32).round() as u32) << 8
}

/// [`pbr_bits`] of a material: its own metallic-roughness settings when it has them (PBR
/// workflow, Window › Materials), otherwise guessed from its name and opacity.
pub fn pbr_of(m: &esq_core::model::Material) -> u32 {
    let p = &m.ext.pbr;
    // Preferences › Graphics › Automatically Enhance Materials: without it, materials that
    // say nothing keep the default roughness and no metalness.
    let (metal_auto, rough_auto) = crate::settings::with(|s| (s.enhance_metalness, s.enhance_roughness));
    let guessed = pbr_bits(&m.name, m.opacity);
    let guess =
        (if rough_auto { guessed & 255 } else { pbr_default() }) | if metal_auto { guessed & 0xff00 } else { 0 };
    if m.ext.workflow != 1 && !p.metalness_enabled && !p.roughness_enabled {
        return guess;
    }
    let rough =
        if p.roughness_enabled { p.roughness_factor.clamp(0.0, 1.0) as f32 } else { (guess & 255) as f32 / 255.0 };
    let metal = if p.metalness_enabled { p.metallic_factor.clamp(0.0, 1.0) as f32 } else { 0.0 };
    (rough * 255.0).round() as u32 | ((metal * 255.0).round() as u32) << 8
}

/// Roughness of faces without a material, or of one that says nothing.
pub const PBR_DEFAULT_ROUGHNESS: f32 = 0.6;

/// [`pbr_bits`] of a face without a material.
pub fn pbr_default() -> u32 {
    (PBR_DEFAULT_ROUGHNESS * 255.0f32).round() as u32
}

/// Turns display effects on from a comma-separated list (used by `--snapshot` through the
/// `ESQ_FX` variable): `depth`, `depth1`..`depth4`, `ao`, `photoreal`, `twopoint`, `shadows`
/// is not handled here.
pub fn apply_fx(list: &str, style: &mut ViewStyle, cam: &mut crate::camera::Camera) {
    for fx in list.split(',').map(str::trim) {
        match fx {
            "depth" => style.depth_cue = true,
            "depth1" | "depth2" | "depth3" | "depth4" => {
                style.depth_cue = true;
                style.depth_cue_width = fx[5..].parse().unwrap_or(4);
            }
            "ao" => style.ambient_occlusion = true,
            "photoreal" => style.photoreal = true,
            "twopoint" => cam.enable_two_point(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unpack(b: u32) -> (u32, u32) {
        (b & 255, b >> 8 & 255)
    }

    #[test]
    fn pbr_guess_from_material_names() {
        assert_eq!(unpack(pbr_bits("Acier brossé", 1.0)).1, 255);
        assert_eq!(unpack(pbr_bits("Brushed Aluminium", 1.0)).1, 255);
        assert_eq!(unpack(pbr_bits("Chêne", 1.0)), (153, 0));
        // Glass is smooth, never metallic.
        let (rough, metal) = unpack(pbr_bits("Bleu", 0.4));
        assert!(rough < 30 && metal == 0);
        assert!(unpack(pbr_bits("Béton", 1.0)).0 > unpack(pbr_bits("Plastique rouge", 1.0)).0);
        assert_eq!(pbr_default(), 153);
    }

    #[test]
    fn display_effects_are_off_by_default_and_toggle_from_the_fx_list() {
        let mut s = ViewStyle::default();
        let mut cam = crate::camera::Camera::default();
        assert!(!s.depth_cue && !s.ambient_occlusion && !s.photoreal);
        assert_eq!(s.depth_cue_width, 4);
        apply_fx("depth2, ao,photoreal,twopoint", &mut s, &mut cam);
        assert!(s.depth_cue && s.ambient_occlusion && s.photoreal);
        assert_eq!(s.depth_cue_width, 2);
        assert!(cam.is_two_point());
    }

    #[test]
    fn every_rendering_option_round_trips_through_a_snapshot() {
        let s = ViewStyle {
            edge_color_mode: EdgeColor::ByAxis,
            bg_color: [1, 2, 3],
            fog_start: 254.0,
            fog_end: 2540.0,
            faces: FaceStyle::Monochrome,
            texture: false,
            ..Default::default()
        };
        let mut snap = s.snapshot();
        snap.insert("BandColor".into(), json!([0, 0, 0, 127]));
        for k in RO_KEYS {
            assert!(snap.contains_key(*k), "{k}");
        }
        assert_eq!(snap["FogStartDist"], json!(10.0));
        assert_eq!(snap["RenderMode"], json!(4));
        let mut back = ViewStyle::default();
        let rest = back.restore(&snap);
        assert_eq!(back, s);
        assert_eq!(rest.keys().collect::<Vec<_>>(), ["BandColor"]);
        // A style leaves the model's fog alone.
        assert!(!s.style_snapshot().contains_key("DisplayFog"));
        // Shaded with textures is RenderMode 2 with the Texture flag, as SketchUp has it.
        let mut t = ViewStyle::default();
        assert_eq!((t.ro_get("RenderMode"), t.ro_get("Texture")), (Some(json!(2)), Some(json!(true))));
        t.ro_set("Texture", &json!(false));
        assert_eq!(t.faces, FaceStyle::Shaded);
        t.ro_set("RenderMode", &json!(0));
        t.ro_set("RenderMode", &json!(2));
        assert_eq!(t.faces, FaceStyle::Shaded);
        assert_eq!(t.ro_set("RenderMode", &json!(9)), Some(false));
        assert_eq!(t.ro_set("NoSuchKey", &json!(1)), None);
    }
}
