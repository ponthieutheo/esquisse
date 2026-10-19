//! Reading and writing SketchUp .skp files, every version from SketchUp 3 to 2026: natively
//! (`native`, `native::write`). Trimble's official C API (`SketchUpAPI.dll`), loaded at run
//! time from an installed SketchUp, is only used with `ESQ_SKP_SDK=1` and by the tests, as the
//! reference the native reader and writer are compared with. Nothing from SketchUp is shipped
//! with Esquisse.
//!
//! The .skp format stores inches; Esquisse stores millimetres.

use std::collections::{HashMap, HashSet};
use std::ffi::{CString, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result, anyhow, bail};
use esq_core::glam::{DMat4, DVec3};
use esq_core::model::{
    AnnotKind, Annotation, AttrValue, Attributes, DefId, Definition, EdgeId, Entities, Guide, Instance, Layer,
    LayerIdx, MaterialId, Model, SectionPlane, Units, VertexId,
};
use libloading::Library;

const IN: f64 = 25.4;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct Ref(*mut c_void);

impl Ref {
    const NULL: Ref = Ref(std::ptr::null_mut());
    fn is_null(self) -> bool {
        self.0.is_null()
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct P3 {
    x: f64,
    y: f64,
    z: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Plane3 {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct P2 {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Xf {
    v: [f64; 16],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Color {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

#[repr(C)]
struct MaterialInput {
    num_uv_coords: usize,
    uv_coords: [P2; 4],
    vertex_indices: [usize; 4],
    material: Ref,
}

/// `SUMaterialPositionInput` (API 9.1): up to four model points and their texture
/// coordinates, and an optional projection direction.
#[repr(C)]
struct MaterialPositionInput {
    num_uv_coords: usize,
    uv_coords: [P2; 4],
    points: [P3; 4],
    material: Ref,
    projection: P3,
}

/// `SUUVQ`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct Uvq {
    u: f64,
    v: f64,
    q: f64,
}

/// `SUComponentBehavior`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Behavior {
    snap: i32,
    cuts_opening: bool,
    always_face_camera: bool,
    shadows_face_sun: bool,
    no_scale_mask: usize,
}

type R = i32;

macro_rules! api {
    ($($name:ident : fn($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        // A binding table, as a header would be: some entries only serve the tests.
        #[allow(non_snake_case, dead_code)]
        struct Api {
            $($name: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)*
            _lib: Library,
        }
        unsafe impl Send for Api {}
        unsafe impl Sync for Api {}
        impl Api {
            unsafe fn load(lib: Library) -> Result<Api> {
                unsafe {
                    Ok(Api {
                        $($name: *lib
                            .get::<unsafe extern "C" fn($($arg),*) $(-> $ret)?>(concat!(stringify!($name), "\0").as_bytes())
                            .with_context(|| concat!("fonction absente : ", stringify!($name)))?,)*
                        _lib: lib,
                    })
                }
            }
        }
    };
}

api! {
    SUInitialize: fn();
    SUSectionPlaneCreate: fn(*mut Ref) -> R;
    SUSectionPlaneSetPlane: fn(Ref, *const Plane3) -> R;
    SUSectionPlaneGetPlane: fn(Ref, *mut Plane3) -> R;
    SUSectionPlaneIsActive: fn(Ref, *mut bool) -> R;
    SUSectionPlaneGetName: fn(Ref, *mut Ref) -> R;
    SUSectionPlaneSetName: fn(Ref, *const c_char) -> R;
    SUSectionPlaneGetSymbol: fn(Ref, *mut Ref) -> R;
    SUSectionPlaneSetSymbol: fn(Ref, *const c_char) -> R;
    SUSectionPlaneToDrawingElement: fn(Ref) -> Ref;
    SUEntitiesGetNumSectionPlanes: fn(Ref, *mut usize) -> R;
    SUEntitiesGetSectionPlanes: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUEntitiesAddSectionPlanes: fn(Ref, usize, *const Ref) -> R;
    SUEntitiesSetActiveSectionPlane: fn(Ref, Ref) -> R;
    SUGetAPIVersion: fn(*mut usize, *mut usize);
    SUModelCreate: fn(*mut Ref) -> R;
    SUModelCreateFromFileWithStatus: fn(*mut Ref, *const c_char, *mut i32) -> R;
    SUModelGetGuid: fn(Ref, *mut Ref) -> R;
    SUModelRelease: fn(*mut Ref) -> R;
    SUModelGetEntities: fn(Ref, *mut Ref) -> R;
    SUModelSaveToFile: fn(Ref, *const c_char) -> R;
    SUModelSaveToFileWithVersion: fn(Ref, *const c_char, i32) -> R;
    SUModelGetUnits: fn(Ref, *mut i32) -> R;
    SUModelGetNumMaterials: fn(Ref, *mut usize) -> R;
    SUModelGetMaterials: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUModelGetNumLayers: fn(Ref, *mut usize) -> R;
    SUModelGetLayers: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUModelGetDefaultLayer: fn(Ref, *mut Ref) -> R;
    SUModelAddMaterials: fn(Ref, usize, *const Ref) -> R;
    SUModelAddLayers: fn(Ref, usize, *const Ref) -> R;
    SUModelAddComponentDefinitions: fn(Ref, usize, *const Ref) -> R;
    SUModelGetOptionsManager: fn(Ref, *mut Ref) -> R;
    SUModelGetCamera: fn(Ref, *mut Ref) -> R;
    SUCameraGetOrientation: fn(Ref, *mut P3, *mut P3, *mut P3) -> R;
    SUCameraSetOrientation: fn(Ref, *const P3, *const P3, *const P3) -> R;
    SUCameraGetPerspective: fn(Ref, *mut bool) -> R;
    SUCameraSetPerspective: fn(Ref, bool) -> R;
    SUCameraGetPerspectiveFrustumFOV: fn(Ref, *mut f64) -> R;
    SUCameraSetPerspectiveFrustumFOV: fn(Ref, f64) -> R;
    SUOptionsManagerGetOptionsProviderByName: fn(Ref, *const c_char, *mut Ref) -> R;
    SUOptionsProviderSetValue: fn(Ref, *const c_char, Ref) -> R;
    SUTypedValueCreate: fn(*mut Ref) -> R;
    SUTypedValueRelease: fn(*mut Ref) -> R;
    SUTypedValueGetBool: fn(Ref, *mut bool) -> R;
    SUTypedValueSetBool: fn(Ref, bool) -> R;
    SUTypedValueGetDouble: fn(Ref, *mut f64) -> R;
    SUTypedValueSetDouble: fn(Ref, f64) -> R;
    SUTypedValueGetInt32: fn(Ref, *mut i32) -> R;
    SUTypedValueGetTime: fn(Ref, *mut i64) -> R;
    SUTypedValueSetTime: fn(Ref, i64) -> R;
    SUTypedValueGetString: fn(Ref, *mut Ref) -> R;
    SUTypedValueSetString: fn(Ref, *const c_char) -> R;
    SUTypedValueSetInt32: fn(Ref, i32) -> R;
    SUTypedValueGetType: fn(Ref, *mut i32) -> R;
    SUTypedValueGetFloat: fn(Ref, *mut f32) -> R;
    SUTypedValueGetInt16: fn(Ref, *mut i16) -> R;
    SUTypedValueGetByte: fn(Ref, *mut u8) -> R;
    SUEntityGetNumAttributeDictionaries: fn(Ref, *mut usize) -> R;
    SUEntityGetAttributeDictionaries: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUEntityGetAttributeDictionary: fn(Ref, *const c_char, *mut Ref) -> R;
    SUEntityAddAttributeDictionary: fn(Ref, Ref) -> R;
    SUAttributeDictionaryCreate: fn(*mut Ref, *const c_char) -> R;
    SUAttributeDictionaryGetName: fn(Ref, *mut Ref) -> R;
    SUAttributeDictionaryGetNumKeys: fn(Ref, *mut usize) -> R;
    SUAttributeDictionaryGetKeys: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUAttributeDictionaryGetValue: fn(Ref, *const c_char, *mut Ref) -> R;
    SUAttributeDictionarySetValue: fn(Ref, *const c_char, Ref) -> R;
    SUComponentDefinitionToEntity: fn(Ref) -> Ref;
    SUComponentInstanceToEntity: fn(Ref) -> Ref;
    SUGroupToEntity: fn(Ref) -> Ref;
    SUModelGetShadowInfo: fn(Ref, *mut Ref) -> R;
    SUShadowInfoGetValue: fn(Ref, *const c_char, *mut Ref) -> R;
    SUShadowInfoSetValue: fn(Ref, *const c_char, Ref) -> R;
    SUEntitiesGetNumFaces: fn(Ref, *mut usize) -> R;
    SUEntitiesGetFaces: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUEntitiesTransform: fn(Ref, usize, *const Ref, *const Xf) -> R;
    SUEntitiesGetNumEdges: fn(Ref, bool, *mut usize) -> R;
    SUEntitiesGetEdges: fn(Ref, bool, usize, *mut Ref, *mut usize) -> R;
    SUEntitiesGetNumGroups: fn(Ref, *mut usize) -> R;
    SUEntitiesGetGroups: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUEntitiesGetNumInstances: fn(Ref, *mut usize) -> R;
    SUEntitiesGetInstances: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUEntitiesFill: fn(Ref, Ref, bool) -> R;
    SUEntitiesAddGroup: fn(Ref, Ref) -> R;
    SUEntitiesAddInstance: fn(Ref, Ref, *mut Ref) -> R;
    SUFaceGetOuterLoop: fn(Ref, *mut Ref) -> R;
    SUFaceGetNumInnerLoops: fn(Ref, *mut usize) -> R;
    SUFaceGetInnerLoops: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUFaceGetNormal: fn(Ref, *mut P3) -> R;
    SUFaceGetFrontMaterial: fn(Ref, *mut Ref) -> R;
    SUFaceGetBackMaterial: fn(Ref, *mut Ref) -> R;
    SUFaceToDrawingElement: fn(Ref) -> Ref;
    SULoopGetNumVertices: fn(Ref, *mut usize) -> R;
    SULoopGetVertices: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUVertexGetPosition: fn(Ref, *mut P3) -> R;
    SUEdgeGetStartVertex: fn(Ref, *mut Ref) -> R;
    SUEdgeGetEndVertex: fn(Ref, *mut Ref) -> R;
    SUEdgeGetSoft: fn(Ref, *mut bool) -> R;
    SUEdgeGetSmooth: fn(Ref, *mut bool) -> R;
    SUEdgeToDrawingElement: fn(Ref) -> Ref;
    SUDrawingElementGetLayer: fn(Ref, *mut Ref) -> R;
    SUDrawingElementGetHidden: fn(Ref, *mut bool) -> R;
    SUDrawingElementGetMaterial: fn(Ref, *mut Ref) -> R;
    SUDrawingElementSetLayer: fn(Ref, Ref) -> R;
    SUDrawingElementSetMaterial: fn(Ref, Ref) -> R;
    SUDrawingElementSetHidden: fn(Ref, bool) -> R;
    SUGroupCreate: fn(*mut Ref) -> R;
    SUGroupGetEntities: fn(Ref, *mut Ref) -> R;
    SUGroupGetTransform: fn(Ref, *mut Xf) -> R;
    SUGroupGetName: fn(Ref, *mut Ref) -> R;
    SUGroupGetDefinition: fn(Ref, *mut Ref) -> R;
    SUGroupSetTransform: fn(Ref, *const Xf) -> R;
    SUGroupSetName: fn(Ref, *const c_char) -> R;
    SUGroupToDrawingElement: fn(Ref) -> Ref;
    SUComponentInstanceGetDefinition: fn(Ref, *mut Ref) -> R;
    SUComponentInstanceGetGuid: fn(Ref, *mut Ref) -> R;
    SUComponentDefinitionGetGuid: fn(Ref, *mut Ref) -> R;
    SUComponentInstanceGetTransform: fn(Ref, *mut Xf) -> R;
    SUComponentInstanceGetName: fn(Ref, *mut Ref) -> R;
    SUComponentInstanceSetTransform: fn(Ref, *const Xf) -> R;
    SUComponentInstanceSetName: fn(Ref, *const c_char) -> R;
    SUComponentInstanceToDrawingElement: fn(Ref) -> Ref;
    SUComponentDefinitionCreate: fn(*mut Ref) -> R;
    SUComponentDefinitionGetEntities: fn(Ref, *mut Ref) -> R;
    SUComponentDefinitionGetName: fn(Ref, *mut Ref) -> R;
    SUComponentDefinitionSetName: fn(Ref, *const c_char) -> R;
    SUComponentDefinitionCreateInstance: fn(Ref, *mut Ref) -> R;
    SUMaterialCreate: fn(*mut Ref) -> R;
    SUMaterialSetName: fn(Ref, *const c_char) -> R;
    SUMaterialSetColor: fn(Ref, *const Color) -> R;
    SUMaterialSetOpacity: fn(Ref, f64) -> R;
    SUMaterialGetName: fn(Ref, *mut Ref) -> R;
    SUMaterialGetColor: fn(Ref, *mut Color) -> R;
    SUMaterialGetOpacity: fn(Ref, *mut f64) -> R;
    SULayerCreate: fn(*mut Ref) -> R;
    SULayerSetName: fn(Ref, *const c_char) -> R;
    SULayerSetVisibility: fn(Ref, bool) -> R;
    SULayerGetName: fn(Ref, *mut Ref) -> R;
    SULayerGetVisibility: fn(Ref, *mut bool) -> R;
    SUStringCreate: fn(*mut Ref) -> R;
    SUStringGetUTF8Length: fn(Ref, *mut usize) -> R;
    SUStringGetUTF8: fn(Ref, usize, *mut c_char, *mut usize) -> R;
    SUStringRelease: fn(*mut Ref) -> R;
    SUGeometryInputCreate: fn(*mut Ref) -> R;
    SUGeometryInputRelease: fn(*mut Ref) -> R;
    SUGeometryInputSetVertices: fn(Ref, usize, *const P3) -> R;
    SUGeometryInputAddFace: fn(Ref, *mut Ref, *mut usize) -> R;
    SUGeometryInputFaceAddInnerLoop: fn(Ref, usize, *mut Ref) -> R;
    SUGeometryInputFaceSetFrontMaterial: fn(Ref, usize, *const MaterialInput) -> R;
    SUGeometryInputFaceSetBackMaterial: fn(Ref, usize, *const MaterialInput) -> R;
    SUGeometryInputFaceSetLayer: fn(Ref, usize, Ref) -> R;
    SUGeometryInputFaceSetHidden: fn(Ref, usize, bool) -> R;
    SUGeometryInputAddEdge: fn(Ref, usize, usize, *mut usize) -> R;
    SUGeometryInputEdgeSetSoft: fn(Ref, usize, bool) -> R;
    SUGeometryInputEdgeSetSmooth: fn(Ref, usize, bool) -> R;
    SUGeometryInputEdgeSetHidden: fn(Ref, usize, bool) -> R;
    SUGeometryInputEdgeSetLayer: fn(Ref, usize, Ref) -> R;
    SULoopInputCreate: fn(*mut Ref) -> R;
    SULoopInputAddVertexIndex: fn(Ref, usize) -> R;
    SULoopInputEdgeSetSoft: fn(Ref, usize, bool) -> R;
    SULoopInputEdgeSetSmooth: fn(Ref, usize, bool) -> R;
    SULoopInputEdgeSetHidden: fn(Ref, usize, bool) -> R;
    SUEntitiesGetNumGuideLines: fn(Ref, *mut usize) -> R;
    SUEntitiesGetGuideLines: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUEntitiesGetNumGuidePoints: fn(Ref, *mut usize) -> R;
    SUEntitiesGetGuidePoints: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUEntitiesAddGuideLines: fn(Ref, usize, *const Ref) -> R;
    SUEntitiesAddGuidePoints: fn(Ref, usize, *const Ref) -> R;
    SUGuideLineCreateFinite: fn(*mut Ref, *const P3, *const P3) -> R;
    SUGuideLineCreateInfinite: fn(*mut Ref, *const P3, *const P3) -> R;
    SUGuideLineGetData: fn(Ref, *mut P3, *mut P3, *mut bool) -> R;
    SUGuideLineToDrawingElement: fn(Ref) -> Ref;
    SUGuidePointCreate: fn(*mut Ref, *const P3) -> R;
    SUGuidePointGetPosition: fn(Ref, *mut P3) -> R;
    SUGuidePointToDrawingElement: fn(Ref) -> Ref;
    // Component snaps (API 13): read through a typed entity list; the API cannot add one
    // to an entities collection.
    SUSnapCreate: fn(*mut Ref, P3, *const P3, *const P3) -> R;
    SUSnapRelease: fn(*mut Ref) -> R;
    SUSnapToEntity: fn(Ref) -> Ref;
    SUSnapFromEntity: fn(Ref) -> Ref;
    SUSnapGetPosition: fn(Ref, *mut P3) -> R;
    SUSnapGetDirection: fn(Ref, *mut P3) -> R;
    SUSnapGetUp: fn(Ref, *mut P3) -> R;
    SUEntityGetType: fn(Ref) -> i32;
    SUEntitiesEntityListFill: fn(Ref, i32, Ref) -> R;
    SUEntityListCreate: fn(*mut Ref) -> R;
    SUEntityListRelease: fn(*mut Ref) -> R;
    SUEntityListBegin: fn(Ref, *mut Ref) -> R;
    SUEntityListIteratorCreate: fn(*mut Ref) -> R;
    SUEntityListIteratorRelease: fn(*mut Ref) -> R;
    SUEntityListIteratorGetEntity: fn(Ref, *mut Ref) -> R;
    SUEntityListIteratorNext: fn(Ref) -> R;
    SUEntityListIteratorIsInRange: fn(Ref, *mut bool) -> R;
    SUGroupToComponentInstance: fn(Ref) -> Ref;
    SUComponentInstanceIsLocked: fn(Ref, *mut bool) -> R;
    SUComponentInstanceSetLocked: fn(Ref, bool) -> R;
    SUModelGetNumScenes: fn(Ref, *mut usize) -> R;
    SUModelGetActiveScene: fn(Ref, *mut Ref) -> R;
    SUModelSetActiveScene: fn(Ref, Ref) -> R;
    SUModelGetScenes: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUModelAddScenes: fn(Ref, usize, *const Ref) -> R;
    SUSceneCreate: fn(*mut Ref) -> R;
    SUSceneGetName: fn(Ref, *mut Ref) -> R;
    SUSceneSetName: fn(Ref, *const c_char) -> R;
    SUSceneGetDescription: fn(Ref, *mut Ref) -> R;
    SUSceneSetDescription: fn(Ref, *const c_char) -> R;
    SUSceneGetCamera: fn(Ref, *mut Ref) -> R;
    SUSceneSetCamera: fn(Ref, Ref) -> R;
    SUSceneSetUseCamera: fn(Ref, bool) -> R;
    SUSceneGetUseHiddenLayers: fn(Ref, *mut bool) -> R;
    SUSceneSetUseHiddenLayers: fn(Ref, bool) -> R;
    SUSceneGetNumLayers: fn(Ref, *mut usize) -> R;
    SUSceneGetLayers: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUSceneAddLayer: fn(Ref, Ref) -> R;
    SUCameraCreate: fn(*mut Ref) -> R;
    SUCameraRelease: fn(*mut Ref) -> R;
    SUInstancePathRelease: fn(*mut Ref) -> R;
    SUModelGetNumFonts: fn(Ref, *mut usize) -> R;
    SUModelGetFonts: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUTextSetFont: fn(Ref, Ref) -> R;
    SUEntitiesGetNumTexts: fn(Ref, *mut usize) -> R;
    SUEntitiesGetTexts: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUEntitiesAddTexts: fn(Ref, usize, *const Ref) -> R;
    SUTextCreate: fn(*mut Ref) -> R;
    SUTextSetString: fn(Ref, *const c_char) -> R;
    SUTextGetString: fn(Ref, *mut Ref) -> R;
    SUTextSetPoint: fn(Ref, *const P3, Ref) -> R;
    SUTextGetPoint: fn(Ref, *mut P3, *mut Ref) -> R;
    SUTextSetLeaderVector: fn(Ref, *const P3) -> R;
    SUTextGetLeaderVector: fn(Ref, *mut P3) -> R;
    SUTextSetLeaderType: fn(Ref, i32) -> R;
    SUTextGetLeaderType: fn(Ref, *mut i32) -> R;
    SUTextSetScreenPosition: fn(Ref, f64, f64) -> R;
    SUTextGetScreenPosition: fn(Ref, *mut f64, *mut f64) -> R;
    SUTextToDrawingElement: fn(Ref) -> Ref;
    SUEntitiesGetNumDimensions: fn(Ref, *mut usize) -> R;
    SUEntitiesGetDimensions: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUEntitiesAddDimensions: fn(Ref, usize, *const Ref) -> R;
    SUDimensionGetType: fn(Ref, *mut i32) -> R;
    SUDimensionGetText: fn(Ref, *mut Ref) -> R;
    SUDimensionSetText: fn(Ref, *const c_char) -> R;
    SUDimensionToDrawingElement: fn(Ref) -> Ref;
    SUDimensionLinearFromDimension: fn(Ref) -> Ref;
    SUDimensionLinearToDimension: fn(Ref) -> Ref;
    SUDimensionLinearCreate: fn(*mut Ref, *const P3, Ref, *const P3, Ref, f64) -> R;
    SUDimensionLinearGetStartPoint: fn(Ref, *mut P3, *mut Ref) -> R;
    SUDimensionLinearGetEndPoint: fn(Ref, *mut P3, *mut Ref) -> R;
    SUDimensionLinearGetXAxis: fn(Ref, *mut P3) -> R;
    SUDimensionLinearSetXAxis: fn(Ref, *const P3) -> R;
    SUDimensionLinearGetNormal: fn(Ref, *mut P3) -> R;
    SUDimensionLinearSetNormal: fn(Ref, *const P3) -> R;
    SUDimensionLinearGetPosition: fn(Ref, *mut P2) -> R;
    SUDimensionLinearSetPosition: fn(Ref, *const P2) -> R;
    // Materials: textures, colorization, PBR.
    SUModelLoadMaterial: fn(Ref, *const c_char, *mut Ref) -> R;
    SUMaterialToEntity: fn(Ref) -> Ref;
    SUMaterialGetTexture: fn(Ref, *mut Ref) -> R;
    SUMaterialSetTexture: fn(Ref, Ref) -> R;
    SUMaterialGetType: fn(Ref, *mut i32) -> R;
    SUMaterialSetType: fn(Ref, i32) -> R;
    SUMaterialGetUseOpacity: fn(Ref, *mut bool) -> R;
    SUMaterialSetUseOpacity: fn(Ref, bool) -> R;
    SUMaterialGetColorizeType: fn(Ref, *mut i32) -> R;
    SUMaterialSetColorizeType: fn(Ref, i32) -> R;
    SUMaterialGetColorizeDeltas: fn(Ref, *mut f64, *mut f64, *mut f64) -> R;
    SUMaterialGetWorkflow: fn(Ref, *mut i32) -> R;
    SUMaterialSetMetalnessEnabled: fn(Ref, bool) -> R;
    SUMaterialIsMetalnessEnabled: fn(Ref, *mut bool) -> R;
    SUMaterialSetMetallicTexture: fn(Ref, Ref) -> R;
    SUMaterialGetMetallicTexture: fn(Ref, *mut Ref) -> R;
    SUMaterialSetMetallicFactor: fn(Ref, f64) -> R;
    SUMaterialGetMetallicFactor: fn(Ref, *mut f64) -> R;
    SUMaterialSetRoughnessEnabled: fn(Ref, bool) -> R;
    SUMaterialIsRoughnessEnabled: fn(Ref, *mut bool) -> R;
    SUMaterialSetRoughnessTexture: fn(Ref, Ref) -> R;
    SUMaterialGetRoughnessTexture: fn(Ref, *mut Ref) -> R;
    SUMaterialSetRoughnessFactor: fn(Ref, f64) -> R;
    SUMaterialGetRoughnessFactor: fn(Ref, *mut f64) -> R;
    SUMaterialSetRoughnessTextureInverted: fn(Ref, bool) -> R;
    SUMaterialIsRoughnessTextureInverted: fn(Ref, *mut bool) -> R;
    SUMaterialSetNormalEnabled: fn(Ref, bool) -> R;
    SUMaterialIsNormalEnabled: fn(Ref, *mut bool) -> R;
    SUMaterialSetNormalTexture: fn(Ref, Ref) -> R;
    SUMaterialGetNormalTexture: fn(Ref, *mut Ref) -> R;
    SUMaterialSetNormalScale: fn(Ref, f64) -> R;
    SUMaterialGetNormalScale: fn(Ref, *mut f64) -> R;
    SUMaterialSetNormalStyle: fn(Ref, i32) -> R;
    SUMaterialGetNormalStyle: fn(Ref, *mut i32) -> R;
    SUMaterialSetAOEnabled: fn(Ref, bool) -> R;
    SUMaterialIsAOEnabled: fn(Ref, *mut bool) -> R;
    SUMaterialSetAOTexture: fn(Ref, Ref) -> R;
    SUMaterialGetAOTexture: fn(Ref, *mut Ref) -> R;
    SUMaterialSetAOStrength: fn(Ref, f64) -> R;
    SUMaterialGetAOStrength: fn(Ref, *mut f64) -> R;
    SUTextureCreateFromFile: fn(*mut Ref, *const c_char, f64, f64) -> R;
    SUTextureGetDimensions: fn(Ref, *mut usize, *mut usize, *mut f64, *mut f64) -> R;
    SUTextureGetFileName: fn(Ref, *mut Ref) -> R;
    SUTextureSetFileName: fn(Ref, *const c_char) -> R;
    SUTextureWriteOriginalToFile: fn(Ref, *const c_char) -> R;
    SUTextureWriteToFile: fn(Ref, *const c_char) -> R;
    SUTextureGetAverageColor: fn(Ref, *mut Color) -> R;
    // Texture positioning on faces.
    SUFaceToEntity: fn(Ref) -> Ref;
    SUFaceGetUVHelper: fn(Ref, bool, bool, Ref, *mut Ref) -> R;
    SUUVHelperGetFrontUVQ: fn(Ref, *const P3, *mut Uvq) -> R;
    SUUVHelperGetBackUVQ: fn(Ref, *const P3, *mut Uvq) -> R;
    SUUVHelperRelease: fn(*mut Ref) -> R;
    SUFaceIsTexturePositioned: fn(Ref, bool, *mut bool) -> R;
    SUFaceIsTextureProjected: fn(Ref, bool, *mut bool) -> R;
    SUFaceGetTextureProjection: fn(Ref, bool, *mut P3) -> R;
    SUGeometryInputFaceSetFrontMaterialByPosition: fn(Ref, usize, *const MaterialPositionInput) -> R;
    SUFacePositionMaterial: fn(Ref, bool, *const MaterialPositionInput) -> R;
    SUFaceReverse: fn(Ref) -> R;
    SUGeometryInputFaceSetBackMaterialByPosition: fn(Ref, usize, *const MaterialPositionInput) -> R;
    // Shadows of drawing elements.
    SUDrawingElementGetCastsShadows: fn(Ref, *mut bool) -> R;
    SUDrawingElementSetCastsShadows: fn(Ref, bool) -> R;
    SUDrawingElementGetReceivesShadows: fn(Ref, *mut bool) -> R;
    SUDrawingElementSetReceivesShadows: fn(Ref, bool) -> R;
    SUGeometryInputFaceSetCastsShadow: fn(Ref, usize, bool) -> R;
    SUGeometryInputFaceSetReceivesShadow: fn(Ref, usize, bool) -> R;
    SUGeometryInputEdgeSetCastsShadow: fn(Ref, usize, bool) -> R;
    SUGeometryInputEdgeSetReceivesShadow: fn(Ref, usize, bool) -> R;
    SULoopInputEdgeSetCastsShadow: fn(Ref, usize, bool) -> R;
    SULoopInputEdgeSetReceivesShadow: fn(Ref, usize, bool) -> R;
    // Edges: materials, curves and arcs.
    SUEdgeToEntity: fn(Ref) -> Ref;
    SUEdgeGetCurve: fn(Ref, *mut Ref) -> R;
    SUEdgeSetSoft: fn(Ref, bool) -> R;
    SUEdgeSetSmooth: fn(Ref, bool) -> R;
    SUGeometryInputEdgeSetMaterial: fn(Ref, usize, Ref) -> R;
    SULoopInputEdgeSetMaterial: fn(Ref, usize, Ref) -> R;
    SULoopInputEdgeSetLayer: fn(Ref, usize, Ref) -> R;
    SUGeometryInputAddCurve: fn(Ref, usize, *const usize, *mut usize) -> R;
    SUGeometryInputAddArcCurve: fn(Ref, usize, usize, *const P3, *const P3, usize, *mut usize, *mut usize) -> R;
    SUCurveGetType: fn(Ref, *mut i32) -> R;
    SUCurveIsPolygon: fn(Ref, *mut bool) -> R;
    SUArcCurveFromCurve: fn(Ref) -> Ref;
    SUArcCurveGetCenter: fn(Ref, *mut P3) -> R;
    SUArcCurveGetNormal: fn(Ref, *mut P3) -> R;
    SUArcCurveGetXAxis: fn(Ref, *mut P3) -> R;
    SUArcCurveGetStartAngle: fn(Ref, *mut f64) -> R;
    SUArcCurveGetEndAngle: fn(Ref, *mut f64) -> R;
    // Tags: colour, line style, folders, scene behaviour.
    SULayerToEntity: fn(Ref) -> Ref;
    SULayerGetMaterial: fn(Ref, *mut Ref) -> R;
    SULayerGetLineStyle: fn(Ref, *mut Ref) -> R;
    SULayerSetLineStyle: fn(Ref, Ref) -> R;
    SULayerGetSceneBehavior: fn(Ref, *mut i32, *mut i32) -> R;
    SULayerSetSceneBehavior: fn(Ref, i32, i32) -> R;
    SULayerGetParentLayerFolder: fn(Ref, *mut Ref) -> R;
    SULineStyleGetName: fn(Ref, *mut Ref) -> R;
    SUModelGetLineStyles: fn(Ref, *mut Ref) -> R;
    SULineStylesGetLineStyleByName: fn(Ref, *const c_char, *mut Ref) -> R;
    SUModelGetNumLayerFolders: fn(Ref, *mut usize) -> R;
    SUModelGetLayerFolders: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUModelAddLayerFolder: fn(Ref, Ref) -> R;
    SULayerFolderCreate: fn(*mut Ref, *const c_char) -> R;
    SULayerFolderGetName: fn(Ref, *mut Ref) -> R;
    SULayerFolderGetVisibility: fn(Ref, *mut bool) -> R;
    SULayerFolderSetVisibility: fn(Ref, bool) -> R;
    SULayerGroupGetVisibleOnNewScenes: fn(Ref, *mut bool) -> R;
    SULayerGroupSetVisibleOnNewScenes: fn(Ref, bool) -> R;
    SULayerFolderAddLayer: fn(Ref, Ref) -> R;
    SULayerFolderGetNumLayers: fn(Ref, *mut usize) -> R;
    SULayerFolderGetLayers: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SULayerFolderAddLayerFolder: fn(Ref, Ref) -> R;
    SULayerFolderGetNumLayerFolders: fn(Ref, *mut usize) -> R;
    SULayerFolderGetLayerFolders: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUModelGetActiveLayer: fn(Ref, *mut Ref) -> R;
    SUModelSetActiveLayer: fn(Ref, Ref) -> R;
    // The model: name, axes, options, rendering options, attributes.
    SUModelGetName: fn(Ref, *mut Ref) -> R;
    SUModelSetName: fn(Ref, *const c_char) -> R;
    SUModelGetDescription: fn(Ref, *mut Ref) -> R;
    SUModelSetDescription: fn(Ref, *const c_char) -> R;
    SUModelGetAxes: fn(Ref, *mut Ref) -> R;
    SUAxesGetTransform: fn(Ref, *mut Xf) -> R;
    SUAxesSetOrigin: fn(Ref, *const P3) -> R;
    SUAxesSetAxesVecs: fn(Ref, *const P3, *const P3, *const P3) -> R;
    SUModelGetRenderingOptions: fn(Ref, *mut Ref) -> R;
    SURenderingOptionsGetNumKeys: fn(Ref, *mut usize) -> R;
    SURenderingOptionsGetKeys: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SURenderingOptionsGetValue: fn(Ref, *const c_char, *mut Ref) -> R;
    SURenderingOptionsSetValue: fn(Ref, *const c_char, Ref) -> R;
    SUOptionsManagerGetNumOptionsProviders: fn(Ref, *mut usize) -> R;
    SUOptionsManagerGetOptionsProviderNames: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUOptionsProviderGetNumKeys: fn(Ref, *mut usize) -> R;
    SUOptionsProviderGetKeys: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUOptionsProviderGetValue: fn(Ref, *const c_char, *mut Ref) -> R;
    SUModelGetNumAttributeDictionaries: fn(Ref, *mut usize) -> R;
    SUModelGetAttributeDictionaries: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUModelGetAttributeDictionary: fn(Ref, *const c_char, *mut Ref) -> R;
    SUTypedValueGetColor: fn(Ref, *mut Color) -> R;
    SUTypedValueSetColor: fn(Ref, *const Color) -> R;
    SUTypedValueGetVector3d: fn(Ref, *mut [f64; 3]) -> R;
    SUTypedValueSetVector3d: fn(Ref, *const [f64; 3]) -> R;
    SUTypedValueGetNumArrayItems: fn(Ref, *mut usize) -> R;
    SUTypedValueGetArrayItems: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUTypedValueSetArrayItems: fn(Ref, usize, *mut Ref) -> R;
    // Component definitions, images.
    SUComponentDefinitionGetBehavior: fn(Ref, *mut Behavior) -> R;
    SUComponentDefinitionSetBehavior: fn(Ref, *const Behavior) -> R;
    SUComponentDefinitionGetDescription: fn(Ref, *mut Ref) -> R;
    SUComponentDefinitionSetDescription: fn(Ref, *const c_char) -> R;
    SUEntitiesGetNumImages: fn(Ref, *mut usize) -> R;
    SUEntitiesGetImages: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUEntitiesAddImage: fn(Ref, Ref) -> R;
    SUImageCreateFromFile: fn(*mut Ref, *const c_char) -> R;
    SUImageGetImageRep: fn(Ref, *mut Ref) -> R;
    SUImageGetName: fn(Ref, *mut Ref) -> R;
    SUImageSetName: fn(Ref, *const c_char) -> R;
    SUImageGetTransform: fn(Ref, *mut Xf) -> R;
    SUImageSetTransform: fn(Ref, *const Xf) -> R;
    SUImageGetFileName: fn(Ref, *mut Ref) -> R;
    SUImageGetPixelDimensions: fn(Ref, *mut usize, *mut usize) -> R;
    SUImageGetDefinition: fn(Ref, *mut Ref) -> R;
    SUImageToEntity: fn(Ref) -> Ref;
    SUImageToDrawingElement: fn(Ref) -> Ref;
    SUImageRepCreate: fn(*mut Ref) -> R;
    SUImageRepRelease: fn(*mut Ref) -> R;
    SUImageRepSaveToFile: fn(Ref, *const c_char) -> R;
    SUImageRepLoadFile: fn(Ref, *const c_char) -> R;
    SUImageRepGetPixelDimensions: fn(Ref, *mut usize, *mut usize) -> R;
    SUImageRepGetRowPadding: fn(Ref, *mut usize) -> R;
    SUImageRepGetDataSize: fn(Ref, *mut usize, *mut usize) -> R;
    SUImageRepGetData: fn(Ref, usize, *mut u8) -> R;
    // Scenes.
    SUSceneToEntity: fn(Ref) -> Ref;
    SUSceneGetFlags: fn(Ref, *mut u32) -> R;
    SUSceneSetFlags: fn(Ref, u32) -> R;
    SUSceneGetUseRenderingOptions: fn(Ref, *mut bool) -> R;
    SUSceneSetUseRenderingOptions: fn(Ref, bool) -> R;
    SUSceneGetRenderingOptions: fn(Ref, *mut Ref) -> R;
    SUSceneUpdate: fn(Ref, u32) -> R;
    SUSceneGetIncludeInAnimation: fn(Ref, *mut bool) -> R;
    SUSceneSetIncludeInAnimation: fn(Ref, bool) -> R;
    // Texts and dimensions.
    SUTextGetArrowType: fn(Ref, *mut i32) -> R;
    SUTextSetArrowType: fn(Ref, i32) -> R;
    SUDimensionGetArrowType: fn(Ref, *mut i32) -> R;
    SUDimensionSetArrowType: fn(Ref, i32) -> R;
    // What `sdk_census` reads (the classes kept by skp-complet).
    SUModelGetStyles: fn(Ref, *mut Ref) -> R;
    SUStylesGetNumStyles: fn(Ref, *mut usize) -> R;
    SUStylesGetStyles: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUStylesGetActiveStyle: fn(Ref, *mut Ref) -> R;
    SUStylesGetSelectedStyle: fn(Ref, *mut Ref) -> R;
    SUStylesGetActiveStyleChanged: fn(Ref, *mut u64) -> R;
    SUStyleGetName: fn(Ref, *mut Ref) -> R;
    SUStyleGetDescription: fn(Ref, *mut Ref) -> R;
    SUStyleGetGuid: fn(Ref, *mut Ref) -> R;
    SUStyleGetPath: fn(Ref, *mut Ref) -> R;
    SUStyleGetDisplaysWatermark: fn(Ref, *mut u64) -> R;
    SUStyleGetProperty: fn(Ref, i32, *mut Ref) -> R;
    SUSceneGetStyle: fn(Ref, *mut Ref) -> R;
    SUSceneGetUseEnvironment: fn(Ref, *mut u64) -> R;
    SUSceneGetEnvironment: fn(Ref, *mut Ref) -> R;
    SUSceneGetPhotoMatchImageDisplayed: fn(Ref, *mut u64) -> R;
    SUModelGetClassifications: fn(Ref, *mut Ref) -> R;
    SUClassificationsGetNumSchemas: fn(Ref, *mut usize) -> R;
    SUClassificationsGetSchemas: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUSchemaGetSchemaName: fn(Ref, *mut Ref) -> R;
    SUComponentInstanceCreateClassificationInfo: fn(Ref, *mut Ref) -> R;
    SUClassificationInfoGetNumSchemas: fn(Ref, *mut usize) -> R;
    SUClassificationInfoGetSchemaName: fn(Ref, usize, *mut Ref) -> R;
    SUClassificationInfoGetSchemaType: fn(Ref, usize, *mut Ref) -> R;
    SUClassificationInfoRelease: fn(*mut Ref) -> R;
    SUDimensionRadialFromDimension: fn(Ref) -> Ref;
    SUDimensionRadialGetDiameter: fn(Ref, *mut u64) -> R;
    SUDimensionRadialGetLeaderBreakPoint: fn(Ref, *mut P3) -> R;
    SUDimensionRadialGetLeaderPoints: fn(Ref, *mut P3) -> R;
    SUDimensionGetFont: fn(Ref, *mut Ref) -> R;
    SUEntitiesGetNumPolyline3ds: fn(Ref, *mut usize) -> R;
    SUEntitiesGetPolyline3ds: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUPolyline3dGetNumPoints: fn(Ref, *mut usize) -> R;
    SUPolyline3dGetPoints: fn(Ref, usize, *mut P3, *mut usize) -> R;
    SUFontGetFaceName: fn(Ref, *mut Ref) -> R;
    SUFontGetPointSize: fn(Ref, *mut u64) -> R;
    SUFontGetBold: fn(Ref, *mut u64) -> R;
    SUFontGetItalic: fn(Ref, *mut u64) -> R;
    SUFontGetUseWorldSize: fn(Ref, *mut u64) -> R;
    SUFontGetWorldSize: fn(Ref, *mut u64) -> R;
    SUModelGetDimensionStyle: fn(Ref, *mut Ref) -> R;
    SUDimensionStyleGetFont: fn(Ref, *mut Ref) -> R;
    SUDimensionStyleGet3D: fn(Ref, *mut u64) -> R;
    SUDimensionStyleGetArrowType: fn(Ref, *mut u64) -> R;
    SUDimensionStyleGetArrowSize: fn(Ref, *mut u64) -> R;
    SUDimensionStyleGetColor: fn(Ref, *mut u64) -> R;
    SUDimensionStyleGetTextColor: fn(Ref, *mut u64) -> R;
    SUDimensionStyleGetLineWeight: fn(Ref, *mut u64) -> R;
    SUDimensionStyleGetExtensionLineOffset: fn(Ref, *mut u64) -> R;
    SUDimensionStyleGetExtensionLineOvershoot: fn(Ref, *mut u64) -> R;
    SUDimensionStyleGetShowRadialPrefix: fn(Ref, *mut u64) -> R;
    SUDimensionStyleGetHideOutOfPlane: fn(Ref, *mut u64) -> R;
    SUDimensionStyleGetHideOutOfPlaneValue: fn(Ref, *mut u64) -> R;
    SUDimensionStyleGetHideSmall: fn(Ref, *mut u64) -> R;
    SUDimensionStyleGetHideSmallValue: fn(Ref, *mut u64) -> R;
    SUDimensionStyleGetHighlightNonAssociativeDimensions: fn(Ref, *mut u64) -> R;
    SUDimensionStyleGetHighlightNonAssociativeDimensionsColor: fn(Ref, *mut u64) -> R;
    SUTextGetFont: fn(Ref, *mut Ref) -> R;
    SUTextGetColor: fn(Ref, *mut u64) -> R;
    SUModelGetEnvironments: fn(Ref, *mut Ref) -> R;
    SUEnvironmentsGetCount: fn(Ref, *mut usize) -> R;
    SUEnvironmentsGetAll: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUEnvironmentsGetSelectedEnvironment: fn(Ref, *mut Ref) -> R;
    SUEnvironmentGetName: fn(Ref, *mut Ref) -> R;
    SUEnvironmentGetDescription: fn(Ref, *mut Ref) -> R;
    SUEnvironmentGetRotation: fn(Ref, *mut u64) -> R;
    SUEnvironmentGetSkydomeExposure: fn(Ref, *mut u64) -> R;
    SUEnvironmentGetReflectionExposure: fn(Ref, *mut u64) -> R;
    SUEnvironmentGetUseAsSkydome: fn(Ref, *mut u64) -> R;
    SUEnvironmentGetUseForReflections: fn(Ref, *mut u64) -> R;
    SUEnvironmentGetLinkedSun: fn(Ref, *mut u64) -> R;
    SUEnvironmentCreateFromFile: fn(*mut Ref, *const c_char, *const c_char) -> R;
    SUEnvironmentsAdd: fn(Ref, Ref) -> R;
    SUEnvironmentsSetSelectedEnvironment: fn(Ref, Ref) -> R;
    SUEnvironmentSetDescription: fn(Ref, *const c_char) -> R;
    SUEnvironmentSetRotation: fn(Ref, f64) -> R;
    SUEnvironmentSetSkydomeExposure: fn(Ref, f64) -> R;
    SUEnvironmentSetReflectionExposure: fn(Ref, f64) -> R;
    SUEnvironmentSetUseAsSkydome: fn(Ref, bool) -> R;
    SUEnvironmentSetUseForReflections: fn(Ref, bool) -> R;
    SUEnvironmentSetLinkedSun: fn(Ref, bool) -> R;
    SUSceneSetEnvironment: fn(Ref, Ref) -> R;
    SUSceneSetUseEnvironment: fn(Ref, bool) -> R;
    // What the corpus of the SDK adds to its files (`sdk_corpus_extras`).
    SUStylesAddStyle: fn(Ref, *const c_char, bool) -> R;


    SUClassificationsLoadSchema: fn(Ref, *const c_char) -> R;
    SUEntitiesGetNumArcCurves: fn(Ref, *mut usize) -> R;
    SUEntitiesGetArcCurves: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUArcCurveToCurve: fn(Ref) -> Ref;
    SUCurveToEntity: fn(Ref) -> Ref;
    SUInstancePathCreate: fn(*mut Ref) -> R;
    SUInstancePathSetLeaf: fn(Ref, Ref) -> R;
    // Attached texts and dimensions (`sdk_attach.rs`, tests only).
    SUInstancePathGetLeafAsEntity: fn(Ref, *mut Ref) -> R;
    SUInstancePathGetPathDepth: fn(Ref, *mut usize) -> R;
    SUModelGetNumComponentDefinitions: fn(Ref, *mut usize) -> R;
    SUModelGetComponentDefinitions: fn(Ref, usize, *mut Ref, *mut usize) -> R;
    SUVertexSetPosition: fn(Ref, *const P3) -> R;
    SUVertexToEntity: fn(Ref) -> Ref;
    SUEntitiesErase: fn(Ref, usize, *mut Ref) -> R;
    SUDimensionRadialCreate: fn(*mut Ref, Ref) -> R;
    SUDimensionRadialSetDiameter: fn(Ref, bool) -> R;
    SUDimensionRadialToDimension: fn(Ref) -> Ref;
    SUModelAddMatchPhotoScene: fn(Ref, *const c_char, Ref, *const c_char, *mut Ref) -> R;
}

static API: OnceLock<std::result::Result<Api, String>> = OnceLock::new();
/// The SketchUp API is not thread-safe: one model is read or written at a time.
static CALLS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Where `SketchUpAPI.dll` is: `ESQ_SKETCHUP_API` if set, otherwise the most recent
/// SketchUp installed under Program Files.
pub fn find_sketchup_api() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ESQ_SKETCHUP_API") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let base =
        PathBuf::from(std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into())).join("SketchUp");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&base)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path().join("SketchUp").join("SketchUpAPI.dll"))
        .filter(|p| p.is_file())
        .collect();
    found.sort();
    found.pop()
}

fn api() -> Result<&'static Api> {
    API.get_or_init(|| {
        let path = find_sketchup_api().ok_or_else(|| {
            "SketchUpAPI.dll introuvable : installez SketchUp ou définissez ESQ_SKETCHUP_API".to_string()
        })?;
        let lib = load_library(&path).map_err(|e| format!("chargement de {} : {e}", path.display()))?;
        let api = unsafe { Api::load(lib) }.map_err(|e| e.to_string())?;
        unsafe { (api.SUInitialize)() };
        Ok(api)
    })
    .as_ref()
    .map_err(|e| anyhow!("{e}"))
}

#[cfg(windows)]
fn load_library(path: &Path) -> Result<Library> {
    use libloading::os::windows::{LOAD_WITH_ALTERED_SEARCH_PATH, Library as WinLib};
    // Lets the DLL find its neighbours (Qt, tbb, …) in the SketchUp folder.
    let lib = unsafe { WinLib::load_with_flags(path, LOAD_WITH_ALTERED_SEARCH_PATH)? };
    Ok(lib.into())
}

#[cfg(not(windows))]
fn load_library(path: &Path) -> Result<Library> {
    Ok(unsafe { Library::new(path)? })
}

/// Whether SketchUp's SDK can be loaded on this machine (it loads it): for the tests and
/// `ESQ_SKP_SDK=1` only, .skp files are read and written without it.
pub fn available() -> bool {
    api().is_ok()
}

pub use sdk_attach::{SdkEnd, SdkOp, ref_type};
pub use sdk_census::CENSUS_CLASSES;

/// The ends of the root's texts and linear dimensions of a file as SketchUp's SDK places them
/// and names what they are attached to, once `op` is done to the model (the SDK moves them as
/// SketchUp does); `None` without SketchUp or when the SDK refuses the file. The oracle of
/// `tests/attaches.rs`.
pub fn sdk_attach_ends(path: &Path, op: SdkOp) -> Option<Vec<SdkEnd>> {
    let a = api().ok()?;
    let _l = CALLS.lock().unwrap_or_else(|e| e.into_inner());
    sdk_attach::attach_ends(a, path, op)
}

/// What the SDK exposes of a file's styles, scenes' styles, classifications, radial
/// dimensions, polylines, fonts, dimension style, texts' fonts and environments, one list of
/// lines per class (`CENSUS_CLASSES`); `None` without SketchUp or when the SDK refuses the
/// file. The oracle of the tests that write SketchUp's files again.
pub fn sdk_census(path: &Path) -> Option<std::collections::BTreeMap<&'static str, Vec<String>>> {
    let a = api().ok()?;
    let _l = CALLS.lock().unwrap_or_else(|e| e.into_inner());
    sdk_census::census_of(a, path)
}

/// An image file decoded by SketchUp's own decoder (`SUImageRepLoadFile`): width, height,
/// channels (1 grey, 3 RGB, 4 RGBA) and the pixels, rows from the top, in that order (the SDK
/// keeps BGR). The oracle of the native decoders (`native::jpeg`).
pub fn sdk_decode_image(path: &Path) -> Result<(usize, usize, usize, Vec<u8>)> {
    let a = api()?;
    let _l = CALLS.lock().unwrap_or_else(|e| e.into_inner());
    let mut rep = Ref::NULL;
    unsafe { (a.SUImageRepCreate)(&mut rep) };
    let p = cstr(&path.to_string_lossy());
    let read = || -> Result<(usize, usize, usize, Vec<u8>)> {
        check(unsafe { (a.SUImageRepLoadFile)(rep, p.as_ptr()) }, "SUImageRepLoadFile")?;
        let (mut w, mut h, mut pad, mut size, mut bits) = (0, 0, 0, 0, 0);
        check(unsafe { (a.SUImageRepGetPixelDimensions)(rep, &mut w, &mut h) }, "SUImageRepGetPixelDimensions")?;
        check(unsafe { (a.SUImageRepGetRowPadding)(rep, &mut pad) }, "SUImageRepGetRowPadding")?;
        check(unsafe { (a.SUImageRepGetDataSize)(rep, &mut size, &mut bits) }, "SUImageRepGetDataSize")?;
        let ch = bits / 8;
        if !(1..=4).contains(&ch) || size < h * (w * ch + pad) {
            bail!("image du SDK inattendue : {w}×{h}, {bits} bits, {size} octets");
        }
        let mut raw = vec![0u8; size];
        check(unsafe { (a.SUImageRepGetData)(rep, size, raw.as_mut_ptr()) }, "SUImageRepGetData")?;
        let mut out = Vec::with_capacity(w * h * ch);
        for row in raw.chunks(w * ch + pad).take(h) {
            for px in row[..w * ch].chunks(ch) {
                match ch {
                    3 | 4 => {
                        out.extend_from_slice(&[px[2], px[1], px[0]]);
                        if ch == 4 {
                            out.push(px[3]);
                        }
                    }
                    _ => out.extend_from_slice(px),
                }
            }
        }
        Ok((w, h, ch, out))
    };
    let out = read();
    unsafe { (a.SUImageRepRelease)(&mut rep) };
    out
}

pub fn api_version() -> Result<(usize, usize)> {
    let a = api()?;
    let (mut ma, mut mi) = (0, 0);
    unsafe { (a.SUGetAPIVersion)(&mut ma, &mut mi) };
    Ok((ma, mi))
}

/// The GUIDs SketchUp gives what a file places (`SUComponentDefinitionGetGuid`,
/// `SUComponentInstanceGetGuid`): those of the definitions placed, and those of the component
/// instances and groups (SketchUp's 22-character form), each list sorted. The oracle of
/// `ComponentDefinition#guid` and `ComponentInstance#guid`.
pub fn sdk_guids(path: &Path) -> Result<(Vec<String>, Vec<String>)> {
    let a = api()?;
    let _l = CALLS.lock().unwrap_or_else(|e| e.into_inner());
    let p = cstr(&path.to_string_lossy());
    let (mut m, mut status) = (Ref::NULL, 0);
    check(unsafe { (a.SUModelCreateFromFileWithStatus)(&mut m, p.as_ptr(), &mut status) }, "SUModelCreateFromFile")?;
    let mut ents = Ref::NULL;
    let out = check(unsafe { (a.SUModelGetEntities)(m, &mut ents) }, "SUModelGetEntities").map(|_| {
        let (mut defs, mut insts) = (Vec::new(), Vec::new());
        let mut seen = std::collections::HashSet::new();
        let mut todo = vec![ents];
        while let Some(e) = todo.pop() {
            let groups = list(
                |n| unsafe { (a.SUEntitiesGetNumGroups)(e, n) },
                |n, p, g| unsafe { (a.SUEntitiesGetGroups)(e, n, p, g) },
            );
            let comps = list(
                |n| unsafe { (a.SUEntitiesGetNumInstances)(e, n) },
                |n, p, g| unsafe { (a.SUEntitiesGetInstances)(e, n, p, g) },
            );
            for i in groups.into_iter().map(|g| unsafe { (a.SUGroupToComponentInstance)(g) }).chain(comps) {
                insts.push(read_string(a, |s| unsafe { (a.SUComponentInstanceGetGuid)(i, s) }));
                let mut d = Ref::NULL;
                if unsafe { (a.SUComponentInstanceGetDefinition)(i, &mut d) } != 0 || !seen.insert(d.0 as usize) {
                    continue;
                }
                defs.push(read_string(a, |s| unsafe { (a.SUComponentDefinitionGetGuid)(d, s) }));
                let mut sub = Ref::NULL;
                if unsafe { (a.SUComponentDefinitionGetEntities)(d, &mut sub) } == 0 {
                    todo.push(sub);
                }
            }
        }
        defs.sort();
        insts.sort();
        (defs, insts)
    });
    unsafe { (a.SUModelRelease)(&mut m) };
    out
}

fn check(r: R, what: &str) -> Result<()> {
    if r == 0 { Ok(()) } else { bail!("{what} a échoué (code SketchUp {r})") }
}

macro_rules! su {
    ($a:ident . $f:ident ( $($arg:expr),* )) => {
        check(unsafe { ($a.$f)($($arg),*) }, stringify!($f))
    };
}
/// SketchUp's own conversion of a file into another version: the SDK opens `from` and saves
/// it as `to` (`None`: the current format). The oracle of what a version keeps of a class.
pub fn sdk_resave(from: &Path, to: &Path, version: Option<SkpVersion>) -> Result<()> {
    let a = api().map_err(|e| anyhow!(e))?;
    let _l = CALLS.lock().unwrap_or_else(|e| e.into_inner());
    let src = cstr(&from.to_string_lossy());
    let (mut m, mut status) = (Ref::NULL, 0);
    su!(a.SUModelCreateFromFileWithStatus(&mut m, src.as_ptr(), &mut status))?;
    let dst = cstr(&to.to_string_lossy());
    let v = version.map_or(SkpVersion::Su2021 as i32, |v| v as i32);
    let r = su!(a.SUModelSaveToFileWithVersion(m, dst.as_ptr(), v));
    unsafe { (a.SUModelRelease)(&mut m) };
    r
}

/// For the tests of texture placements: SketchUp's own edit of a file's faces. The SDK opens
/// `from`, moves every face of the root by `t` (`SUEntitiesTransform`, in millimetres)
/// or, without `t`, reverses each of them (`SUFaceReverse`), and saves the result as `to`.
pub fn sdk_edit_faces(from: &Path, to: &Path, t: Option<&DMat4>) -> Result<()> {
    let a = api().map_err(|e| anyhow!(e))?;
    let _l = CALLS.lock().unwrap_or_else(|e| e.into_inner());
    let src = cstr(&from.to_string_lossy());
    let (mut m, mut status) = (Ref::NULL, 0);
    su!(a.SUModelCreateFromFileWithStatus(&mut m, src.as_ptr(), &mut status))?;
    let r = (|| {
        let mut ents = Ref::NULL;
        su!(a.SUModelGetEntities(m, &mut ents))?;
        let faces = list(
            |n| unsafe { (a.SUEntitiesGetNumFaces)(ents, n) },
            |n, p, g| unsafe { (a.SUEntitiesGetFaces)(ents, n, p, g) },
        );
        match t {
            Some(t) => {
                let refs: Vec<Ref> = faces.iter().map(|f| unsafe { (a.SUFaceToEntity)(*f) }).collect();
                su!(a.SUEntitiesTransform(ents, refs.len(), refs.as_ptr(), &xf_from_mm(t)))?;
            }
            None => {
                for f in faces {
                    su!(a.SUFaceReverse(f))?;
                }
            }
        }
        let dst = cstr(&to.to_string_lossy());
        su!(a.SUModelSaveToFile(m, dst.as_ptr()))
    })();
    unsafe { (a.SUModelRelease)(&mut m) };
    r
}

/// What the corpus of the SDK (`examples/skp_corpus.rs`) adds with the SDK itself to a file the
/// SDK wrote: styles from `.style` files (the first one made the selected one),
/// classification schemas (`.skc`), a radial dimension on each arc of the root (the odd ones
/// diameters), a scene matched to a photo (its image file, its name: the model's camera).
#[derive(Clone, Debug, Default)]
pub struct CorpusExtras {
    pub styles: Vec<PathBuf>,
    pub schemas: Vec<PathBuf>,
    pub radials: bool,
    pub photo: Option<(PathBuf, String)>,
}

/// Opens `from` with the SDK, adds `extras`, saves it as `to` in `version` (`None`: the current
/// format).
pub fn sdk_corpus_extras(from: &Path, to: &Path, version: Option<SkpVersion>, extras: &CorpusExtras) -> Result<()> {
    let a = api().map_err(|e| anyhow!(e))?;
    let _l = CALLS.lock().unwrap_or_else(|e| e.into_inner());
    let src = cstr(&from.to_string_lossy());
    let (mut m, mut status) = (Ref::NULL, 0);
    su!(a.SUModelCreateFromFileWithStatus(&mut m, src.as_ptr(), &mut status))?;
    let res = (|| -> Result<()> {
        let mut styles = Ref::NULL;
        su!(a.SUModelGetStyles(m, &mut styles))?;
        for (k, s) in extras.styles.iter().enumerate() {
            let p = cstr(&s.to_string_lossy());
            // (`SUStylesApplyStyleToScene` crashes SketchUpAPI.dll 26.2: the scenes' styles come
            // from the real corpus.)
            su!(a.SUStylesAddStyle(styles, p.as_ptr(), k == 0)).with_context(|| format!("style {}", s.display()))?;
        }
        if !extras.schemas.is_empty() {
            let mut cl = Ref::NULL;
            su!(a.SUModelGetClassifications(m, &mut cl))?;
            for s in &extras.schemas {
                let p = cstr(&s.to_string_lossy());
                su!(a.SUClassificationsLoadSchema(cl, p.as_ptr()))
                    .with_context(|| format!("schéma {}", s.display()))?;
            }
        }
        if extras.radials {
            let mut ents = Ref::NULL;
            su!(a.SUModelGetEntities(m, &mut ents))?;
            let mut n = 0;
            su!(a.SUEntitiesGetNumArcCurves(ents, &mut n))?;
            let mut arcs = vec![Ref::NULL; n];
            let mut got = 0;
            su!(a.SUEntitiesGetArcCurves(ents, n, arcs.as_mut_ptr(), &mut got))?;
            let mut dims = Vec::new();
            for (k, arc) in arcs.iter().take(got).enumerate() {
                let mut path = Ref::NULL;
                su!(a.SUInstancePathCreate(&mut path))?;
                let leaf = unsafe { (a.SUCurveToEntity)((a.SUArcCurveToCurve)(*arc)) };
                su!(a.SUInstancePathSetLeaf(path, leaf))?;
                let mut d = Ref::NULL;
                let r = su!(a.SUDimensionRadialCreate(&mut d, path));
                unsafe { (a.SUInstancePathRelease)(&mut path) };
                r?;
                su!(a.SUDimensionRadialSetDiameter(d, k % 2 == 1))?;
                dims.push(unsafe { (a.SUDimensionRadialToDimension)(d) });
            }
            if !dims.is_empty() {
                su!(a.SUEntitiesAddDimensions(ents, dims.len(), dims.as_ptr()))?;
            }
        }
        if let Some((img, name)) = &extras.photo {
            let mut cam = Ref::NULL;
            su!(a.SUModelGetCamera(m, &mut cam))?;
            let (p, n) = (cstr(&img.to_string_lossy()), cstr(name));
            let mut scene = Ref::NULL;
            su!(a.SUModelAddMatchPhotoScene(m, p.as_ptr(), cam, n.as_ptr(), &mut scene)).context("photo adaptée")?;
        }
        let dst = cstr(&to.to_string_lossy());
        let v = version.map_or(SkpVersion::Su2021 as i32, |v| v as i32);
        su!(a.SUModelSaveToFileWithVersion(m, dst.as_ptr(), v))
    })();
    unsafe { (a.SUModelRelease)(&mut m) };
    res
}

fn cstr(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

fn read_string(a: &Api, getter: impl FnOnce(*mut Ref) -> R) -> String {
    let mut s = Ref::NULL;
    if unsafe { (a.SUStringCreate)(&mut s) } != 0 {
        return String::new();
    }
    let mut out = String::new();
    if getter(&mut s) == 0 {
        let mut len = 0;
        unsafe { (a.SUStringGetUTF8Length)(s, &mut len) };
        let mut buf = vec![0u8; len + 1];
        let mut copied = 0;
        unsafe { (a.SUStringGetUTF8)(s, buf.len(), buf.as_mut_ptr() as *mut c_char, &mut copied) };
        buf.truncate(copied.min(len));
        out = String::from_utf8_lossy(&buf).into_owned();
    }
    unsafe { (a.SUStringRelease)(&mut s) };
    out
}

/// Text of a `SUStringRef` that already exists.
fn string_of(a: &Api, s: Ref) -> String {
    let mut len = 0;
    unsafe { (a.SUStringGetUTF8Length)(s, &mut len) };
    let mut buf = vec![0u8; len + 1];
    let mut copied = 0;
    unsafe { (a.SUStringGetUTF8)(s, buf.len(), buf.as_mut_ptr() as *mut c_char, &mut copied) };
    buf.truncate(copied.min(len));
    String::from_utf8_lossy(&buf).into_owned()
}

/// `SUTypedValueType` values (typed_value.h).
mod tv {
    pub const EMPTY: i32 = 0;
    pub const BYTE: i32 = 1;
    pub const SHORT: i32 = 2;
    pub const INT32: i32 = 3;
    pub const FLOAT: i32 = 4;
    pub const DOUBLE: i32 = 5;
    pub const BOOL: i32 = 6;
    pub const COLOR: i32 = 7;
    pub const TIME: i32 = 8;
    pub const STRING: i32 = 9;
    pub const VECTOR3D: i32 = 10;
    pub const ARRAY: i32 = 11;
}

/// The attribute dictionaries of an entity.
fn read_attrs(a: &Api, entity: Ref) -> Attributes {
    if entity.is_null() {
        return Attributes::new();
    }
    read_dicts(
        a,
        list(
            |n| unsafe { (a.SUEntityGetNumAttributeDictionaries)(entity, n) },
            |n, p, g| unsafe { (a.SUEntityGetAttributeDictionaries)(entity, n, p, g) },
        ),
    )
}

/// The model's own attribute dictionaries (`Model#attribute_dictionaries`).
fn read_model_attrs(a: &Api, m: Ref) -> Attributes {
    read_dicts(
        a,
        list(
            |n| unsafe { (a.SUModelGetNumAttributeDictionaries)(m, n) },
            |n, p, g| unsafe { (a.SUModelGetAttributeDictionaries)(m, n, p, g) },
        ),
    )
}

fn read_dicts(a: &Api, dicts: Vec<Ref>) -> Attributes {
    let mut out = Attributes::new();
    for d in dicts {
        let name = read_string(a, |s| unsafe { (a.SUAttributeDictionaryGetName)(d, s) });
        let mut count = 0;
        if unsafe { (a.SUAttributeDictionaryGetNumKeys)(d, &mut count) } != 0 || count == 0 {
            continue;
        }
        let mut keys = vec![Ref::NULL; count];
        for k in &mut keys {
            unsafe { (a.SUStringCreate)(k) };
        }
        let mut got = 0;
        if unsafe { (a.SUAttributeDictionaryGetKeys)(d, count, keys.as_mut_ptr(), &mut got) } == 0 {
            for k in &keys[..got.min(count)] {
                let key = string_of(a, *k);
                let ckey = cstr(&key);
                let mut v = Ref::NULL;
                if unsafe { (a.SUTypedValueCreate)(&mut v) } != 0 {
                    continue;
                }
                if unsafe { (a.SUAttributeDictionaryGetValue)(d, ckey.as_ptr(), &mut v) } == 0
                    && let Some(val) = read_typed(a, v)
                {
                    out.entry(name.clone()).or_default().insert(key, val);
                }
                unsafe { (a.SUTypedValueRelease)(&mut v) };
            }
        }
        for k in &mut keys {
            unsafe { (a.SUStringRelease)(k) };
        }
    }
    out
}

/// A typed value as an attribute value: strings, floats and booleans as such, the rest
/// (integers, colours, times, vectors, arrays, empty) tagged as the Ruby bridge tags them
/// (`{"t": "i", "v": 3}`, `{"t": "c", "v": [r, g, b, a]}`…).
fn read_typed(a: &Api, v: Ref) -> Option<AttrValue> {
    use serde_json::Value as J;
    Some(match typed_tagged(a, v, 0)? {
        J::Object(o) if o.get("t").and_then(J::as_str) == Some("s") => {
            AttrValue::Text(o.get("v").and_then(J::as_str).unwrap_or_default().to_string())
        }
        J::Object(o) if o.get("t").and_then(J::as_str) == Some("f") => {
            AttrValue::Number(o.get("v").and_then(J::as_f64).unwrap_or_default())
        }
        J::Object(o) if o.get("t").and_then(J::as_str) == Some("b") => {
            AttrValue::Bool(o.get("v").and_then(J::as_bool).unwrap_or_default())
        }
        j => AttrValue::Json(j),
    })
}

/// A typed value in the Ruby bridge's tagged JSON.
fn typed_tagged(a: &Api, v: Ref, depth: usize) -> Option<serde_json::Value> {
    use serde_json::json;
    let mut t = 0;
    if unsafe { (a.SUTypedValueGetType)(v, &mut t) } != 0 {
        return None;
    }
    Some(match t {
        tv::EMPTY => json!({"t": "n"}),
        tv::BYTE => {
            let mut x = 0u8;
            unsafe { (a.SUTypedValueGetByte)(v, &mut x) };
            json!({"t": "i", "v": x})
        }
        tv::SHORT => {
            let mut x = 0i16;
            unsafe { (a.SUTypedValueGetInt16)(v, &mut x) };
            json!({"t": "i", "v": x})
        }
        tv::INT32 => {
            let mut x = 0i32;
            unsafe { (a.SUTypedValueGetInt32)(v, &mut x) };
            json!({"t": "i", "v": x})
        }
        tv::FLOAT => {
            let mut x = 0f32;
            unsafe { (a.SUTypedValueGetFloat)(v, &mut x) };
            json!({"t": "f", "v": x as f64})
        }
        tv::DOUBLE => {
            let mut x = 0f64;
            unsafe { (a.SUTypedValueGetDouble)(v, &mut x) };
            if x.is_finite() {
                json!({"t": "f", "v": x})
            } else {
                json!({"t": "nf", "v": if x.is_nan() { "nan" } else if x > 0.0 { "inf" } else { "-inf" }})
            }
        }
        tv::BOOL => {
            let mut x = false;
            unsafe { (a.SUTypedValueGetBool)(v, &mut x) };
            json!({"t": "b", "v": x})
        }
        tv::COLOR => {
            let mut c = Color::default();
            unsafe { (a.SUTypedValueGetColor)(v, &mut c) };
            json!({"t": "c", "v": [c.r, c.g, c.b, c.a]})
        }
        tv::TIME => {
            let mut x = 0i64;
            unsafe { (a.SUTypedValueGetTime)(v, &mut x) };
            json!({"t": "time", "v": [x, 0]})
        }
        tv::STRING => json!({"t": "s", "v": read_string(a, |s| unsafe { (a.SUTypedValueGetString)(v, s) })}),
        tv::VECTOR3D => {
            let mut x = [0.0; 3];
            unsafe { (a.SUTypedValueGetVector3d)(v, &mut x) };
            json!({"t": "vec", "v": x})
        }
        tv::ARRAY if depth < 16 => {
            // The items belong to the array (measured: passing created values and releasing
            // them corrupts the model's memory).
            let mut n = 0;
            unsafe { (a.SUTypedValueGetNumArrayItems)(v, &mut n) };
            let mut items = vec![Ref::NULL; n];
            let mut got = 0;
            let mut out = Vec::new();
            if n > 0 && unsafe { (a.SUTypedValueGetArrayItems)(v, n, items.as_mut_ptr(), &mut got) } == 0 {
                out = items[..got.min(n)].iter().filter_map(|it| typed_tagged(a, *it, depth + 1)).collect();
            }
            json!({"t": "a", "v": out})
        }
        _ => return None,
    })
}

/// Sets a typed value from the Ruby bridge's tagged JSON.
fn set_tagged(a: &Api, v: Ref, j: &serde_json::Value, depth: usize) -> R {
    use serde_json::Value as J;
    let val = j.get("v").unwrap_or(&J::Null);
    let num = |k: usize| val.get(k).and_then(J::as_f64).unwrap_or_default();
    unsafe {
        match j.get("t").and_then(J::as_str).unwrap_or("n") {
            "s" => {
                let s = cstr(val.as_str().unwrap_or_default());
                (a.SUTypedValueSetString)(v, s.as_ptr())
            }
            "b" => (a.SUTypedValueSetBool)(v, val.as_bool().unwrap_or_default()),
            "i" => match val.as_i64() {
                Some(n) if i32::try_from(n).is_ok() => (a.SUTypedValueSetInt32)(v, n as i32),
                _ => (a.SUTypedValueSetDouble)(v, val.as_f64().unwrap_or_default()),
            },
            "f" | "l" => (a.SUTypedValueSetDouble)(v, val.as_f64().unwrap_or_default()),
            "nf" => (a.SUTypedValueSetDouble)(
                v,
                match val.as_str() {
                    Some("inf") => f64::INFINITY,
                    Some("-inf") => f64::NEG_INFINITY,
                    _ => f64::NAN,
                },
            ),
            "c" => {
                let c = |k: usize| val.get(k).and_then(J::as_u64).unwrap_or(255).min(255) as u8;
                (a.SUTypedValueSetColor)(v, &Color { r: c(0), g: c(1), b: c(2), a: c(3) })
            }
            "time" => (a.SUTypedValueSetTime)(v, val.get(0).and_then(J::as_i64).unwrap_or_default()),
            "p" | "vec" => (a.SUTypedValueSetVector3d)(v, &[num(0), num(1), num(2)]),
            "a" if depth < 16 => {
                let items: Vec<&J> = val.as_array().map(|x| x.iter().collect()).unwrap_or_default();
                let mut refs = vec![Ref::NULL; items.len()];
                let mut r = 0;
                for (it, j) in refs.iter_mut().zip(&items) {
                    (a.SUTypedValueCreate)(it);
                    if r == 0 {
                        r = set_tagged(a, *it, j, depth + 1);
                    }
                }
                if r == 0 {
                    r = (a.SUTypedValueSetArrayItems)(v, refs.len(), refs.as_mut_ptr());
                }
                for it in &mut refs {
                    (a.SUTypedValueRelease)(it);
                }
                r
            }
            // nil (and anything SketchUp cannot store): an empty value.
            _ => 0,
        }
    }
}

/// Writes attribute dictionaries on an entity.
fn write_attrs(a: &Api, entity: Ref, attrs: &Attributes) -> Result<()> {
    write_dicts(a, attrs, |name| {
        let mut d = Ref::NULL;
        if unsafe { (a.SUEntityGetAttributeDictionary)(entity, name.as_ptr(), &mut d) } != 0 {
            d = Ref::NULL;
            su!(a.SUAttributeDictionaryCreate(&mut d, name.as_ptr()))?;
            su!(a.SUEntityAddAttributeDictionary(entity, d))?;
        }
        Ok(d)
    })
}

/// Writes the model's attribute dictionaries (`SUModelGetAttributeDictionary` makes them).
fn write_model_attrs(a: &Api, m: Ref, attrs: &Attributes) -> Result<()> {
    write_dicts(a, attrs, |name| {
        let mut d = Ref::NULL;
        su!(a.SUModelGetAttributeDictionary(m, name.as_ptr(), &mut d))?;
        Ok(d)
    })
}

fn write_dicts(a: &Api, attrs: &Attributes, mut dict: impl FnMut(&CString) -> Result<Ref>) -> Result<()> {
    for (name, values) in attrs {
        let d = dict(&cstr(name))?;
        for (key, value) in values {
            let ckey = cstr(key);
            let mut v = Ref::NULL;
            su!(a.SUTypedValueCreate(&mut v))?;
            let set = match value {
                AttrValue::Number(n) => unsafe { (a.SUTypedValueSetDouble)(v, *n) },
                AttrValue::Bool(b) => unsafe { (a.SUTypedValueSetBool)(v, *b) },
                AttrValue::Text(s) => {
                    let text = cstr(s);
                    unsafe { (a.SUTypedValueSetString)(v, text.as_ptr()) }
                }
                AttrValue::Json(j) => set_tagged(a, v, j, 0),
            };
            let res =
                check(set, "SUTypedValueSet").and_then(|_| su!(a.SUAttributeDictionarySetValue(d, ckey.as_ptr(), v)));
            unsafe { (a.SUTypedValueRelease)(&mut v) };
            res?;
        }
    }
    Ok(())
}

fn list(count: impl FnOnce(&mut usize) -> R, get: impl FnOnce(usize, *mut Ref, &mut usize) -> R) -> Vec<Ref> {
    let mut n = 0;
    if count(&mut n) != 0 || n == 0 {
        return Vec::new();
    }
    let mut v = vec![Ref::NULL; n];
    let mut got = 0;
    if get(n, v.as_mut_ptr(), &mut got) != 0 {
        return Vec::new();
    }
    v.truncate(got);
    v
}

fn xf_to_mm(x: &Xf) -> DMat4 {
    let mut m = DMat4::from_cols_array(&x.v);
    m.w_axis.x *= IN;
    m.w_axis.y *= IN;
    m.w_axis.z *= IN;
    m
}

fn xf_from_mm(m: &DMat4) -> Xf {
    let mut v = m.to_cols_array();
    v[12] /= IN;
    v[13] /= IN;
    v[14] /= IN;
    Xf { v }
}

/// `SURefType_Snap`, asked of a snap the API makes (the enumeration has grown in the middle
/// over the versions, so its number is not hard-coded).
fn snap_type(a: &Api) -> Option<i32> {
    static T: OnceLock<Option<i32>> = OnceLock::new();
    *T.get_or_init(|| {
        let mut s = Ref::NULL;
        // SAFETY: a snap is created, its type read, and the snap released.
        unsafe {
            if (a.SUSnapCreate)(&mut s, P3::default(), std::ptr::null(), std::ptr::null()) != 0 || s.is_null() {
                return None;
            }
            let t = (a.SUEntityGetType)((a.SUSnapToEntity)(s));
            (a.SUSnapRelease)(&mut s);
            Some(t)
        }
    })
}

/// The entities of type `t` (`SURefType`) of an entities collection.
fn typed_entities(a: &Api, ents: Ref, t: i32) -> Vec<Ref> {
    let mut out = Vec::new();
    let (mut list, mut it) = (Ref::NULL, Ref::NULL);
    // SAFETY: the list and its iterator are created here and released at the end; the
    // entities it gives stay owned by the model.
    unsafe {
        if (a.SUEntityListCreate)(&mut list) != 0 {
            return out;
        }
        if (a.SUEntitiesEntityListFill)(ents, t, list) == 0
            && (a.SUEntityListIteratorCreate)(&mut it) == 0
            && (a.SUEntityListBegin)(list, &mut it) == 0
        {
            let mut in_range = false;
            while (a.SUEntityListIteratorIsInRange)(it, &mut in_range) == 0 && in_range {
                let mut e = Ref::NULL;
                if (a.SUEntityListIteratorGetEntity)(it, &mut e) == 0 && !e.is_null() {
                    out.push(e);
                }
                if (a.SUEntityListIteratorNext)(it) != 0 {
                    break;
                }
            }
        }
        if !it.is_null() {
            (a.SUEntityListIteratorRelease)(&mut it);
        }
        (a.SUEntityListRelease)(&mut list);
    }
    out
}

/// The component snaps of an entities collection (`Sketchup::Snap`): position, direction
/// and up, in millimetres.
fn read_snaps(a: &Api, ents: Ref) -> Vec<esq_core::model::SnapPoint> {
    let Some(t) = snap_type(a) else { return Vec::new() };
    let mut out = Vec::new();
    for e in typed_entities(a, ents, t) {
        // SAFETY: `e` is a snap of the collection; plain getters into local structs.
        unsafe {
            let s = (a.SUSnapFromEntity)(e);
            let (mut p, mut d, mut u) = (P3::default(), P3::default(), P3::default());
            if !s.is_null()
                && (a.SUSnapGetPosition)(s, &mut p) == 0
                && (a.SUSnapGetDirection)(s, &mut d) == 0
                && (a.SUSnapGetUp)(s, &mut u) == 0
            {
                let v = |q: P3| DVec3::new(q.x, q.y, q.z);
                out.push(esq_core::model::SnapPoint::new(v(p) * IN, v(d), v(u)));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------
// Import

struct Reader<'a> {
    a: &'a Api,
    model: Model,
    materials: HashMap<usize, MaterialId>,
    layers: HashMap<usize, LayerIdx>,
    defs: HashMap<usize, DefId>,
    /// Attribute dictionaries of the faces and edges of the collection being read: kind,
    /// id, dictionaries. They go to `Model::attr_store` once the collection has its id.
    pending: Pending,
    /// The font of the model's dimension style: the SDK gives it for a dimension without a
    /// font of its own.
    dim_font: Ref,
    /// The names of the file's components, which no group's definition takes (as the native
    /// reader: a component may be called « Groupe#1 »).
    component_names: HashSet<String>,
}

/// Dictionaries of a collection's faces and edges waiting for the collection's id.
type Pending = Vec<(char, u64, Attributes)>;

/// `SUComponentBehavior` as Esquisse keeps it (`Sketchup::Behavior`): `component_snap` is
/// none (does not glue), any, horizontal, vertical or sloped.
fn behavior_of(b: &Behavior) -> esq_core::model::Behavior {
    esq_core::model::Behavior {
        always_face_camera: b.always_face_camera,
        cuts_opening: b.cuts_opening,
        is2d: b.snap != 0,
        shadows_face_sun: b.shadows_face_sun,
        no_scale_mask: b.no_scale_mask as i32,
        snapto: (b.snap - 1).clamp(0, 3),
    }
}

fn behavior_to(b: &esq_core::model::Behavior) -> Behavior {
    Behavior {
        snap: if b.is2d { b.snapto.clamp(0, 3) + 1 } else { 0 },
        cuts_opening: b.cuts_opening,
        always_face_camera: b.always_face_camera,
        shadows_face_sun: b.shadows_face_sun,
        no_scale_mask: b.no_scale_mask.max(0) as usize,
    }
}

/// Key of the side table entry of an entity's dictionaries (the Ruby bridge's handle).
fn attr_owner(kind: char, ctx: Option<DefId>, ffi: u64) -> String {
    use slotmap::Key;
    format!("h:{kind}:{}:{ffi}", ctx.map_or(0, |d| d.data().as_ffi()))
}

/// A value of SketchUp's shadow settings (`SUShadowInfo`).
enum Typed<'s> {
    Bool(bool),
    Int(i32),
    Double(f64),
    Time(i64),
    Str(&'s str),
}

fn read_shadow(a: &Api, m: Ref) -> esq_core::sun::ShadowInfo {
    let mut s = esq_core::sun::ShadowInfo::default();
    let mut si = Ref::NULL;
    if unsafe { (a.SUModelGetShadowInfo)(m, &mut si) } != 0 {
        return s;
    }
    let get = |key: &str, f: &mut dyn FnMut(Ref) -> R| {
        let k = cstr(key);
        let mut tv = Ref::NULL;
        if unsafe { (a.SUTypedValueCreate)(&mut tv) } != 0 {
            return;
        }
        if unsafe { (a.SUShadowInfoGetValue)(si, k.as_ptr(), &mut tv) } == 0 {
            f(tv);
        }
        unsafe { (a.SUTypedValueRelease)(&mut tv) };
    };
    let flag = |key: &str, out: &mut bool| get(key, &mut |tv| unsafe { (a.SUTypedValueGetBool)(tv, out) });
    let int = |key: &str, out: &mut i32| get(key, &mut |tv| unsafe { (a.SUTypedValueGetInt32)(tv, out) });
    let real = |key: &str, out: &mut f64| get(key, &mut |tv| unsafe { (a.SUTypedValueGetDouble)(tv, out) });
    flag("DisplayShadows", &mut s.display);
    get("ShadowTime", &mut |tv| unsafe { (a.SUTypedValueGetTime)(tv, &mut s.time) });
    real("TZOffset", &mut s.tz_offset);
    int("Light", &mut s.light);
    int("Dark", &mut s.dark);
    flag("UseSunForAllShading", &mut s.use_sun_for_shading);
    flag("DisplayOnAllFaces", &mut s.on_faces);
    flag("DisplayOnGroundPlane", &mut s.on_ground);
    flag("EdgesCastShadows", &mut s.from_edges);
    real("Latitude", &mut s.latitude);
    real("Longitude", &mut s.longitude);
    real("NorthAngle", &mut s.north_angle);
    get("City", &mut |tv| {
        s.city = read_string(a, |x| unsafe { (a.SUTypedValueGetString)(tv, x) });
        0
    });
    get("Country", &mut |tv| {
        s.country = read_string(a, |x| unsafe { (a.SUTypedValueGetString)(tv, x) });
        0
    });
    s
}

fn write_shadow(a: &Api, m: Ref, s: &esq_core::sun::ShadowInfo) -> Result<()> {
    let mut si = Ref::NULL;
    su!(a.SUModelGetShadowInfo(m, &mut si))?;
    let values = [
        ("DisplayShadows", Typed::Bool(s.display)),
        ("ShadowTime", Typed::Time(s.time)),
        ("TZOffset", Typed::Double(s.tz_offset)),
        ("Light", Typed::Int(s.light)),
        ("Dark", Typed::Int(s.dark)),
        ("UseSunForAllShading", Typed::Bool(s.use_sun_for_shading)),
        ("DisplayOnAllFaces", Typed::Bool(s.on_faces)),
        ("DisplayOnGroundPlane", Typed::Bool(s.on_ground)),
        ("EdgesCastShadows", Typed::Bool(s.from_edges)),
        ("Latitude", Typed::Double(s.latitude)),
        ("Longitude", Typed::Double(s.longitude)),
        ("NorthAngle", Typed::Double(s.north_angle)),
        ("City", Typed::Str(&s.city)),
        ("Country", Typed::Str(&s.country)),
    ];
    for (key, v) in values {
        let mut tv = Ref::NULL;
        su!(a.SUTypedValueCreate(&mut tv))?;
        let text;
        let r = unsafe {
            match v {
                Typed::Bool(b) => (a.SUTypedValueSetBool)(tv, b),
                Typed::Int(i) => (a.SUTypedValueSetInt32)(tv, i),
                Typed::Double(d) => (a.SUTypedValueSetDouble)(tv, d),
                Typed::Time(t) => (a.SUTypedValueSetTime)(tv, t),
                Typed::Str(x) => {
                    text = cstr(x);
                    (a.SUTypedValueSetString)(tv, text.as_ptr())
                }
            }
        };
        let k = cstr(key);
        let r = if r == 0 { unsafe { (a.SUShadowInfoSetValue)(si, k.as_ptr(), tv) } } else { r };
        unsafe { (a.SUTypedValueRelease)(&mut tv) };
        check(r, key)?;
    }
    Ok(())
}

/// Point of view stored in a .skp file (millimetres).
pub type View = esq_core::model::SceneCamera;

/// Whether .skp files are read through SketchUp's SDK (`ESQ_SKP_SDK=1`) rather than natively.
pub fn sdk_reader_forced() -> bool {
    std::env::var("ESQ_SKP_SDK").is_ok_and(|v| v.trim() == "1")
}

/// The GUID of the model saved in a .skp file (`Sketchup::Skp.read_guid`): `None` when the
/// file is not a SketchUp model. Read natively from the file's header (or `meta/meta.dat`),
/// through the SDK with `ESQ_SKP_SDK=1`.
pub fn read_guid(path: &Path) -> Result<Option<String>> {
    if !sdk_reader_forced() || !available() {
        let data = std::fs::read(path)?;
        return Ok(native::guid(&data).ok().filter(|g| !g.is_empty()));
    }
    let a = api()?;
    let _one_at_a_time = CALLS.lock().unwrap_or_else(|e| e.into_inner());
    let p = cstr(&path.to_string_lossy());
    let (mut m, mut status) = (Ref::NULL, 0);
    // SAFETY: the model is opened, its GUID read into a string, and the model released.
    if unsafe { (a.SUModelCreateFromFileWithStatus)(&mut m, p.as_ptr(), &mut status) } != 0 || m.is_null() {
        return Ok(None);
    }
    let g = read_string(a, |s| unsafe { (a.SUModelGetGuid)(m, s) });
    unsafe { (a.SUModelRelease)(&mut m) };
    Ok((!g.is_empty()).then_some(g))
}

/// Opens a .skp file.
pub fn import_skp(path: &Path) -> Result<Model> {
    import_skp_view(path).map(|(m, _)| m)
}

/// Opens a .skp file and returns the camera it was saved with: with the native reader
/// (`native`, every version from SketchUp 3), or through SketchUp's SDK when `ESQ_SKP_SDK=1`
/// (and SketchUp is installed).
pub fn import_skp_view(path: &Path) -> Result<(Model, Option<View>)> {
    if !sdk_reader_forced() || !available() {
        let data = std::fs::read(path).with_context(|| format!("ouverture de {}", path.display()))?;
        let (mut m, view) = native::read_bytes(&data).with_context(|| format!("ouverture de {}", path.display()))?;
        // The same checks as a .esq (esq_core::repair): no dangling vertex key, no component
        // that contains itself, no tree unfolding into hundreds of millions of placements.
        if m.check_after_reading()
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("ouverture de {}", path.display()))?
            > 0
        {
            m.rebuild_topology();
        }
        return Ok((m, view));
    }
    sdk_import_skp_view(path)
}

/// Opens a .skp file through SketchUp's SDK (the reference the native reader is tested against).
pub fn sdk_import_skp_view(path: &Path) -> Result<(Model, Option<View>)> {
    let a = api()?;
    let _one_at_a_time = CALLS.lock().unwrap_or_else(|e| e.into_inner());
    let p = cstr(&path.to_string_lossy());
    let mut m = Ref::NULL;
    let mut status = 0;
    su!(a.SUModelCreateFromFileWithStatus(&mut m, p.as_ptr(), &mut status))
        .with_context(|| format!("ouverture de {}", path.display()))?;
    let res = read_model(a, m).map(|model| (model, read_view(a, m)));
    unsafe { (a.SUModelRelease)(&mut m) };
    res
}

fn read_view(a: &Api, m: Ref) -> Option<View> {
    let mut cam = Ref::NULL;
    if unsafe { (a.SUModelGetCamera)(m, &mut cam) } != 0 {
        return None;
    }
    read_camera(a, cam)
}

fn read_camera(a: &Api, cam: Ref) -> Option<View> {
    let (mut e, mut t, mut u) = (P3::default(), P3::default(), P3::default());
    if unsafe { (a.SUCameraGetOrientation)(cam, &mut e, &mut t, &mut u) } != 0 {
        return None;
    }
    let mut persp = true;
    let mut fov = 35.0;
    unsafe {
        (a.SUCameraGetPerspective)(cam, &mut persp);
        (a.SUCameraGetPerspectiveFrustumFOV)(cam, &mut fov);
    }
    let v = |p: P3| DVec3::new(p.x, p.y, p.z);
    Some(View { eye: v(e) * IN, target: v(t) * IN, up: v(u), fov_deg: fov, perspective: persp })
}

fn read_model(a: &Api, m: Ref) -> Result<Model> {
    let mut r = Reader {
        a,
        model: Model::new(),
        materials: HashMap::new(),
        layers: HashMap::new(),
        defs: HashMap::new(),
        pending: Vec::new(),
        dim_font: Ref::NULL,
        component_names: HashSet::new(),
    };
    for d in list(
        |n| unsafe { (a.SUModelGetNumComponentDefinitions)(m, n) },
        |n, p, g| unsafe { (a.SUModelGetComponentDefinitions)(m, n, p, g) },
    ) {
        r.component_names.insert(read_string(a, |s| unsafe { (a.SUComponentDefinitionGetName)(d, s) }));
    }
    let mut ds = Ref::NULL;
    if unsafe { (a.SUModelGetDimensionStyle)(m, &mut ds) } == 0 && !ds.is_null() {
        unsafe { (a.SUDimensionStyleGetFont)(ds, &mut r.dim_font) };
    }
    let mut units = 0;
    unsafe { (a.SUModelGetUnits)(m, &mut units) };
    r.model.units = match units {
        0 => Units::Inches,
        1 => Units::Feet,
        3 => Units::Centimeters,
        4 => Units::Meters,
        5 => Units::Yards,
        _ => Units::Millimeters,
    };
    r.model.shadow = read_shadow(a, m);
    r.read_layers(m);
    r.read_model_info(m);
    // The styles: what the SDK says of them (name, description, GUID, path).
    let mut style_names: HashMap<usize, String> = HashMap::new();
    let mut styles = Ref::NULL;
    if unsafe { (a.SUModelGetStyles)(m, &mut styles) } == 0 && !styles.is_null() {
        let all = list(
            |n| unsafe { (a.SUStylesGetNumStyles)(styles, n) },
            |n, p, g| unsafe { (a.SUStylesGetStyles)(styles, n, p, g) },
        );
        let mut sel = Ref::NULL;
        unsafe { (a.SUStylesGetSelectedStyle)(styles, &mut sel) };
        let mut entries = Vec::new();
        let mut selected = 1;
        for (i, s) in all.iter().enumerate() {
            let text = |f: unsafe extern "C" fn(Ref, *mut Ref) -> R| read_string(a, |x| unsafe { f(*s, x) });
            let name = text(a.SUStyleGetName);
            style_names.insert(s.0 as usize, name.clone());
            if s.0 == sel.0 {
                selected = i as i64 + 1;
            }
            entries.push(serde_json::json!({
                "id": i as i64 + 1, "name": name, "description": text(a.SUStyleGetDescription),
                "path": text(a.SUStyleGetPath), "guid": text(a.SUStyleGetGuid), "ro": {}, "wm": [],
            }));
        }
        if !entries.is_empty() {
            r.model.view_ext.insert("styles".into(), serde_json::json!({"list": entries, "selected": selected}));
        }
    }
    for mat in
        list(|n| unsafe { (a.SUModelGetNumMaterials)(m, n) }, |n, p, g| unsafe { (a.SUModelGetMaterials)(m, n, p, g) })
    {
        r.material(mat);
    }

    let mut ents = Ref::NULL;
    su!(a.SUModelGetEntities(m, &mut ents))?;
    let (root, attrs) = r.read_entities(ents)?;
    r.model.root = root;
    r.store_attrs(None, attrs);
    r.model.rebuild_topology();

    let mut scene_attrs = Vec::new();
    // The scene selected when the file was saved (`Pages#selected_page`, the highlighted tab).
    let mut active = Ref::NULL;
    // SAFETY: the API writes one scene reference into `active` (null on entry, as required).
    let active = (unsafe { (a.SUModelGetActiveScene)(m, &mut active) } == 0).then_some(active.0 as usize);
    for s in list(|n| unsafe { (a.SUModelGetNumScenes)(m, n) }, |n, p, g| unsafe { (a.SUModelGetScenes)(m, n, p, g) }) {
        let name = read_string(a, |x| unsafe { (a.SUSceneGetName)(s, x) });
        let description = read_string(a, |x| unsafe { (a.SUSceneGetDescription)(s, x) });
        let mut cam = Ref::NULL;
        let camera = if unsafe { (a.SUSceneGetCamera)(s, &mut cam) } == 0 { read_camera(a, cam) } else { None };
        let Some(camera) = camera else { continue };
        let mut use_layers = false;
        unsafe { (a.SUSceneGetUseHiddenLayers)(s, &mut use_layers) };
        let hidden_layers = use_layers.then(|| {
            list(|n| unsafe { (a.SUSceneGetNumLayers)(s, n) }, |n, p, g| unsafe { (a.SUSceneGetLayers)(s, n, p, g) })
                .into_iter()
                .filter_map(|l| r.layers.get(&(l.0 as usize)).copied())
                .collect()
        });
        let mut scene =
            esq_core::model::Scene { name, description, camera, hidden_layers, photo: None, ext: Default::default() };
        let attrs = r.read_scene_ext(s, &mut scene);
        let mut st = Ref::NULL;
        if unsafe { (a.SUSceneGetStyle)(s, &mut st) } == 0
            && let Some(n) = style_names.get(&(st.0 as usize))
        {
            scene.ext.props.insert("style".into(), serde_json::json!(n));
        }
        if active == Some(s.0 as usize) {
            r.model.view_ext.insert(esq_core::model::ACTIVE_SCENE.into(), r.model.scenes.len().into());
        }
        r.model.scenes.push(scene);
        scene_attrs.push(attrs);
    }
    r.model.ensure_scene_uids();
    for (s, attrs) in r.model.scenes.iter().zip(scene_attrs) {
        if !attrs.is_empty() {
            r.model.attr_store.insert(model_info::scene_owner(s), attrs);
        }
    }
    Ok(r.model)
}

impl Reader<'_> {
    fn material(&mut self, mat: Ref) -> Option<MaterialId> {
        if mat.is_null() {
            return None;
        }
        if let Some(&id) = self.materials.get(&(mat.0 as usize)) {
            return Some(id);
        }
        let a = self.a;
        let name = read_string(a, |s| unsafe { (a.SUMaterialGetName)(mat, s) });
        let mut c = Color::default();
        unsafe { (a.SUMaterialGetColor)(mat, &mut c) };
        let mut op = 1.0;
        unsafe { (a.SUMaterialGetOpacity)(mat, &mut op) };
        let mut opacity = op as f32;
        let ext = textures::read_material_ext(a, mat, &mut opacity);
        let id = self.model.add_material(&name, [c.r, c.g, c.b]);
        let m = &mut self.model.materials[id];
        (m.opacity, m.ext) = (opacity, ext);
        let attrs = read_attrs(a, unsafe { (a.SUMaterialToEntity)(mat) });
        if !attrs.is_empty() {
            self.model.attr_store.insert(format!("mat:{name}"), attrs);
        }
        self.materials.insert(mat.0 as usize, id);
        Some(id)
    }

    fn layer(&self, el: Ref) -> LayerIdx {
        let mut l = Ref::NULL;
        if unsafe { (self.a.SUDrawingElementGetLayer)(el, &mut l) } != 0 {
            return 0;
        }
        self.layers.get(&(l.0 as usize)).copied().unwrap_or(0)
    }

    fn hidden(&self, el: Ref) -> bool {
        let mut h = false;
        unsafe { (self.a.SUDrawingElementGetHidden)(el, &mut h) };
        h
    }

    fn el_material(&mut self, el: Ref) -> Option<MaterialId> {
        let mut mat = Ref::NULL;
        let r = unsafe { (self.a.SUDrawingElementGetMaterial)(el, &mut mat) };
        if r != 0 { None } else { self.material(mat) }
    }

    /// The font of a text or a dimension (for a dimension, none when it is the style's, which
    /// the SDK gives for one without its own) and its colour (a material SketchUp names « * »),
    /// as the native reader keeps them (`native::fonts::props`).
    fn annot_extras(&mut self, out: &mut Entities, key: &str, el: Ref, font: Ref, dim: bool) {
        let a = self.a;
        if !font.is_null() && !(dim && font.0 == self.dim_font.0) {
            let flag = |v: Option<u64>| v.is_some_and(|x| x & 0xff != 0);
            let f = native::doc::Font {
                id: 0,
                face: read_string(a, |x| unsafe { (a.SUFontGetFaceName)(font, x) }),
                bold: flag(sdk_census::get(|x| unsafe { (a.SUFontGetBold)(font, x) })),
                italic: flag(sdk_census::get(|x| unsafe { (a.SUFontGetItalic)(font, x) })),
                size: sdk_census::get(|x| unsafe { (a.SUFontGetPointSize)(font, x) }).unwrap_or(0) as u32,
                world: flag(sdk_census::get(|x| unsafe { (a.SUFontGetUseWorldSize)(font, x) })),
                height: sdk_census::get(|x| unsafe { (a.SUFontGetWorldSize)(font, x) }).map_or(0.0, f64::from_bits),
            };
            for (k, v) in native::fonts::props(&f) {
                out.set_prop(key, k, v);
            }
        }
        if let Some(m) = self.el_material(el) {
            let mt = &self.model.materials[m];
            out.set_prop(key, "material", AttrValue::Text(mt.name.clone()));
            out.set_prop(key, "color", AttrValue::Json(serde_json::json!(mt.color)));
        }
    }

    /// Attribute dictionaries, shadow flags and material of a face or an edge, kept in the
    /// side tables as the Ruby bridge keeps them.
    fn entity_extras(&mut self, out: &mut Entities, kind: char, ffi: u64, entity: Ref, el: Ref) {
        let a = self.a;
        let attrs = read_attrs(a, entity);
        if !attrs.is_empty() {
            self.pending.push((kind, ffi, attrs));
        }
        self.shadow_flags(out, kind, ffi, el);
        if kind == 'e'
            && let Some(m) = self.el_material(el)
        {
            out.set_prop(&format!("{kind}{ffi}"), "material", AttrValue::Text(self.model.materials[m].name.clone()));
        }
    }

    /// "Casts shadows" and "receives shadows" when they are off (`Drawingelement#casts_shadows=`).
    fn shadow_flags(&self, out: &mut Entities, kind: char, ffi: u64, el: Ref) {
        let a = self.a;
        let key = format!("{kind}{ffi}");
        let (mut casts, mut receives) = (true, true);
        unsafe {
            (a.SUDrawingElementGetCastsShadows)(el, &mut casts);
            (a.SUDrawingElementGetReceivesShadows)(el, &mut receives);
        }
        if !casts {
            out.set_prop(&key, "casts", AttrValue::Bool(false));
        }
        if !receives {
            out.set_prop(&key, "receives", AttrValue::Bool(false));
        }
    }

    /// What a curve of the file is: a polygon, a plain curve, or an arc with its start and
    /// end angles and x axis (`ArcCurve`).
    fn curve_props(&self, out: &mut Entities, c: Ref, id: u32) {
        let a = self.a;
        let key = format!("c{id}");
        let mut ty = 0;
        unsafe { (a.SUCurveGetType)(c, &mut ty) };
        // Measured: `SUCurveIsPolygon` answers true for every plain curve; only an arc can be
        // a polygon (`Curve#is_polygon?`).
        let mut polygon = false;
        unsafe { (a.SUCurveIsPolygon)(c, &mut polygon) };
        if ty == 1 && polygon {
            out.set_prop(&key, "kind", AttrValue::Text("polygon".into()));
        } else if ty != 1 {
            out.set_prop(&key, "kind", AttrValue::Text("curve".into()));
        } else {
            let arc = unsafe { (a.SUArcCurveFromCurve)(c) };
            let (mut start, mut end, mut x) = (0.0, 0.0, P3::default());
            unsafe {
                (a.SUArcCurveGetStartAngle)(arc, &mut start);
                (a.SUArcCurveGetEndAngle)(arc, &mut end);
                (a.SUArcCurveGetXAxis)(arc, &mut x);
            }
            let (mut c, mut n) = (P3::default(), P3::default());
            unsafe {
                (a.SUArcCurveGetCenter)(arc, &mut c);
                (a.SUArcCurveGetNormal)(arc, &mut n);
            }
            let x = DVec3::new(x.x, x.y, x.z).normalize_or_zero();
            let c = DVec3::new(c.x, c.y, c.z) * IN;
            // The centre and normal too: an arc of one or two segments cannot be found again
            // from its points.
            for (k, v) in [
                ("start", start),
                ("end", end),
                ("xx", x.x),
                ("xy", x.y),
                ("xz", x.z),
                ("cx", c.x),
                ("cy", c.y),
                ("cz", c.z),
                ("nx", n.x),
                ("ny", n.y),
                ("nz", n.z),
            ] {
                out.set_prop(&key, k, AttrValue::Number(v));
            }
        }
    }

    /// The entities of a collection, and the dictionaries of its faces and edges.
    fn read_entities(&mut self, ents: Ref) -> Result<(Entities, Pending)> {
        let outer = std::mem::take(&mut self.pending);
        let res = self.read_entities_inner(ents);
        let mine = std::mem::replace(&mut self.pending, outer);
        res.map(|e| (e, mine))
    }

    /// Puts the dictionaries of a collection's faces and edges in the side table.
    fn store_attrs(&mut self, ctx: Option<DefId>, list: Pending) {
        for (kind, ffi, attrs) in list {
            self.model.attr_store.insert(attr_owner(kind, ctx, ffi), attrs);
        }
    }

    fn read_entities_inner(&mut self, ents: Ref) -> Result<Entities> {
        use slotmap::Key;
        let a = self.a;
        let mut out = Entities::default();
        let mut curves: HashMap<usize, u32> = HashMap::new();
        let mut verts: HashMap<usize, VertexId> = HashMap::new();
        let mut vertex = |out: &mut Entities, v: Ref| -> VertexId {
            *verts.entry(v.0 as usize).or_insert_with(|| {
                let mut p = P3::default();
                unsafe { (a.SUVertexGetPosition)(v, &mut p) };
                out.add_vertex_raw(DVec3::new(p.x, p.y, p.z) * IN)
            })
        };

        let edges = list(
            |n| unsafe { (a.SUEntitiesGetNumEdges)(ents, false, n) },
            |n, p, g| unsafe { (a.SUEntitiesGetEdges)(ents, false, n, p, g) },
        );
        for e in edges {
            let (mut s, mut t) = (Ref::NULL, Ref::NULL);
            unsafe {
                (a.SUEdgeGetStartVertex)(e, &mut s);
                (a.SUEdgeGetEndVertex)(e, &mut t);
            }
            let (vs, vt) = (vertex(&mut out, s), vertex(&mut out, t));
            if let Some(id) = out.add_edge_raw(vs, vt) {
                let (mut soft, mut smooth) = (false, false);
                unsafe {
                    (a.SUEdgeGetSoft)(e, &mut soft);
                    (a.SUEdgeGetSmooth)(e, &mut smooth);
                }
                let el = unsafe { (a.SUEdgeToDrawingElement)(e) };
                let (hidden, layer) = (self.hidden(el), self.layer(el));
                let edge = &mut out.edges[id];
                edge.soft = soft;
                edge.smooth = smooth;
                edge.hidden = hidden;
                edge.layer = layer;
                let mut c = Ref::NULL;
                if unsafe { (a.SUEdgeGetCurve)(e, &mut c) } == 0 && !c.is_null() {
                    let cid = match curves.get(&(c.0 as usize)) {
                        Some(&x) => x,
                        None => {
                            let x = out.new_curve_id();
                            curves.insert(c.0 as usize, x);
                            self.curve_props(&mut out, c, x);
                            x
                        }
                    };
                    out.edges[id].curve = Some(cid);
                }
                self.entity_extras(&mut out, 'e', id.data().as_ffi(), unsafe { (a.SUEdgeToEntity)(e) }, el);
            }
        }

        let faces = list(
            |n| unsafe { (a.SUEntitiesGetNumFaces)(ents, n) },
            |n, p, g| unsafe { (a.SUEntitiesGetFaces)(ents, n, p, g) },
        );
        for f in faces {
            let read_loop = |lp: Ref, out: &mut Entities, vertex: &mut dyn FnMut(&mut Entities, Ref) -> VertexId| {
                list(
                    |n| unsafe { (a.SULoopGetNumVertices)(lp, n) },
                    |n, p, g| unsafe { (a.SULoopGetVertices)(lp, n, p, g) },
                )
                .into_iter()
                .map(|v| vertex(out, v))
                .collect::<Vec<VertexId>>()
            };
            let mut outer = Ref::NULL;
            if unsafe { (a.SUFaceGetOuterLoop)(f, &mut outer) } != 0 {
                continue;
            }
            let outer_ids = read_loop(outer, &mut out, &mut vertex);
            let holes: Vec<Vec<VertexId>> = list(
                |n| unsafe { (a.SUFaceGetNumInnerLoops)(f, n) },
                |n, p, g| unsafe { (a.SUFaceGetInnerLoops)(f, n, p, g) },
            )
            .into_iter()
            .map(|l| read_loop(l, &mut out, &mut vertex))
            .collect();
            let Some(id) = out.add_face_raw(outer_ids, holes) else { continue };
            let mut n = P3::default();
            unsafe { (a.SUFaceGetNormal)(f, &mut n) };
            let el = unsafe { (a.SUFaceToDrawingElement)(f) };
            let (mut fm, mut bm) = (Ref::NULL, Ref::NULL);
            let rf = unsafe { (a.SUFaceGetFrontMaterial)(f, &mut fm) };
            let rb = unsafe { (a.SUFaceGetBackMaterial)(f, &mut bm) };
            let front = if rf == 0 { self.material(fm) } else { None };
            let back = if rb == 0 { self.material(bm) } else { None };
            let (layer, hidden) = (self.layer(el), self.hidden(el));
            // SketchUp keeps the position of a texture even under a plain colour.
            let sides = [front.is_some(), back.is_some()];
            let face = &mut out.faces[id];
            let normal = DVec3::new(n.x, n.y, n.z);
            if normal.length() > 0.5 {
                face.normal = normal.normalize();
            }
            face.front = front;
            face.back = back;
            face.layer = layer;
            face.hidden = hidden;
            if sides[0] || sides[1] {
                let pts: Vec<DVec3> = face.outer.iter().map(|v| out.verts[*v].pos).collect();
                let tex = textures::read_face_tex(a, f, sides, &pts);
                out.faces[id].tex = tex.map(Box::new);
            }
            self.entity_extras(&mut out, 'f', id.data().as_ffi(), unsafe { (a.SUFaceToEntity)(f) }, el);
        }

        let v3 = |p: P3| DVec3::new(p.x, p.y, p.z);
        for gl in list(
            |n| unsafe { (a.SUEntitiesGetNumGuideLines)(ents, n) },
            |n, p, g| unsafe { (a.SUEntitiesGetGuideLines)(ents, n, p, g) },
        ) {
            let (mut start, mut dir, mut infinite) = (P3::default(), P3::default(), false);
            if unsafe { (a.SUGuideLineGetData)(gl, &mut start, &mut dir, &mut infinite) } != 0 {
                continue;
            }
            let pos = v3(start) * IN;
            let mut g = if infinite { Guide::line(pos, v3(dir)) } else { Guide::segment(pos, pos + v3(dir) * IN) };
            if g.dir.is_nan() || g.dir == DVec3::ZERO {
                continue;
            }
            let el = unsafe { (a.SUGuideLineToDrawingElement)(gl) };
            (g.layer, g.hidden) = (self.layer(el), self.hidden(el));
            out.guides.insert(g);
        }
        for gp in list(
            |n| unsafe { (a.SUEntitiesGetNumGuidePoints)(ents, n) },
            |n, p, g| unsafe { (a.SUEntitiesGetGuidePoints)(ents, n, p, g) },
        ) {
            let mut p = P3::default();
            if unsafe { (a.SUGuidePointGetPosition)(gp, &mut p) } != 0 {
                continue;
            }
            let mut g = Guide::point(v3(p) * IN);
            let el = unsafe { (a.SUGuidePointToDrawingElement)(gp) };
            (g.layer, g.hidden) = (self.layer(el), self.hidden(el));
            out.guides.insert(g);
        }
        out.snaps.extend(read_snaps(a, ents));
        for (mut s, el) in spaces::read_spaces(a, ents) {
            (s.layer, s.hidden) = (self.layer(el), self.hidden(el));
            out.spaces.push(s);
        }

        // Section planes. SketchUp keeps what lies on the positive side of a·x + b·y + c·z + d.
        for sp in list(
            |n| unsafe { (a.SUEntitiesGetNumSectionPlanes)(ents, n) },
            |n, p, g| unsafe { (a.SUEntitiesGetSectionPlanes)(ents, n, p, g) },
        ) {
            let mut pl = Plane3::default();
            if unsafe { (a.SUSectionPlaneGetPlane)(sp, &mut pl) } != 0 {
                continue;
            }
            let n = DVec3::new(pl.a, pl.b, pl.c);
            let len = n.length();
            if len < 1e-12 {
                continue;
            }
            let mut sec = SectionPlane::new(-n * (pl.d / (len * len)) * IN, n);
            sec.name = read_string(a, |s| unsafe { (a.SUSectionPlaneGetName)(sp, s) });
            sec.symbol = read_string(a, |s| unsafe { (a.SUSectionPlaneGetSymbol)(sp, s) });
            let el = unsafe { (a.SUSectionPlaneToDrawingElement)(sp) };
            (sec.layer, sec.hidden) = (self.layer(el), self.hidden(el));
            let mut active = false;
            unsafe { (a.SUSectionPlaneIsActive)(sp, &mut active) };
            let id = out.sections.insert(sec);
            if active {
                out.active_section = Some(id);
            }
        }

        // Texts.
        for tx in list(
            |n| unsafe { (a.SUEntitiesGetNumTexts)(ents, n) },
            |n, p, g| unsafe { (a.SUEntitiesGetTexts)(ents, n, p, g) },
        ) {
            let text = read_string(a, |s| unsafe { (a.SUTextGetString)(tx, s) });
            let (mut pt, mut path) = (P3::default(), Ref::NULL);
            let has_point = unsafe { (a.SUTextGetPoint)(tx, &mut pt, &mut path) } == 0;
            if !path.is_null() {
                unsafe { (a.SUInstancePathRelease)(&mut path) };
            }
            let mut leader_type = 0;
            unsafe { (a.SUTextGetLeaderType)(tx, &mut leader_type) };
            let mut lv = P3::default();
            unsafe { (a.SUTextGetLeaderVector)(tx, &mut lv) };
            let kind = if has_point && leader_type != 0 {
                AnnotKind::Text { point: v3(pt) * IN, leader: v3(lv) * IN }
            } else {
                let (mut x, mut y) = (0.5, 0.5);
                unsafe { (a.SUTextGetScreenPosition)(tx, &mut x, &mut y) };
                AnnotKind::ScreenText { x, y }
            };
            let el = unsafe { (a.SUTextToDrawingElement)(tx) };
            let id = out.annots.insert(Annotation {
                kind,
                text,
                layer: self.layer(el),
                hidden: self.hidden(el),
                attach: Default::default(),
            });
            let key = format!("a{}", id.data().as_ffi());
            // A leader that follows the view (1) rather than pinned to the model (2).
            if leader_type == 1 {
                out.set_prop(&key, "leader_type", AttrValue::Number(1.0));
            }
            let mut arrow = 0;
            if unsafe { (a.SUTextGetArrowType)(tx, &mut arrow) } == 0 && arrow != 0 {
                out.set_prop(&key, "arrow_type", AttrValue::Number(arrow as f64));
            }
            let mut font = Ref::NULL;
            unsafe { (a.SUTextGetFont)(tx, &mut font) };
            self.annot_extras(&mut out, &key, el, font, false);
        }
        // Linear dimensions (radial ones are not read yet).
        for dm in list(
            |n| unsafe { (a.SUEntitiesGetNumDimensions)(ents, n) },
            |n, p, g| unsafe { (a.SUEntitiesGetDimensions)(ents, n, p, g) },
        ) {
            let mut ty = 0;
            unsafe { (a.SUDimensionGetType)(dm, &mut ty) };
            if ty != 1 {
                continue;
            }
            let d = unsafe { (a.SUDimensionLinearFromDimension)(dm) };
            let (mut s, mut e, mut sp, mut ep) = (P3::default(), P3::default(), Ref::NULL, Ref::NULL);
            let ok = unsafe {
                (a.SUDimensionLinearGetStartPoint)(d, &mut s, &mut sp) == 0
                    && (a.SUDimensionLinearGetEndPoint)(d, &mut e, &mut ep) == 0
            };
            for mut p in [sp, ep] {
                if !p.is_null() {
                    unsafe { (a.SUInstancePathRelease)(&mut p) };
                }
            }
            if !ok {
                continue;
            }
            let (mut x, mut n, mut pos) = (P3::default(), P3::default(), P2::default());
            unsafe {
                (a.SUDimensionLinearGetXAxis)(d, &mut x);
                (a.SUDimensionLinearGetNormal)(d, &mut n);
                (a.SUDimensionLinearGetPosition)(d, &mut pos);
            }
            // The dimension line sits at `pos.y` along normal × x axis.
            let side = v3(n).cross(v3(x)).normalize_or_zero();
            let mut text = read_string(a, |st| unsafe { (a.SUDimensionGetText)(dm, st) });
            if text.starts_with('<') && text.ends_with('>') {
                text.clear();
            }
            let el = unsafe { (a.SUDimensionToDrawingElement)(dm) };
            let id = out.annots.insert(Annotation {
                kind: AnnotKind::Linear { a: v3(s) * IN, b: v3(e) * IN, offset: side * pos.y * IN },
                text,
                layer: self.layer(el),
                hidden: self.hidden(el),
                attach: Default::default(),
            });
            let mut arrow = 3;
            let key = format!("a{}", id.data().as_ffi());
            if unsafe { (a.SUDimensionGetArrowType)(dm, &mut arrow) } == 0 && arrow != 3 {
                out.set_prop(&key, "arrow_type", AttrValue::Number(arrow as f64));
            }
            let mut font = Ref::NULL;
            unsafe { (a.SUDimensionGetFont)(dm, &mut font) };
            self.annot_extras(&mut out, &key, el, font, true);
        }

        for g in list(
            |n| unsafe { (a.SUEntitiesGetNumGroups)(ents, n) },
            |n, p, gg| unsafe { (a.SUEntitiesGetGroups)(ents, n, p, gg) },
        ) {
            let mut sub = Ref::NULL;
            su!(a.SUGroupGetEntities(g, &mut sub))?;
            let mut x = Xf { v: [0.0; 16] };
            unsafe { (a.SUGroupGetTransform)(g, &mut x) };
            let name = read_string(a, |s| unsafe { (a.SUGroupGetName)(g, s) });
            let el = unsafe { (a.SUGroupToDrawingElement)(g) };
            let mut sdef = Ref::NULL;
            unsafe { (a.SUGroupGetDefinition)(g, &mut sdef) };
            let def = match self.defs.get(&(sdef.0 as usize)).filter(|_| !sdef.is_null()) {
                Some(&d) => d,
                None => {
                    let (ents, attrs) = self.read_entities(sub)?;
                    let def_name = (1..)
                        .map(|i| format!("Groupe#{i}"))
                        .find(|n| !self.component_names.contains(n) && !self.model.defs.values().any(|d| &d.name == n))
                        .unwrap_or_default();
                    // Its dictionaries (a group's definition has some: IFC data…).
                    let attributes = if sdef.is_null() {
                        Attributes::new()
                    } else {
                        read_attrs(a, unsafe { (a.SUComponentDefinitionToEntity)(sdef) })
                    };
                    let d = self.model.defs.insert(Definition {
                        name: def_name,
                        is_group: true,
                        description: String::new(),
                        attributes,
                        ext: Default::default(),
                        entities: ents,
                    });
                    self.store_attrs(Some(d), attrs);
                    if !sdef.is_null() {
                        self.defs.insert(sdef.0 as usize, d);
                    }
                    d
                }
            };
            let material = self.el_material(el);
            let mut locked = false;
            unsafe { (a.SUComponentInstanceIsLocked)((a.SUGroupToComponentInstance)(g), &mut locked) };
            let id = out.instances.insert(Instance {
                def,
                xform: xf_to_mm(&x),
                name,
                material,
                layer: self.layer(el),
                hidden: self.hidden(el),
                locked,
                attributes: read_attrs(a, unsafe { (a.SUGroupToEntity)(g) }),
                ext: Default::default(),
            });
            self.shadow_flags(&mut out, 'i', id.data().as_ffi(), el);
        }

        for i in list(
            |n| unsafe { (a.SUEntitiesGetNumInstances)(ents, n) },
            |n, p, g| unsafe { (a.SUEntitiesGetInstances)(ents, n, p, g) },
        ) {
            let mut sdef = Ref::NULL;
            su!(a.SUComponentInstanceGetDefinition(i, &mut sdef))?;
            let def = match self.defs.get(&(sdef.0 as usize)) {
                Some(&d) => d,
                None => {
                    let mut sub = Ref::NULL;
                    su!(a.SUComponentDefinitionGetEntities(sdef, &mut sub))?;
                    let (ents, attrs) = self.read_entities(sub)?;
                    let name = read_string(a, |s| unsafe { (a.SUComponentDefinitionGetName)(sdef, s) });
                    let description = read_string(a, |s| unsafe { (a.SUComponentDefinitionGetDescription)(sdef, s) });
                    let mut b = Behavior::default();
                    unsafe { (a.SUComponentDefinitionGetBehavior)(sdef, &mut b) };
                    let ext = esq_core::model::DefExt { behavior: behavior_of(&b), ..Default::default() };
                    let d = self.model.defs.insert(Definition {
                        name,
                        is_group: false,
                        description,
                        attributes: read_attrs(a, unsafe { (a.SUComponentDefinitionToEntity)(sdef) }),
                        ext,
                        entities: ents,
                    });
                    self.store_attrs(Some(d), attrs);
                    self.defs.insert(sdef.0 as usize, d);
                    d
                }
            };
            let mut x = Xf { v: [0.0; 16] };
            unsafe { (a.SUComponentInstanceGetTransform)(i, &mut x) };
            let name = read_string(a, |s| unsafe { (a.SUComponentInstanceGetName)(i, s) });
            let el = unsafe { (a.SUComponentInstanceToDrawingElement)(i) };
            let material = self.el_material(el);
            let mut locked = false;
            unsafe { (a.SUComponentInstanceIsLocked)(i, &mut locked) };
            let id = out.instances.insert(Instance {
                def,
                xform: xf_to_mm(&x),
                name,
                material,
                layer: self.layer(el),
                hidden: self.hidden(el),
                locked,
                attributes: read_attrs(a, unsafe { (a.SUComponentInstanceToEntity)(i) }),
                ext: Default::default(),
            });
            self.shadow_flags(&mut out, 'i', id.data().as_ffi(), el);
        }
        images::read_images(self, &mut out, ents)?;
        out.rebuild_topology();
        Ok(out)
    }
}

// ---------------------------------------------------------------------------------------
// Export

struct Writer<'a> {
    a: &'a Api,
    su_model: Ref,
    model: &'a Model,
    materials: HashMap<MaterialId, Ref>,
    layers: Vec<Ref>,
    defs: HashMap<DefId, Ref>,
    /// Images written so far (each goes through its own scratch folder).
    images: std::cell::Cell<usize>,
}

/// Saves the model as a .skp file (current SketchUp format).
pub fn export_skp(model: &Model, path: &Path) -> Result<()> {
    export_skp_view(model, path, None)
}

/// Saves the model as a .skp file, opening in SketchUp with the given point of view.
pub fn export_skp_view(model: &Model, path: &Path, view: Option<&View>) -> Result<()> {
    export_skp_version(model, path, view, None)
}

/// Saves the model as a .skp file of SketchUp version `version` (`None` = the current
/// format), as `Model#save(path, version)` does: natively (`native::write`), through
/// SketchUp's SDK only with `ESQ_SKP_SDK=1`.
pub fn export_skp_version(model: &Model, path: &Path, view: Option<&View>, version: Option<SkpVersion>) -> Result<()> {
    export_skp_with(model, path, view, version, None)
}

/// [`export_skp_version`], with the model's preview (a PNG, the picture SketchUp keeps in the
/// file for the Explorer and its dialogs).
pub fn export_skp_with(
    model: &Model,
    path: &Path,
    view: Option<&View>,
    version: Option<SkpVersion>,
    thumbnail: Option<&[u8]>,
) -> Result<()> {
    if sdk_reader_forced() && available() {
        return sdk_export_skp_version(model, path, view, version);
    }
    let extras = native::write::Extras { thumbnail: thumbnail.map(<[u8]>::to_vec) };
    let bytes = native::write::write(model, view, version, &extras)
        .map_err(|e| anyhow!("{}", e.0))
        .with_context(|| format!("enregistrement de {}", path.display()))?;
    write_replacing(path, &bytes).with_context(|| format!("enregistrement de {}", path.display()))
}

/// Writes a file whole: into a sibling first, then in place of the file (a failure leaves
/// the old file as it was).
fn write_replacing(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "modele.skp".into());
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Saves the model through SketchUp's SDK (`SketchUpAPI.dll`): the reference the native
/// writer is tested against.
pub fn sdk_export_skp_version(
    model: &Model,
    path: &Path,
    view: Option<&View>,
    version: Option<SkpVersion>,
) -> Result<()> {
    let a = api()?;
    let _one_at_a_time = CALLS.lock().unwrap_or_else(|e| e.into_inner());
    let mut m = Ref::NULL;
    su!(a.SUModelCreate(&mut m))?;
    let res = (|| {
        if let Some(v) = view {
            write_view(a, m, v)?;
        }
        write_model(a, m, model, path, version)
    })();
    unsafe { (a.SUModelRelease)(&mut m) };
    res
}

/// The version a .skp file was written by, from its header (`{21.0.339}` → (21, 0, 339)).
pub fn file_version(path: &Path) -> Option<(u32, u32, u32)> {
    let mut head = vec![0u8; 256];
    let n = std::io::Read::read(&mut std::fs::File::open(path).ok()?, &mut head).ok()?;
    let words: Vec<u16> = head[..n].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let text = String::from_utf16_lossy(&words);
    let open = text.find('{')?;
    let close = open + text[open..].find('}')?;
    let mut it = text[open + 1..close].split('.').map(|x| x.trim().parse::<u32>().ok());
    Some((it.next()??, it.next().flatten().unwrap_or(0), it.next().flatten().unwrap_or(0)))
}

fn write_view(a: &Api, m: Ref, v: &View) -> Result<()> {
    let mut cam = Ref::NULL;
    su!(a.SUModelGetCamera(m, &mut cam))?;
    write_camera(a, cam, v)
}

fn write_camera(a: &Api, cam: Ref, v: &View) -> Result<()> {
    let p = |x: DVec3, s: f64| P3 { x: x.x / s, y: x.y / s, z: x.z / s };
    let (e, t, u) = (p(v.eye, IN), p(v.target, IN), p(v.up, 1.0));
    su!(a.SUCameraSetOrientation(cam, &e, &t, &u))?;
    su!(a.SUCameraSetPerspective(cam, v.perspective))?;
    if v.perspective {
        su!(a.SUCameraSetPerspectiveFrustumFOV(cam, v.fov_deg))?;
    }
    Ok(())
}

fn write_model(a: &Api, m: Ref, model: &Model, path: &Path, version: Option<SkpVersion>) -> Result<()> {
    let mut w = Writer {
        a,
        su_model: m,
        model,
        materials: HashMap::new(),
        layers: Vec::new(),
        defs: HashMap::new(),
        images: std::cell::Cell::new(0),
    };
    w.write_model_info(m)?;
    w.set_units()?;
    write_shadow(a, m, &model.shadow)?;

    for (id, mat) in model.materials.iter() {
        // An image's material is made with its image.
        if mat.ext.owner_image {
            continue;
        }
        let mut r = Ref::NULL;
        su!(a.SUMaterialCreate(&mut r))?;
        let name = cstr(&mat.name);
        su!(a.SUMaterialSetName(r, name.as_ptr()))?;
        textures::write_material_ext(a, r, &mat.ext, mat.opacity)?;
        // The colour of a textured material colorizes it: its type is given again after.
        let c = Color { r: mat.color[0], g: mat.color[1], b: mat.color[2], a: 255 };
        su!(a.SUMaterialSetColor(r, &c))?;
        su!(a.SUMaterialSetOpacity(r, mat.opacity as f64))?;
        textures::write_material_type(a, r, &mat.ext)?;
        su!(a.SUModelAddMaterials(m, 1, &r))?;
        if let Some(attrs) = model.attr_store.get(&format!("mat:{}", mat.name)) {
            write_attrs(a, unsafe { (a.SUMaterialToEntity)(r) }, attrs)?;
        }
        w.materials.insert(id, r);
    }

    w.write_layers(m)?;

    let mut ents = Ref::NULL;
    su!(a.SUModelGetEntities(m, &mut ents))?;
    w.write_entities(ents, &model.root, None)?;
    let envs = write_environments(a, m, model)?;

    for sc in &model.scenes {
        let mut s = Ref::NULL;
        su!(a.SUSceneCreate(&mut s))?;
        let name = cstr(&sc.name);
        su!(a.SUSceneSetName(s, name.as_ptr()))?;
        su!(a.SUModelAddScenes(m, 1, &s))?;
        if !sc.description.is_empty() {
            let d = cstr(&sc.description);
            su!(a.SUSceneSetDescription(s, d.as_ptr()))?;
        }
        let mut cam = Ref::NULL;
        su!(a.SUCameraCreate(&mut cam))?;
        let res = write_camera(a, cam, &sc.camera).and_then(|_| su!(a.SUSceneSetCamera(s, cam)));
        unsafe { (a.SUCameraRelease)(&mut cam) };
        res?;
        su!(a.SUSceneSetUseCamera(s, true))?;
        if let Some(hidden) = &sc.hidden_layers {
            su!(a.SUSceneSetUseHiddenLayers(s, true))?;
            for &l in hidden {
                if l != 0 && l < w.layers.len() {
                    su!(a.SUSceneAddLayer(s, w.layers[l]))?;
                }
            }
        }
        w.write_scene_ext(s, sc)?;
        // The environment the scene keeps (`Page#environment`, by name).
        if let Some(e) = sc.ext.props.get("env").and_then(serde_json::Value::as_str).and_then(|n| envs.get(n)) {
            su!(a.SUSceneSetEnvironment(s, *e))?;
            su!(a.SUSceneSetUseEnvironment(s, true))?;
        }
    }

    let p = cstr(&path.to_string_lossy());
    match version {
        None => su!(a.SUModelSaveToFile(m, p.as_ptr())),
        Some(v) => su!(a.SUModelSaveToFileWithVersion(m, p.as_ptr(), v.raw())),
    }
    .with_context(|| format!("enregistrement de {}", path.display()))
}

/// The model's environments (`view_ext["envs"]`, their files in `Model::files["env:<id>"]`), as
/// the SDK makes one from a file (`SUEnvironmentCreateFromFile`: the file is written in the
/// temporary folder, then removed), with their settings; the current one selected. By name.
fn write_environments(a: &Api, m: Ref, model: &Model) -> Result<HashMap<String, Ref>> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut out = HashMap::new();
    let Some(list) = model.view_ext.get("envs").and_then(serde_json::Value::as_array) else { return Ok(out) };
    let mut envs = Ref::NULL;
    su!(a.SUModelGetEnvironments(m, &mut envs))?;
    let current = model.view_ext.get("env_current").and_then(serde_json::Value::as_i64);
    for (k, e) in list.iter().enumerate() {
        let Some(id) = e["id"].as_i64() else { continue };
        let Some(file) = model.files.get(&format!("env:{id}")) else { continue };
        let b = &file.0;
        let ext = if b.starts_with(b"PK\x03\x04") {
            "ske"
        } else if b.starts_with(&[0x76, 0x2f, 0x31, 0x01]) {
            "exr"
        } else {
            "hdr"
        };
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = std::env::temp_dir().join(format!("esq-env-{}-{seq}-{k}.{ext}", std::process::id()));
        std::fs::write(&tmp, b.as_slice())?;
        let name = e["name"].as_str().unwrap_or("Environnement");
        let (p, n) = (cstr(&tmp.to_string_lossy()), cstr(name));
        let mut r = Ref::NULL;
        let res = su!(a.SUEnvironmentCreateFromFile(&mut r, p.as_ptr(), n.as_ptr()));
        let _ = std::fs::remove_file(&tmp);
        res?;
        let num = |k: &str, d: f64| e[k].as_f64().unwrap_or(d);
        let flag = |k: &str| e[k].as_bool().unwrap_or(false);
        let d = cstr(e["description"].as_str().unwrap_or_default());
        unsafe {
            (a.SUEnvironmentSetDescription)(r, d.as_ptr());
            (a.SUEnvironmentSetRotation)(r, num("rotation", 0.0));
            (a.SUEnvironmentSetSkydomeExposure)(r, num("skydome_exposure", 1.0));
            (a.SUEnvironmentSetReflectionExposure)(r, num("reflection_exposure", 1.0));
            (a.SUEnvironmentSetUseAsSkydome)(r, flag("skydome"));
            (a.SUEnvironmentSetUseForReflections)(r, flag("reflections"));
            (a.SUEnvironmentSetLinkedSun)(r, flag("linked_sun"));
        }
        su!(a.SUEnvironmentsAdd(envs, r))?;
        if current == Some(id) {
            su!(a.SUEnvironmentsSetSelectedEnvironment(envs, r))?;
        }
        out.insert(name.to_string(), r);
    }
    Ok(out)
}

/// The SketchUp versions a model can be saved for: the values of `SUModelVersion`
/// (`Model#save(path, Sketchup::Model::VERSION_…)`). Measured with the SDK: 0 to 5 write
/// SketchUp 3 to 8 files, 6 to 13 SketchUp 2013 to 2020, 14 the current format (unchanged
/// since 2021, which every later version opens). SketchUpAPI.dll refuses any other value
/// (`SUModelSaveToFileWithVersion`: `version < 15`); SketchUp 2026's own Save As dialog offers
/// the current format only (its type list holds "SketchUp Models (*.skp)" alone), the older
/// ones are reached from Ruby.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum SkpVersion {
    Su3 = 0,
    Su4,
    Su5,
    Su6,
    Su7,
    Su8,
    Su2013,
    Su2014,
    Su2015,
    Su2016,
    Su2017,
    Su2018,
    Su2019,
    Su2020,
    Su2021,
}

impl SkpVersion {
    /// Newest first, as in a "Save as type" list.
    pub const ALL: [SkpVersion; 15] = [
        SkpVersion::Su2021,
        SkpVersion::Su2020,
        SkpVersion::Su2019,
        SkpVersion::Su2018,
        SkpVersion::Su2017,
        SkpVersion::Su2016,
        SkpVersion::Su2015,
        SkpVersion::Su2014,
        SkpVersion::Su2013,
        SkpVersion::Su8,
        SkpVersion::Su7,
        SkpVersion::Su6,
        SkpVersion::Su5,
        SkpVersion::Su4,
        SkpVersion::Su3,
    ];

    /// The major version written in the file's header (3 … 8, 13 … 20; 2021 writes the
    /// version of the SketchUp that saved it).
    pub fn major(self) -> u32 {
        match self as i32 {
            r @ 0..=5 => r as u32 + 3,
            r => r as u32 + 7,
        }
    }

    /// "2020", "8"…
    pub fn name(self) -> String {
        let m = self.major();
        if m >= 13 { format!("20{m}") } else { m.to_string() }
    }

    fn raw(self) -> i32 {
        self as i32
    }

    /// The version a `Sketchup::Model::VERSION_…` constant names (`VERSION_3` = 3 …
    /// `VERSION_2021` = 17, as SketchUp.exe defines them).
    pub fn from_ruby(v: i64) -> Option<SkpVersion> {
        SkpVersion::ALL.into_iter().find(|x| x.raw() as i64 + 3 == v)
    }
}

impl Writer<'_> {
    fn set_units(&self) -> Result<()> {
        let a = self.a;
        let unit = self.model.units.code() as i32;
        let mut om = Ref::NULL;
        su!(a.SUModelGetOptionsManager(self.su_model, &mut om))?;
        let mut prov = Ref::NULL;
        let name = cstr("UnitsOptions");
        su!(a.SUOptionsManagerGetOptionsProviderByName(om, name.as_ptr(), &mut prov))?;
        // Decimals shown, as in SketchUp's templates: none in millimetres, more for larger units.
        let mut precision = match self.model.units {
            Units::Millimeters => 0,
            Units::Centimeters => 1,
            Units::Meters => 2,
            Units::Inches | Units::Feet | Units::Yards => 3,
        };
        let mut format = 0;
        // Model Info › Units as the model has them, when they are for its unit.
        let o = self.model.meta.options.get("UnitsOptions");
        let int = |k: &str| o.and_then(|o| o.get(k)).and_then(|v| v.as_i64()).map(|v| v as i32);
        if int("LengthUnit") == Some(unit) {
            format = int("LengthFormat").unwrap_or(format);
            precision = int("LengthPrecision").unwrap_or(precision);
        }
        for (key, val) in [("LengthFormat", format), ("LengthUnit", unit), ("LengthPrecision", precision)] {
            let mut tv = Ref::NULL;
            su!(a.SUTypedValueCreate(&mut tv))?;
            su!(a.SUTypedValueSetInt32(tv, val))?;
            let k = cstr(key);
            let r = unsafe { (a.SUOptionsProviderSetValue)(prov, k.as_ptr(), tv) };
            unsafe { (a.SUTypedValueRelease)(&mut tv) };
            check(r, key)?;
        }
        Ok(())
    }

    /// Texts need a font to be saved; a new model has a default one.
    fn set_font(&self, text: Ref) -> Result<()> {
        let a = self.a;
        let m = self.su_model;
        let fonts =
            list(|n| unsafe { (a.SUModelGetNumFonts)(m, n) }, |n, p, g| unsafe { (a.SUModelGetFonts)(m, n, p, g) });
        match fonts.first() {
            Some(&f) => su!(a.SUTextSetFont(text, f)),
            None => Ok(()),
        }
    }

    fn layer(&self, l: LayerIdx) -> Ref {
        self.layers.get(l).copied().unwrap_or(self.layers[0])
    }

    fn write_entities(&mut self, target: Ref, ents: &Entities, ctx: Option<DefId>) -> Result<()> {
        let a = self.a;
        if !ents.edges.is_empty() || !ents.faces.is_empty() {
            let mut gi = Ref::NULL;
            su!(a.SUGeometryInputCreate(&mut gi))?;
            let res = self.fill(gi, target, ents, ctx);
            unsafe { (a.SUGeometryInputRelease)(&mut gi) };
            res?;
            self.geometry_attrs(target, ents, ctx)?;
        }
        for (aid, an) in ents.annots.iter() {
            let akey = {
                use slotmap::Key;
                format!("a{}", aid.data().as_ffi())
            };
            let num = |k: &str| match ents.prop(&akey, k) {
                Some(AttrValue::Number(x)) => Some(*x as i32),
                _ => None,
            };
            let p = |x: DVec3| P3 { x: x.x / IN, y: x.y / IN, z: x.z / IN };
            let v = |x: DVec3| P3 { x: x.x, y: x.y, z: x.z };
            let el = match &an.kind {
                AnnotKind::Linear { a: s, b: e, offset } => {
                    let mut d = Ref::NULL;
                    su!(a.SUDimensionLinearCreate(&mut d, &p(*s), Ref::NULL, &p(*e), Ref::NULL, offset.length() / IN))?;
                    let x = (*e - *s).normalize_or_zero();
                    let side = offset.normalize_or_zero();
                    let n = if side == DVec3::ZERO { esq_core::math::basis_for(x).0 } else { x.cross(side) };
                    su!(a.SUDimensionLinearSetXAxis(d, &v(x)))?;
                    su!(a.SUDimensionLinearSetNormal(d, &v(n)))?;
                    let pos = P2 { x: 0.0, y: offset.length() / IN };
                    su!(a.SUDimensionLinearSetPosition(d, &pos))?;
                    let dm = unsafe { (a.SUDimensionLinearToDimension)(d) };
                    if !an.text.is_empty() {
                        let t = cstr(&an.text);
                        su!(a.SUDimensionSetText(dm, t.as_ptr()))?;
                    }
                    su!(a.SUEntitiesAddDimensions(target, 1, &dm))?;
                    if let Some(t) = num("arrow_type") {
                        su!(a.SUDimensionSetArrowType(dm, t))?;
                    }
                    unsafe { (a.SUDimensionToDrawingElement)(dm) }
                }
                AnnotKind::Text { point, leader } => {
                    let mut tx = Ref::NULL;
                    su!(a.SUTextCreate(&mut tx))?;
                    self.set_font(tx)?;
                    let s = cstr(&an.text);
                    su!(a.SUTextSetString(tx, s.as_ptr()))?;
                    su!(a.SUEntitiesAddTexts(target, 1, &tx))?;
                    su!(a.SUTextSetLeaderType(tx, num("leader_type").filter(|t| *t == 1).unwrap_or(2)))?;
                    if let Some(t) = num("arrow_type") {
                        su!(a.SUTextSetArrowType(tx, t))?;
                    }
                    su!(a.SUTextSetPoint(tx, &p(*point), Ref::NULL))?;
                    su!(a.SUTextSetLeaderVector(tx, &p(*leader)))?;
                    unsafe { (a.SUTextToDrawingElement)(tx) }
                }
                AnnotKind::ScreenText { x, y } => {
                    let mut tx = Ref::NULL;
                    su!(a.SUTextCreate(&mut tx))?;
                    self.set_font(tx)?;
                    let s = cstr(&an.text);
                    su!(a.SUTextSetString(tx, s.as_ptr()))?;
                    su!(a.SUTextSetLeaderType(tx, 0))?;
                    su!(a.SUEntitiesAddTexts(target, 1, &tx))?;
                    su!(a.SUTextSetScreenPosition(tx, x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)))?;
                    unsafe { (a.SUTextToDrawingElement)(tx) }
                }
            };
            if an.layer != 0 {
                su!(a.SUDrawingElementSetLayer(el, self.layer(an.layer)))?;
            }
            if an.hidden {
                su!(a.SUDrawingElementSetHidden(el, true))?;
            }
        }
        for (id, sec) in ents.sections.iter() {
            let mut r = Ref::NULL;
            su!(a.SUSectionPlaneCreate(&mut r))?;
            let n = sec.normal;
            let pl = Plane3 { a: n.x, b: n.y, c: n.z, d: -n.dot(sec.pos) / IN };
            su!(a.SUSectionPlaneSetPlane(r, &pl))?;
            let (name, symbol) = (cstr(&sec.name), cstr(&sec.symbol));
            su!(a.SUSectionPlaneSetName(r, name.as_ptr()))?;
            su!(a.SUSectionPlaneSetSymbol(r, symbol.as_ptr()))?;
            su!(a.SUEntitiesAddSectionPlanes(target, 1, &r))?;
            if ents.active_section == Some(id) {
                su!(a.SUEntitiesSetActiveSectionPlane(target, r))?;
            }
            let el = unsafe { (a.SUSectionPlaneToDrawingElement)(r) };
            if sec.layer != 0 {
                su!(a.SUDrawingElementSetLayer(el, self.layer(sec.layer)))?;
            }
            if sec.hidden {
                su!(a.SUDrawingElementSetHidden(el, true))?;
            }
        }
        for (_, g) in ents.guides.iter() {
            let p = |x: DVec3| P3 { x: x.x / IN, y: x.y / IN, z: x.z / IN };
            let mut r = Ref::NULL;
            let el = if g.is_point() {
                su!(a.SUGuidePointCreate(&mut r, &p(g.pos)))?;
                su!(a.SUEntitiesAddGuidePoints(target, 1, &r))?;
                unsafe { (a.SUGuidePointToDrawingElement)(r) }
            } else {
                if g.infinite {
                    let d = P3 { x: g.dir.x, y: g.dir.y, z: g.dir.z };
                    su!(a.SUGuideLineCreateInfinite(&mut r, &p(g.pos), &d))?;
                } else {
                    su!(a.SUGuideLineCreateFinite(&mut r, &p(g.pos), &p(g.pos + g.dir)))?;
                }
                su!(a.SUEntitiesAddGuideLines(target, 1, &r))?;
                unsafe { (a.SUGuideLineToDrawingElement)(r) }
            };
            if g.layer != 0 {
                su!(a.SUDrawingElementSetLayer(el, self.layer(g.layer)))?;
            }
            if g.hidden {
                su!(a.SUDrawingElementSetHidden(el, true))?;
            }
        }
        for (iid, inst) in ents.instances.iter() {
            let def = &self.model.defs[inst.def];
            let xf = xf_from_mm(&inst.xform);

            let el = if def.ext.image.is_some() {
                match images::write_image(self, target, inst)? {
                    Some(el) => el,
                    None => continue,
                }
            } else if def.is_group {
                let mut g = Ref::NULL;
                su!(a.SUGroupCreate(&mut g))?;
                su!(a.SUEntitiesAddGroup(target, g))?;
                su!(a.SUGroupSetTransform(g, &xf))?;
                if !inst.name.is_empty() {
                    let n = cstr(&inst.name);
                    su!(a.SUGroupSetName(g, n.as_ptr()))?;
                }
                let mut sub = Ref::NULL;
                su!(a.SUGroupGetEntities(g, &mut sub))?;
                self.write_entities(sub, &def.entities, Some(inst.def))?;
                if inst.locked {
                    let gi = unsafe { (a.SUGroupToComponentInstance)(g) };
                    su!(a.SUComponentInstanceSetLocked(gi, true))?;
                }
                write_attrs(a, unsafe { (a.SUGroupToEntity)(g) }, &inst.attributes)?;
                unsafe { (a.SUGroupToDrawingElement)(g) }
            } else {
                let sdef = self.definition(inst.def)?;
                let mut i = Ref::NULL;
                su!(a.SUComponentDefinitionCreateInstance(sdef, &mut i))?;
                su!(a.SUComponentInstanceSetTransform(i, &xf))?;
                if !inst.name.is_empty() {
                    let n = cstr(&inst.name);
                    su!(a.SUComponentInstanceSetName(i, n.as_ptr()))?;
                }
                su!(a.SUEntitiesAddInstance(target, i, std::ptr::null_mut()))?;
                if inst.locked {
                    su!(a.SUComponentInstanceSetLocked(i, true))?;
                }
                write_attrs(a, unsafe { (a.SUComponentInstanceToEntity)(i) }, &inst.attributes)?;
                unsafe { (a.SUComponentInstanceToDrawingElement)(i) }
            };
            if let Some(&mr) = inst.material.and_then(|m| self.materials.get(&m)) {
                su!(a.SUDrawingElementSetMaterial(el, mr))?;
            }
            if inst.layer != 0 {
                su!(a.SUDrawingElementSetLayer(el, self.layer(inst.layer)))?;
            }
            if inst.hidden {
                su!(a.SUDrawingElementSetHidden(el, true))?;
            }
            let key = {
                use slotmap::Key;
                format!("i{}", iid.data().as_ffi())
            };
            if matches!(ents.prop(&key, "casts"), Some(AttrValue::Bool(false))) {
                su!(a.SUDrawingElementSetCastsShadows(el, false))?;
            }
            if matches!(ents.prop(&key, "receives"), Some(AttrValue::Bool(false))) {
                su!(a.SUDrawingElementSetReceivesShadows(el, false))?;
            }
        }
        Ok(())
    }

    fn definition(&mut self, d: DefId) -> Result<Ref> {
        if let Some(&r) = self.defs.get(&d) {
            return Ok(r);
        }
        let a = self.a;
        let def = &self.model.defs[d];
        let mut r = Ref::NULL;
        su!(a.SUComponentDefinitionCreate(&mut r))?;
        let n = cstr(&def.name);
        su!(a.SUComponentDefinitionSetName(r, n.as_ptr()))?;
        su!(a.SUModelAddComponentDefinitions(self.su_model, 1, &r))?;
        if !def.description.is_empty() {
            let t = cstr(&def.description);
            su!(a.SUComponentDefinitionSetDescription(r, t.as_ptr()))?;
        }
        su!(a.SUComponentDefinitionSetBehavior(r, &behavior_to(&def.ext.behavior)))?;
        self.defs.insert(d, r);
        let mut sub = Ref::NULL;
        su!(a.SUComponentDefinitionGetEntities(r, &mut sub))?;
        let ents = &self.model.defs[d].entities;
        self.write_entities(sub, ents, Some(d))?;
        write_attrs(a, unsafe { (a.SUComponentDefinitionToEntity)(r) }, &self.model.defs[d].attributes)?;
        Ok(r)
    }

    /// What an edge's properties ask of the SDK: its material (Ruby's `material` property)
    /// and whether it casts and receives shadows.
    fn edge_extras(&self, ents: &Entities, e: EdgeId) -> (Option<Ref>, bool, bool) {
        use slotmap::Key;
        let key = format!("e{}", e.data().as_ffi());
        let mat = match ents.prop(&key, "material") {
            Some(AttrValue::Text(n)) => self.model.find_material(n).and_then(|m| self.materials.get(&m).copied()),
            _ => None,
        };
        let flag = |k: &str| !matches!(ents.prop(&key, k), Some(AttrValue::Bool(false)));
        (mat, flag("casts"), flag("receives"))
    }

    /// Sets the properties of edge `ei` of the geometry input.
    fn edge_input(&self, gi: Ref, ei: usize, ents: &Entities, e: EdgeId) -> Result<()> {
        let a = self.a;
        let edge = &ents.edges[e];
        if edge.soft {
            su!(a.SUGeometryInputEdgeSetSoft(gi, ei, true))?;
        }
        if edge.smooth {
            su!(a.SUGeometryInputEdgeSetSmooth(gi, ei, true))?;
        }
        if edge.hidden {
            su!(a.SUGeometryInputEdgeSetHidden(gi, ei, true))?;
        }
        if edge.layer != 0 {
            su!(a.SUGeometryInputEdgeSetLayer(gi, ei, self.layer(edge.layer)))?;
        }
        let (mat, casts, receives) = self.edge_extras(ents, e);
        if let Some(m) = mat {
            su!(a.SUGeometryInputEdgeSetMaterial(gi, ei, m))?;
        }
        if !casts {
            su!(a.SUGeometryInputEdgeSetCastsShadow(gi, ei, false))?;
        }
        if !receives {
            su!(a.SUGeometryInputEdgeSetReceivesShadow(gi, ei, false))?;
        }
        Ok(())
    }

    fn fill(&self, gi: Ref, target: Ref, ents: &Entities, ctx: Option<DefId>) -> Result<()> {
        use esq_core::curves::{arc_fit, chain_vertices, chains, curve_edges};
        use slotmap::Key;
        let a = self.a;
        let mut index: HashMap<VertexId, usize> = HashMap::new();
        let mut pts: Vec<P3> = Vec::with_capacity(ents.verts.len());
        for (id, v) in ents.verts.iter() {
            index.insert(id, pts.len());
            pts.push(P3 { x: v.pos.x / IN, y: v.pos.y / IN, z: v.pos.z / IN });
        }
        su!(a.SUGeometryInputSetVertices(gi, pts.len(), pts.as_ptr()))?;
        let mut count = pts.len();

        // Curves. Arcs are made by the SDK (`SUGeometryInputAddArcCurve`, which adds their
        // inner points); the faces then go through those points. Other curves are joined
        // where their edges are added.
        let mut ids: Vec<u32> = ents.edges.values().filter_map(|e| e.curve).collect();
        ids.sort_unstable();
        ids.dedup();
        let mut remap: HashMap<VertexId, usize> = HashMap::new();
        let mut arc_edges: HashSet<EdgeId> = HashSet::new();
        let mut simple: Vec<Vec<EdgeId>> = Vec::new();
        for id in ids {
            let edges = curve_edges(ents, id);
            let ch = chains(ents, &edges);
            if ch.len() != 1 {
                continue;
            }
            let key = format!("c{id}");
            let num = |k: &str| match ents.prop(&key, k) {
                Some(AttrValue::Number(x)) => Some(*x),
                _ => None,
            };
            let (vs, closed) = chain_vertices(ents, &ch[0]);
            let pts: Vec<DVec3> = vs.iter().map(|v| ents.verts[*v].pos).collect();
            // The arc as the file had it (centre, normal, x axis), if the points still lie on it;
            // else as the points make one.
            let stored = match (num("cx"), num("cy"), num("cz"), num("nx"), num("ny"), num("nz")) {
                (Some(cx), Some(cy), Some(cz), Some(nx), Some(ny), Some(nz)) => {
                    let (c, n) = (DVec3::new(cx, cy, cz), DVec3::new(nx, ny, nz).normalize_or_zero());
                    let r = pts[0].distance(c);
                    let x = DVec3::new(num("xx").unwrap_or(0.0), num("xy").unwrap_or(0.0), num("xz").unwrap_or(0.0));
                    let tol = 1e-3f64.max(r * 1e-6);
                    (n != DVec3::ZERO
                        && pts.iter().all(|p| (p.distance(c) - r).abs() < tol && (*p - c).dot(n).abs() < tol))
                    .then_some((c, n, x, r))
                }
                _ => None,
            };
            let arc = match ents.prop(&key, "kind") {
                Some(AttrValue::Text(k)) if k == "curve" => None,
                _ => stored.or_else(|| arc_fit(ents, id).map(|f| (f.center, f.normal, f.xaxis, f.radius))),
            };
            let Some((center, normal, xaxis, radius)) = arc else {
                simple.push(ch[0].clone());
                continue;
            };
            // The SDK goes counter-clockwise around the normal from the start point: start at
            // the end the x axis points to (or turn the points the right way round).
            let mut vs = vs;
            if closed {
                if (pts[0] - center).cross(pts[1] - center).dot(normal) < 0.0 {
                    vs.reverse();
                    vs.rotate_right(1);
                }
                if xaxis != DVec3::ZERO {
                    let want = center + xaxis.normalize() * radius;
                    let k = (0..vs.len())
                        .min_by(|&i, &j| {
                            ents.verts[vs[i]].pos.distance(want).total_cmp(&ents.verts[vs[j]].pos.distance(want))
                        })
                        .unwrap_or(0);
                    vs.rotate_left(k);
                }
            } else {
                let want = if xaxis != DVec3::ZERO { Some(center + xaxis.normalize() * radius) } else { None };
                let reverse = match want {
                    Some(w) => pts[pts.len() - 1].distance(w) < pts[0].distance(w),
                    None => (pts[0] - center).cross(pts[1] - center).dot(normal) < 0.0,
                };
                if reverse {
                    vs.reverse();
                }
            }
            let n = ch[0].len();
            let last = if closed { vs[0] } else { vs[vs.len() - 1] };
            let c = P3 { x: center.x / IN, y: center.y / IN, z: center.z / IN };
            let nrm = P3 { x: normal.x, y: normal.y, z: normal.z };
            let (mut ci, mut first) = (0, 0);
            su!(a.SUGeometryInputAddArcCurve(gi, index[&vs[0]], index[&last], &c, &nrm, n, &mut ci, &mut first))?;
            for (k, v) in vs.iter().enumerate().skip(1) {
                if closed || k < vs.len() - 1 {
                    remap.insert(*v, count + k - 1);
                }
            }
            count += n - 1;
            for k in 0..n {
                if let Some(e) = ents.find_edge(vs[k], vs[(k + 1) % vs.len()]) {
                    self.edge_input(gi, first + k, ents, e)?;
                    arc_edges.insert(e);
                }
            }
        }
        let vi = |v: &VertexId| remap.get(v).copied().unwrap_or(index[v]);

        let mat_input = |m: Option<MaterialId>| {
            m.and_then(|m| self.materials.get(&m)).map(|&r| MaterialInput {
                num_uv_coords: 0,
                uv_coords: [P2::default(); 4],
                vertex_indices: [0; 4],
                material: r,
            })
        };
        for (fid, face) in ents.faces.iter() {
            let loop_input = |l: &[VertexId]| -> Result<Ref> {
                let m = l.len();
                let edges: Vec<Option<EdgeId>> = (0..m).map(|k| ents.find_edge(l[k], l[(k + 1) % m])).collect();
                let mut li = Ref::NULL;
                su!(a.SULoopInputCreate(&mut li))?;
                for v in l {
                    su!(a.SULoopInputAddVertexIndex(li, vi(v)))?;
                }
                for (j, e) in edges.iter().enumerate() {
                    let Some(e) = *e else { continue };
                    let edge = &ents.edges[e];
                    if edge.soft {
                        su!(a.SULoopInputEdgeSetSoft(li, j, true))?;
                    }
                    if edge.smooth {
                        su!(a.SULoopInputEdgeSetSmooth(li, j, true))?;
                    }
                    if edge.hidden {
                        su!(a.SULoopInputEdgeSetHidden(li, j, true))?;
                    }
                    if edge.layer != 0 {
                        su!(a.SULoopInputEdgeSetLayer(li, j, self.layer(edge.layer)))?;
                    }
                    let (mat, casts, receives) = self.edge_extras(ents, e);
                    if let Some(mr) = mat {
                        su!(a.SULoopInputEdgeSetMaterial(li, j, mr))?;
                    }
                    if !casts {
                        su!(a.SULoopInputEdgeSetCastsShadow(li, j, false))?;
                    }
                    if !receives {
                        su!(a.SULoopInputEdgeSetReceivesShadow(li, j, false))?;
                    }
                }
                Ok(li)
            };
            let mut li = loop_input(&face.outer)?;
            let mut fi = 0;
            su!(a.SUGeometryInputAddFace(gi, &mut li, &mut fi))?;
            for h in &face.holes {
                let mut hi = loop_input(h)?;
                su!(a.SUGeometryInputFaceAddInnerLoop(gi, fi, &mut hi))?;
            }
            let fpts: Vec<DVec3> = face.outer.iter().map(|v| ents.verts[*v].pos).collect();
            // A face that gets dictionaries is positioned after them (measured: dictionaries
            // added to a face positioned through the geometry input are not kept).
            let later = self.model.attr_store.contains_key(&attr_owner('f', ctx, fid.data().as_ffi()));
            for (side, m) in [face.front, face.back].into_iter().enumerate() {
                let Some(&mr) = m.and_then(|m| self.materials.get(&m)) else { continue };
                let positioned = if later {
                    None
                } else {
                    textures::position_input(mr, face.tex.as_deref(), side, &fpts, face.normal)
                };
                match (side, positioned) {
                    (0, Some(p)) => su!(a.SUGeometryInputFaceSetFrontMaterialByPosition(gi, fi, &p))?,
                    (_, Some(p)) => su!(a.SUGeometryInputFaceSetBackMaterialByPosition(gi, fi, &p))?,
                    (0, None) => su!(a.SUGeometryInputFaceSetFrontMaterial(gi, fi, &mat_input(m).unwrap()))?,
                    (_, None) => su!(a.SUGeometryInputFaceSetBackMaterial(gi, fi, &mat_input(m).unwrap()))?,
                }
            }
            if face.layer != 0 {
                su!(a.SUGeometryInputFaceSetLayer(gi, fi, self.layer(face.layer)))?;
            }
            if face.hidden {
                su!(a.SUGeometryInputFaceSetHidden(gi, fi, true))?;
            }
            let fkey = format!("f{}", fid.data().as_ffi());
            if matches!(ents.prop(&fkey, "casts"), Some(AttrValue::Bool(false))) {
                su!(a.SUGeometryInputFaceSetCastsShadow(gi, fi, false))?;
            }
            if matches!(ents.prop(&fkey, "receives"), Some(AttrValue::Bool(false))) {
                su!(a.SUGeometryInputFaceSetReceivesShadow(gi, fi, false))?;
            }
        }
        // Edges without faces, and the edges of the other curves (those of faces too: the
        // SDK merges them with the faces' edges, which then make the curve).
        let in_curve: HashSet<EdgeId> = simple.iter().flatten().copied().collect();
        let mut added: HashMap<EdgeId, usize> = HashMap::new();
        for (id, edge) in ents.edges.iter() {
            if (!edge.faces.is_empty() && !in_curve.contains(&id)) || arc_edges.contains(&id) {
                continue;
            }
            let mut ei = 0;
            su!(a.SUGeometryInputAddEdge(gi, vi(&edge.v[0]), vi(&edge.v[1]), &mut ei))?;
            self.edge_input(gi, ei, ents, id)?;
            added.insert(id, ei);
        }
        for c in &simple {
            let idx: Vec<usize> = c.iter().filter_map(|e| added.get(e).copied()).collect();
            let mut ci = 0;
            su!(a.SUGeometryInputAddCurve(gi, idx.len(), idx.as_ptr(), &mut ci))?;
        }
        su!(a.SUEntitiesFill(target, gi, true))
    }

    /// Attribute dictionaries of the faces and edges just made by `fill`: found again by
    /// their points.
    fn geometry_attrs(&self, target: Ref, ents: &Entities, ctx: Option<DefId>) -> Result<()> {
        use slotmap::Key;
        let a = self.a;
        let key = |p: DVec3| {
            [(p.x / IN * 1e4).round() as i64, (p.y / IN * 1e4).round() as i64, (p.z / IN * 1e4).round() as i64]
        };
        let mut faces: HashMap<Vec<[i64; 3]>, (&Attributes, &esq_core::model::Face)> = HashMap::new();
        for (id, f) in ents.faces.iter() {
            if let Some(attrs) = self.model.attr_store.get(&attr_owner('f', ctx, id.data().as_ffi())) {
                let mut k: Vec<[i64; 3]> = f.outer.iter().map(|v| key(ents.verts[*v].pos)).collect();
                k.sort_unstable();
                faces.insert(k, (attrs, f));
            }
        }
        let empty = Attributes::new();
        let mut edges: HashMap<Vec<[i64; 3]>, &Attributes> = HashMap::new();
        // The SDK gives every edge of a curve the softness of its first one: edges of curves
        // that mix them get theirs back once the curves are made.
        let mut mixed: HashSet<u32> = HashSet::new();
        let mut first: HashMap<u32, (bool, bool)> = HashMap::new();
        for e in ents.edges.values() {
            if let Some(c) = e.curve {
                let f = *first.entry(c).or_insert((e.soft, e.smooth));
                if f != (e.soft, e.smooth) {
                    mixed.insert(c);
                }
            }
        }
        let mut flags: HashMap<Vec<[i64; 3]>, (bool, bool)> = HashMap::new();
        for (id, e) in ents.edges.iter() {
            let mut k: Vec<[i64; 3]> = e.v.iter().map(|v| key(ents.verts[*v].pos)).collect();
            k.sort_unstable();
            if e.curve.is_some_and(|c| mixed.contains(&c)) {
                flags.insert(k.clone(), (e.soft, e.smooth));
            }
            if let Some(attrs) = self.model.attr_store.get(&attr_owner('e', ctx, id.data().as_ffi())) {
                edges.insert(k, attrs);
            } else if flags.contains_key(&k) {
                edges.insert(k, &empty);
            }
        }
        let pos = |v: Ref| {
            let mut p = P3::default();
            unsafe { (a.SUVertexGetPosition)(v, &mut p) };
            key(DVec3::new(p.x, p.y, p.z) * IN)
        };
        if !faces.is_empty() {
            for f in list(
                |n| unsafe { (a.SUEntitiesGetNumFaces)(target, n) },
                |n, p, g| unsafe { (a.SUEntitiesGetFaces)(target, n, p, g) },
            ) {
                let mut lp = Ref::NULL;
                unsafe { (a.SUFaceGetOuterLoop)(f, &mut lp) };
                let mut k: Vec<[i64; 3]> = list(
                    |n| unsafe { (a.SULoopGetNumVertices)(lp, n) },
                    |n, p, g| unsafe { (a.SULoopGetVertices)(lp, n, p, g) },
                )
                .into_iter()
                .map(pos)
                .collect();
                k.sort_unstable();
                if let Some((attrs, face)) = faces.get(&k) {
                    write_attrs(a, unsafe { (a.SUFaceToEntity)(f) }, attrs)?;
                    // Then the texture position `fill` left out.
                    let pts: Vec<DVec3> = face.outer.iter().map(|v| ents.verts[*v].pos).collect();
                    for (side, m) in [face.front, face.back].into_iter().enumerate() {
                        let Some(&mr) = m.and_then(|m| self.materials.get(&m)) else { continue };
                        // A colour cannot be positioned (`SUFacePositionMaterial` refuses it).
                        if m.is_some_and(|m| self.model.materials[m].ext.texture.is_none()) {
                            continue;
                        }
                        if let Some(p) = textures::position_input(mr, face.tex.as_deref(), side, &pts, face.normal) {
                            su!(a.SUFacePositionMaterial(f, side == 0, &p))?;
                        }
                    }
                }
            }
        }
        if !edges.is_empty() {
            for e in list(
                |n| unsafe { (a.SUEntitiesGetNumEdges)(target, false, n) },
                |n, p, g| unsafe { (a.SUEntitiesGetEdges)(target, false, n, p, g) },
            ) {
                let (mut s, mut t) = (Ref::NULL, Ref::NULL);
                unsafe {
                    (a.SUEdgeGetStartVertex)(e, &mut s);
                    (a.SUEdgeGetEndVertex)(e, &mut t);
                }
                let mut k = vec![pos(s), pos(t)];
                k.sort_unstable();
                if let Some(&(soft, smooth)) = flags.get(&k) {
                    su!(a.SUEdgeSetSoft(e, soft))?;
                    su!(a.SUEdgeSetSmooth(e, smooth))?;
                }
                if let Some(attrs) = edges.get(&k) {
                    write_attrs(a, unsafe { (a.SUEdgeToEntity)(e) }, attrs)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod census;
mod images;
mod model_info;
pub mod native;
mod sdk_attach;
mod sdk_census;
mod spaces;
mod textures;
