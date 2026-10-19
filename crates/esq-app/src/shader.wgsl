struct Globals {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
    // x: edge depth bias, y: pixels per point, z: profile width (points),
    // w: style bits (1 edges, 2 profiles, 4 extension, 8 unlit faces, 16 fog, 32 back edges,
    // 64 depth cue)
    params: vec4<f32>,
    // xy: viewport size in pixels; zw: nearest and farthest distance of the model (depth cue)
    viewport: vec4<f32>,
    // xyz: eye position, w: 1 for perspective, 0 for parallel projection
    eye: vec4<f32>,
    // xyz: view direction
    fwd: vec4<f32>,
    // x, y: fog start and end distances; z: depth cue width (px)
    fog: vec4<f32>,
    // background colour (linear)
    bg: vec4<f32>,
    // section planes: xyz normal, w offset; cut away where dot(normal, p) < offset
    clips: array<vec4<f32>, 16>,
    // world → shadow map
    light_vp: mat4x4<f32>,
    // xyz towards the sun, w = 1 when the sun lights the faces
    sun: vec4<f32>,
    // x light, y dark (0..1), z = 1 to shadow faces, w = 1 when shadows are cast
    shadow: vec4<f32>,
    // ground square: xy centre, z half size, w = 1 when ground shadows are shown
    ground: vec4<f32>,
    // clip space → world
    inv_view_proj: mat4x4<f32>,
    // sky colour (sRGB 0..1), w = 1 when the sky is drawn
    sky: vec4<f32>,
    // ambient occlusion: x radius (mm), y intensity (contrast), z multiplier
    ao: vec4<f32>,
    // edge style: x extension length, y endpoint length (points), z endpoint width factor,
    // w jitter (points)
    edge: vec4<f32>,
    // colour the fog fades to (linear)
    fog_color: vec4<f32>,
    // ground colour (linear) and opacity
    ground_color: vec4<f32>,
    // environment: x rotation (radians), y skydome exposure, z reflection exposure,
    // w bits (1 skydome, 2 reflections)
    env: vec4<f32>,
    // environment panorama: x number of levels, y level of the sky
    env_lod: vec4<f32>,
    // modeling grid: origin (w bits: 1 shown, 2 major, 4 minor, 8 radial), u and v
    // directions (w: spacing), major and minor lines (linear colour, thickness px),
    // misc (subdivisions u and v, dash of major and minor), quad (centre u v, half size,
    // spokes)
    grid_origin: vec4<f32>,
    grid_u: vec4<f32>,
    grid_v: vec4<f32>,
    grid_major: vec4<f32>,
    grid_minor: vec4<f32>,
    grid_misc: vec4<f32>,
    grid_quad: vec4<f32>,
};

// Window › Environments: an equirectangular panorama. Its point u lies at angle
// rotation − u·2π from the red axis' negative side (see `environments.rs`), v goes from the
// zenith (0) to the nadir (1).
@group(0) @binding(6) var env_tex: texture_2d<f32>;
@group(0) @binding(7) var env_samp: sampler;

fn env_bits() -> u32 {
    return u32(g.env.w + 0.5);
}

fn env_at(d: vec3<f32>, lod: f32) -> vec3<f32> {
    let el = asin(clamp(d.z, -1.0, 1.0));
    let a = atan2(-d.y, -d.x);
    let u = fract((g.env.x - a) / 6.28318531);
    let v = 0.5 - el / 3.14159265;
    return textureSampleLevel(env_tex, env_samp, vec2(u, v), lod).rgb;
}

// Background › Ground: a plane at z = 0 that hides (partly, by its opacity) what lies on
// the other side of it from the eye. Seen from below only with "show ground from below".
fn ground_veil(world: vec3<f32>) -> f32 {
    let bits = style();
    if ((bits & 1024u) == 0u) {
        return 0.0;
    }
    let eye_z = g.eye.z;
    if (eye_z >= 0.0 && world.z < -0.5) {
        return g.ground_color.a;
    }
    if (eye_z < 0.0 && (bits & 2048u) != 0u && world.z > 0.5) {
        return g.ground_color.a;
    }
    return 0.0;
}

fn grounded(c: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    return mix(c, g.ground_color.rgb, ground_veil(world));
}

@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_cmp: sampler_comparison;

// 1 where the sun reaches the point, 0 in shadow (soft over a 3×3 texel area).
fn sunlit(p: vec3<f32>) -> f32 {
    if (g.shadow.w < 0.5) {
        return 1.0;
    }
    let l = g.light_vp * vec4(p, 1.0);
    let uv = vec2(l.x * 0.5 + 0.5, 0.5 - l.y * 0.5);
    if (any(uv < vec2(0.0)) || any(uv > vec2(1.0)) || l.z > 1.0) {
        return 1.0;
    }
    let texel = 1.0 / vec2<f32>(textureDimensions(shadow_map));
    var sum = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            sum += textureSampleCompareLevel(shadow_map, shadow_cmp, uv + vec2(f32(x), f32(y)) * texel, l.z);
        }
    }
    return sum / 9.0;
}

// Whether one of the section planes in `mask` cuts the point away.
fn clipped(p: vec3<f32>, bits: u32) -> bool {
    // Faces keep roughness and metalness in the upper 16 bits (see `photoreal`).
    let mask = bits & 0xffffu;
    if (mask == 0u) {
        return false;
    }
    for (var i = 0u; i < 16u; i++) {
        if ((mask & (1u << i)) != 0u) {
            let c = g.clips[i];
            if (dot(c.xyz, p) < c.w - 0.02) {
                return true;
            }
        }
    }
    return false;
}

fn style() -> u32 {
    return u32(g.params.w + 0.5);
}

// View › Fog: linear between the start and end distances (along the view direction), to the
// fog colour (the background's, or its own).
fn fogged(c: vec3<f32>, dist: f32) -> vec3<f32> {
    if ((style() & 16u) == 0u) {
        return c;
    }
    let f = clamp((dist - g.fog.x) / max(g.fog.y - g.fog.x, 1e-3), 0.0, 1.0);
    return mix(c, g.fog_color.rgb, f);
}

@group(0) @binding(0) var<uniform> g: Globals;

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

struct FaceIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) front: vec4<f32>,
    @location(3) back: vec4<f32>,
    @location(4) clip: u32,
    @location(5) uv_front: vec3<f32>,
    @location(6) uv_back: vec3<f32>,
    @location(7) tex: u32,
};

struct FaceOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) front: vec4<f32>,
    @location(2) back: vec4<f32>,
    @location(3) dist: f32,
    @location(4) world: vec3<f32>,
    @location(5) @interpolate(flat) cut_mask: u32,
    @location(6) uv_front: vec3<f32>,
    @location(7) @interpolate(flat) tex: u32,
    @location(8) uv_back: vec3<f32>,
};

// Material textures: one per layer (see `scene::FaceVertex::tex`).
@group(0) @binding(4) var tex_array: texture_2d_array<f32>;
@group(0) @binding(5) var tex_samp: sampler;

// A face-me component whose shadows face the sun: the faces shown cast no shadow, a copy
// turned to the sun casts it and is not shown (`scene::SHADOW_ONLY`, `scene::NO_SHADOW`).
const SHADOW_ONLY: u32 = 134217728u; // 1 << 27
const NO_SHADOW: u32 = 268435456u; // 1 << 28
// Receive Shadows off: no cast shadow darkens the face (`scene::NO_RECEIVE`).
const NO_RECEIVE: u32 = 536870912u; // 1 << 29
const OUTSIDE: vec4<f32> = vec4(2.0, 2.0, 2.0, 1.0);

@vertex
fn vs_face(v: FaceIn) -> FaceOut {
    var o: FaceOut;
    o.clip = select(g.view_proj * vec4(v.pos, 1.0), OUTSIDE, (v.tex & SHADOW_ONLY) != 0u);
    o.normal = v.normal;
    o.front = v.front;
    o.back = v.back;
    o.dist = dot(v.pos - g.eye.xyz, g.fwd.xyz);
    o.world = v.pos;
    o.cut_mask = v.clip;
    o.uv_front = v.uv_front;
    o.uv_back = v.uv_back;
    o.tex = v.tex;
    return o;
}

@fragment
fn fs_face(i: FaceOut, @builtin(front_facing) ff: bool) -> @location(0) vec4<f32> {
    return shade_face(i, ff);
}

struct OitOut {
    @location(0) accum: vec4<f32>,
    @location(1) reveal: vec4<f32>,
};

// Styles › Faces › Transparency quality « Rapide » (`oit.rs`): the face added into the
// colour accumulator (colour × alpha, alpha) and the coverage product (× 1 − alpha).
@fragment
fn fs_face_oit(i: FaceOut, @builtin(front_facing) ff: bool) -> OitOut {
    let c = shade_face(i, ff);
    var o: OitOut;
    o.accum = vec4(c.rgb * c.a, c.a);
    o.reveal = vec4(c.a);
    return o;
}

fn shade_face(i: FaceOut, ff: bool) -> vec4<f32> {
    // The texture of the side seen (sampled before any branch: mipmaps need derivatives).
    // Texture coordinates start at the picture's bottom left, its rows from the top.
    // An image's picture does not repeat: its edges do not bleed into each other.
    // The coordinates are homogeneous (a texture distorted in perspective): divided here, per
    // pixel, they stay continuous across the face's triangles.
    let layer = select((i.tex >> 9u) & 511u, i.tex & 511u, ff);
    let uvq = select(i.uv_back, i.uv_front, ff);
    let q = select(uvq.z, 1.0, abs(uvq.z) < 1e-20);
    var st = uvq.xy / q;
    if ((i.tex & (1u << 26u)) != 0u) {
        let half = 0.5 / f32(textureDimensions(tex_array).x);
        st = clamp(st, vec2(half), vec2(1.0 - half));
    }
    let texel = textureSample(tex_array, tex_samp, vec2(st.x, 1.0 - st.y), max(i32(layer) - 1, 0));
    if (clipped(i.world, i.cut_mask)) {
        discard;
    }
    var n = normalize(i.normal);
    var c = i.front;
    if (!ff) {
        n = -n;
        c = i.back;
    }
    // A textured side: the texture, veiled by the side's colour (selection, fading).
    if (layer > 0u) {
        let t = f32((i.tex >> 18u) & 255u) / 255.0;
        c = vec4(mix(texel.rgb, c.rgb, t), c.a * texel.a);
    }
    var shade = 1.0;
    var gamma_shade = 1.0;
    if (g.sun.w > 0.5) {
        // Sun lighting, measured on SketchUp 2026: 0.2 + dark + light × cos, capped at 1;
        // a cast shadow darkens like a black veil of opacity 0.8 × (1 − dark).
        let cosine = max(dot(n, g.sun.xyz), 0.0) * f32(g.sun.z > 0.0);
        // SketchUp applies these factors to the 8-bit (sRGB) colour.
        gamma_shade = min(0.2 + g.shadow.y + g.shadow.x * cosine, 1.0);
        // As in SketchUp, only the front side of a face receives cast shadows, unless the
        // face (or a group around it) receives none.
        let receives = (i.tex & NO_RECEIVE) == 0u;
        if (cosine > 0.0 && g.shadow.z > 0.5 && ff && receives) {
            gamma_shade *= 1.0 - 0.8 * (1.0 - g.shadow.y) * (1.0 - sunlit(i.world));
        }
    } else {
        let l = normalize(g.light_dir.xyz);
        let diffuse = max(dot(n, l), 0.0);
        let fill = max(dot(n, normalize(vec3(-0.4, -0.2, 0.3))), 0.0);
        shade = 0.58 + 0.34 * diffuse + 0.08 * fill;
    }
    if ((style() & 8u) != 0u) {
        shade = 1.0;
        gamma_shade = 1.0;
    }
    var rgb = srgb_to_linear(c.rgb * gamma_shade) * shade;
    if ((style() & 256u) != 0u && (style() & 8u) == 0u) {
        var vis = 1.0;
        if (g.sun.w > 0.5 && g.shadow.z > 0.5 && ff && (i.tex & NO_RECEIVE) == 0u) {
            vis = sunlit(i.world);
        }
        rgb = photoreal(c.rgb, n, i.world, i.cut_mask, vis);
    }
    // Ambient occlusion of the nearest opaque surface (computed from the depth pre-pass).
    if ((style() & 128u) != 0u && c.a > 0.999 && (style() & 8u) == 0u) {
        rgb *= textureLoad(ao_tex, vec2<i32>(i.clip.xy), 0).r;
    }
    return vec4(fogged(grounded(rgb, i.world), i.dist), c.a);
}

// Photoreal materials: a metallic-roughness (PBR) look lit by the sun (or a camera light)
// and by an environment made of the sky and the ground. The roughness and the metalness
// of the face travel in the upper 16 bits of its clip mask.
fn env_color(d: vec3<f32>, rough: f32) -> vec3<f32> {
    let sky = srgb_to_linear(g.sky.rgb);
    let horizon = mix(sky, vec3(1.0), 0.75);
    let ground = g.bg.rgb * 0.7;
    let up = mix(horizon, sky, smoothstep(0.0, 0.5, d.z));
    let e = mix(ground, up, smoothstep(-0.05, 0.05, d.z));
    // A rough surface sees a blurred environment: the average of sky and ground.
    return mix(e, mix(ground, mix(sky, horizon, 0.5), 0.5), rough);
}

fn photoreal(base: vec3<f32>, n: vec3<f32>, world: vec3<f32>, bits: u32, vis: f32) -> vec3<f32> {
    let rough = max(f32((bits >> 16u) & 255u) / 255.0, 0.04);
    let metal = f32((bits >> 24u) & 255u) / 255.0;
    let albedo = srgb_to_linear(base);
    var v = -g.fwd.xyz;
    if (g.eye.w > 0.5) {
        v = normalize(g.eye.xyz - world);
    }
    var l = normalize(g.light_dir.xyz);
    if (g.sun.w > 0.5) {
        l = g.sun.xyz;
    }
    let nl = max(dot(n, l), 0.0);
    let nv = max(dot(n, v), 1e-3);
    let h = normalize(l + v);
    let nh = max(dot(n, h), 0.0);
    let vh = max(dot(v, h), 0.0);
    // GGX distribution, Smith-Schlick visibility, Schlick Fresnel.
    let a2 = pow(rough, 4.0);
    let dd = nh * nh * (a2 - 1.0) + 1.0;
    let ndf = a2 / (3.14159265 * dd * dd);
    let k = (rough + 1.0) * (rough + 1.0) / 8.0;
    let vis_g = (nl / (nl * (1.0 - k) + k)) * (nv / (nv * (1.0 - k) + k));
    let f0 = mix(vec3(0.04), albedo, metal);
    let fres = f0 + (vec3(1.0) - f0) * pow(1.0 - vh, 5.0);
    let spec = ndf * vis_g * fres / max(4.0 * nl * nv, 1e-3);
    let sun_i = 0.8;
    let hemi = mix(g.bg.rgb * 0.7, mix(srgb_to_linear(g.sky.rgb), vec3(1.0), 0.6), n.z * 0.5 + 0.5);
    var ambient = hemi * 0.7;
    // Environment reflection, grazing angles reflect more (rough surfaces less).
    let f_env = f0 + (max(vec3(1.0 - rough), f0) - f0) * pow(1.0 - nv, 5.0);
    var env = f_env * env_color(reflect(-v, n), rough);
    // Window › Environments, "for reflections": the panorama lights the material (its most
    // blurred levels around the normal) and is mirrored (blurred by the roughness).
    if ((env_bits() & 2u) != 0u) {
        let top = g.env_lod.x - 1.0;
        ambient = env_at(n, max(top - 2.0, 0.0)) * g.env.z;
        env = f_env * env_at(reflect(-v, n), top * min(rough * 1.25, 1.0) * 0.85) * g.env.z;
    }
    let diffuse = albedo * (1.0 - metal) * 0.96 * (ambient + vec3(sun_i * nl * vis));
    return diffuse + spec * nl * vis * sun_i * 2.0 + env;
}

// Ambient occlusion: a depth-only pre-pass of the opaque faces, then a hemisphere of
// samples around each pixel's surface point (radius `ao.x`), then a depth-aware blur.
@group(0) @binding(3) var ao_tex: texture_2d<f32>;
@group(1) @binding(0) var scene_depth: texture_depth_2d;
@group(1) @binding(1) var ao_in: texture_2d<f32>;

fn depth_at(px: vec2<i32>) -> f32 {
    let size = vec2<i32>(g.viewport.xy);
    return textureLoad(scene_depth, clamp(px, vec2<i32>(0), size - vec2<i32>(1)), 0);
}

fn unproject(px: vec2<i32>, d: f32) -> vec3<f32> {
    let sz = g.viewport.xy;
    let ndc = vec2((f32(px.x) + 0.5) / sz.x * 2.0 - 1.0, 1.0 - (f32(px.y) + 0.5) / sz.y * 2.0);
    let w = g.inv_view_proj * vec4(ndc, d, 1.0);
    return w.xyz / w.w;
}

fn view_depth(p: vec3<f32>) -> f32 {
    return dot(p - g.eye.xyz, g.fwd.xyz);
}

fn ign(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2(0.06711056, 0.00583715))));
}

@vertex
fn vs_pre(v: FaceIn) -> ShadowOut {
    var o: ShadowOut;
    o.clip = select(g.view_proj * vec4(v.pos, 1.0), OUTSIDE, (v.tex & SHADOW_ONLY) != 0u);
    o.world = v.pos;
    o.cut_mask = v.clip;
    return o;
}

@fragment
fn fs_pre(i: ShadowOut) {
    if (clipped(i.world, i.cut_mask)) {
        discard;
    }
}

@fragment
fn fs_ao(i: SkyOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(i.clip.xy);
    let d0 = depth_at(px);
    if (d0 <= 0.0) {
        return vec4(1.0);
    }
    let p = unproject(px, d0);
    let dp = view_depth(p);
    // Normal from the closer neighbour on each axis.
    var dx = vec3(0.0);
    var dy = vec3(0.0);
    let dl = depth_at(px + vec2<i32>(-1, 0));
    let dr = depth_at(px + vec2<i32>(1, 0));
    let du = depth_at(px + vec2<i32>(0, -1));
    let dd = depth_at(px + vec2<i32>(0, 1));
    var ex = vec3(0.0);
    if (dl > 0.0 && dr > 0.0) {
        let pl = unproject(px + vec2<i32>(-1, 0), dl);
        let pr = unproject(px + vec2<i32>(1, 0), dr);
        ex = select(p - pl, pr - p, abs(view_depth(pr) - dp) < abs(dp - view_depth(pl)));
    }
    var ey = vec3(0.0);
    if (du > 0.0 && dd > 0.0) {
        let pu = unproject(px + vec2<i32>(0, -1), du);
        let pd = unproject(px + vec2<i32>(0, 1), dd);
        ey = select(p - pu, pd - p, abs(view_depth(pd) - dp) < abs(dp - view_depth(pu)));
    }
    dx = ex;
    dy = ey;
    var n = cross(dx, dy);
    if (dot(n, n) < 1e-12) {
        return vec4(1.0);
    }
    n = normalize(n);
    var to_eye = -g.fwd.xyz;
    if (g.eye.w > 0.5) {
        to_eye = g.eye.xyz - p;
    }
    if (dot(n, to_eye) < 0.0) {
        n = -n;
    }
    let radius = g.ao.x;
    let t = normalize(cross(n, select(vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), abs(n.x) > 0.9)));
    let b = cross(n, t);
    let rot = ign(vec2<f32>(px)) * 6.2831853;
    var occ = 0.0;
    for (var k = 0; k < 16; k++) {
        let fk = f32(k);
        let u = fract(0.5 + fk * 0.6180339887);
        let ang = rot + fk * 2.399963;
        let r2 = (fk + 0.5) / 16.0;
        let sr = sqrt(r2);
        let dir = t * (cos(ang) * sr) + b * (sin(ang) * sr) + n * sqrt(1.0 - r2);
        let s = p + dir * (radius * mix(0.15, 1.0, u * u)) + n * (radius * 0.02);
        let c = g.view_proj * vec4(s, 1.0);
        if (c.w <= 1e-4) {
            continue;
        }
        let q = vec2(c.x / c.w * 0.5 + 0.5, 0.5 - c.y / c.w * 0.5) * g.viewport.xy;
        let spx = vec2<i32>(floor(q));
        if (q.x < 0.0 || q.y < 0.0 || q.x >= g.viewport.x || q.y >= g.viewport.y) {
            continue;
        }
        let ds = depth_at(spx);
        if (ds <= 0.0) {
            continue;
        }
        let dq = view_depth(unproject(spx, ds));
        // The surface seen through the sample's pixel is nearer than the sample: it hides it.
        if (dq < view_depth(s) - radius * 0.02) {
            occ += smoothstep(0.0, 1.0, radius / max(abs(dp - dq), 1e-3));
        }
    }
    occ = occ / 16.0;
    return vec4(pow(clamp(1.0 - occ * g.ao.z, 0.0, 1.0), g.ao.y), 0.0, 0.0, 1.0);
}

@fragment
fn fs_ao_blur(i: SkyOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(i.clip.xy);
    let d0 = depth_at(px);
    if (d0 <= 0.0) {
        return vec4(1.0);
    }
    let z0 = view_depth(unproject(px, d0));
    var sum = 0.0;
    var wsum = 0.0;
    for (var y = -2; y <= 2; y++) {
        for (var x = -2; x <= 2; x++) {
            let q = px + vec2<i32>(x, y);
            let dq = depth_at(q);
            if (dq <= 0.0) {
                continue;
            }
            let w = exp(-abs(view_depth(unproject(q, dq)) - z0) / max(g.ao.x * 0.05, 1e-3));
            sum += w * textureLoad(ao_in, q, 0).r;
            wsum += w;
        }
    }
    return vec4(sum / max(wsum, 1e-4), 0.0, 0.0, 1.0);
}

// Background: the sky above the horizon, the background colour below. Measured on
// SketchUp 2026: the sky colour is mixed with white, 75 % at the horizon, fading out
// (smoothstep) about 18° above it.

struct SkyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_sky(@builtin(vertex_index) vi: u32) -> SkyOut {
    var pts = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    var o: SkyOut;
    o.clip = vec4(pts[vi], 0.0, 1.0);
    o.ndc = pts[vi];
    return o;
}

@fragment
fn fs_sky(i: SkyOut) -> @location(0) vec4<f32> {
    let a = g.inv_view_proj * vec4(i.ndc, 1.0, 1.0);
    let b = g.inv_view_proj * vec4(i.ndc, 0.01, 1.0);
    let d = normalize(b.xyz / b.w - a.xyz / a.w);
    // The environment as a skydome replaces the background, sky and ground.
    if ((env_bits() & 1u) != 0u) {
        return vec4(min(env_at(d, g.env_lod.y) * g.env.y, vec3(1.0)), 1.0);
    }
    var col = g.bg.rgb;
    if (g.sky.w > 0.5 && d.z > 0.0) {
        let e = degrees(asin(clamp(d.z, 0.0, 1.0)));
        let t = 0.75 * (1.0 - smoothstep(0.0, 18.0, e));
        col = srgb_to_linear(mix(g.sky.rgb, vec3(1.0), t));
    }
    // Background › Ground: below the horizon seen from above, above it seen from below.
    let bits = style();
    if ((bits & 1024u) != 0u) {
        let above = g.eye.z >= 0.0;
        if ((above && d.z < 0.0) || (!above && (bits & 2048u) != 0u && d.z > 0.0)) {
            col = mix(col, g.ground_color.rgb, g.ground_color.a);
        }
    }
    return vec4(col, 1.0);
}

// The sun's view, for the shadow map.

struct ShadowOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) @interpolate(flat) cut_mask: u32,
};

@vertex
fn vs_shadow(v: FaceIn) -> ShadowOut {
    var o: ShadowOut;
    o.clip = select(g.light_vp * vec4(v.pos, 1.0), OUTSIDE, (v.tex & NO_SHADOW) != 0u);
    o.world = v.pos;
    o.cut_mask = v.clip;
    return o;
}

// What a section plane cuts away casts no shadow.
@fragment
fn fs_shadow(i: ShadowOut) {
    if (clipped(i.world, i.cut_mask)) {
        discard;
    }
}

// Shadows on the ground (z = 0), where no face lies.

struct GroundOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
};

@vertex
fn vs_ground(@builtin(vertex_index) vi: u32) -> GroundOut {
    var corners = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
        vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0),
    );
    let p = vec3(g.ground.xy + corners[vi] * g.ground.z, 0.0);
    var o: GroundOut;
    o.clip = g.view_proj * vec4(p, 1.0);
    o.world = p;
    return o;
}

@fragment
fn fs_ground(i: GroundOut) -> @location(0) vec4<f32> {
    let a = 0.8 * (1.0 - g.shadow.y) * (1.0 - sunlit(i.world));
    if (g.ground.w < 0.5 || a < 0.004) {
        discard;
    }
    // A veil of opacity `a` over the 8-bit background, as SketchUp blends it.
    let bg = max(g.bg.g, 1e-4);
    let bg_srgb = pow(bg, 1.0 / 2.4) * 1.055 - 0.055;
    let veiled = srgb_to_linear(vec3(bg_srgb * (1.0 - a))).x;
    return vec4(0.0, 0.0, 0.0, 1.0 - veiled / bg);
}

// View › Grid: the modeling grid, drawn procedurally on a large square of its plane. Lines
// are anti-aliased from the screen-space rate of the grid coordinates and fade out where
// they would crowd (less than a few pixels apart).

struct GridOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs_grid(@builtin(vertex_index) vi: u32) -> GridOut {
    var corners = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
        vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0),
    );
    let uv = g.grid_quad.xy + corners[vi] * g.grid_quad.z;
    let p = g.grid_origin.xyz + g.grid_u.xyz * uv.x + g.grid_v.xyz * uv.y;
    var o: GridOut;
    o.clip = g.view_proj * vec4(p, 1.0);
    o.world = p;
    o.uv = uv;
    return o;
}

// How much of a line of `width` pixels covers this pixel: lines at multiples of `step` of
// coordinate `x`, which changes by `fw` per pixel.
fn grid_cover(x: f32, step: f32, fw: f32, width: f32) -> f32 {
    let d = abs(x - step * round(x / step)) / max(fw, 1e-6);
    // Lines that crowd fade out: gone when they are less than two widths apart.
    let room = step / max(fw, 1e-6);
    return clamp(width * 0.5 + 0.5 - d, 0.0, 1.0) * clamp((room - 2.0 * width - 1.0) / (3.0 * width + 3.0), 0.0, 1.0);
}

@fragment
fn fs_grid(i: GridOut) -> @location(0) vec4<f32> {
    let bits = u32(g.grid_origin.w + 0.5);
    let radial = (bits & 8u) != 0u;
    let r = length(i.uv);
    let ang = atan2(i.uv.y, i.uv.x);
    // Rates first (uniform control flow).
    let fu = fwidth(i.uv.x);
    let fv = fwidth(i.uv.y);
    let fr = fwidth(r);
    let fpx = max(fu, fv);
    let su = g.grid_u.w;
    let sv = g.grid_v.w;
    let spokes = max(g.grid_quad.w, 1.0);
    var major = 0.0;
    var minor = 0.0;
    var along_u = i.uv.y / max(fv, 1e-6);
    var along_v = i.uv.x / max(fu, 1e-6);
    if (radial) {
        let step_a = 6.28318531 / spokes;
        let da = (ang - step_a * round(ang / step_a)) * r;
        let spoke = clamp(g.grid_major.w * 0.5 + 0.5 - abs(da) / max(fpx, 1e-6), 0.0, 1.0)
            * clamp((r * step_a / max(fpx, 1e-6) - 1.5) / 1.5, 0.0, 1.0);
        major = max(grid_cover(r, su, fr, g.grid_major.w), spoke);
        if (g.grid_misc.x > 1.0) {
            minor = grid_cover(r, su / g.grid_misc.x, fr, g.grid_minor.w);
        }
        if (g.grid_misc.y > 1.0) {
            let step_m = step_a / g.grid_misc.y;
            let dm = (ang - step_m * round(ang / step_m)) * r;
            minor = max(minor, clamp(g.grid_minor.w * 0.5 + 0.5 - abs(dm) / max(fpx, 1e-6), 0.0, 1.0)
                * clamp((r * step_m / max(fpx, 1e-6) - 1.5) / 1.5, 0.0, 1.0));
        }
        along_u = r / max(fr, 1e-6);
        along_v = along_u;
    } else {
        major = max(grid_cover(i.uv.x, su, fu, g.grid_major.w), grid_cover(i.uv.y, sv, fv, g.grid_major.w));
        if (g.grid_misc.x > 1.0) {
            minor = grid_cover(i.uv.x, su / g.grid_misc.x, fu, g.grid_minor.w);
        }
        if (g.grid_misc.y > 1.0) {
            minor = max(minor, grid_cover(i.uv.y, sv / g.grid_misc.y, fv, g.grid_minor.w));
        }
    }
    if ((bits & 2u) == 0u) {
        major = 0.0;
    }
    if ((bits & 4u) == 0u) {
        minor = 0.0;
    }
    // Dash patterns, measured in pixels along the line.
    let dash_major = u32(g.grid_misc.z + 0.5);
    let dash_minor = u32(g.grid_misc.w + 0.5);
    let along = select(along_v, along_u, abs(i.uv.x - su * round(i.uv.x / su)) / max(fu, 1e-6) < 2.0);
    if (dash_major != 0u && !dash_on(dash_major, max(along, 0.0) / g.params.y)) {
        major = 0.0;
    }
    if (dash_minor != 0u && !dash_on(dash_minor, max(along, 0.0) / g.params.y)) {
        minor = 0.0;
    }
    let a = max(major, minor);
    if (a < 0.01) {
        discard;
    }
    let c = select(g.grid_minor.rgb, g.grid_major.rgb, major >= minor);
    return vec4(fogged(c, dot(i.world - g.eye.xyz, g.fwd.xyz)), a);
}

// Lines are instanced quads, so that they can be thicker than one pixel.

struct LineIn {
    @builtin(vertex_index) vi: u32,
    @location(0) a: vec3<f32>,
    @location(1) b: vec3<f32>,
    @location(2) t1: vec3<f32>,
    @location(3) t2: vec3<f32>,
    @location(4) color: vec4<f32>,
    @location(5) width: f32,
    @location(6) flags: u32,
};

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    // Distance from the start of the line, in pixels (for dashes and endpoints).
    @location(1) @interpolate(linear) along: f32,
    @location(2) @interpolate(flat) flags: u32,
    @location(3) dist: f32,
    @location(4) world: vec3<f32>,
};
// A silhouette edge: both faces lie on the same side of the plane through the edge and the eye.
fn is_profile(v: LineIn) -> bool {
    if ((v.flags & 2u) == 0u || (style() & 2u) == 0u) {
        return false;
    }
    if (dot(v.t2, v.t2) < 0.25) {
        return true;
    }
    var view = g.fwd.xyz;
    if (g.eye.w > 0.5) {
        view = (v.a + v.b) * 0.5 - g.eye.xyz;
    }
    let q = cross(v.b - v.a, view);
    return dot(v.t1, q) * dot(v.t2, q) > 0.0;
}

// A pseudo-random offset in [-1, 1]², the same for a given edge stroke at every frame.
fn jitter_offset(p: vec3<f32>, k: f32) -> vec2<f32> {
    let h = vec2(dot(p, vec3(12.9898, 78.233, 37.719)) + k * 17.31, dot(p, vec3(39.346, 11.135, 83.155)) + k * 9.17);
    return fract(sin(h) * 43758.5453) * 2.0 - 1.0;
}

@vertex
fn vs_line(v: LineIn) -> LineOut {
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
        vec2(0.0, -1.0), vec2(1.0, 1.0), vec2(0.0, 1.0),
    );
    let corner = corners[v.vi];
    var o: LineOut;
    o.color = v.color;
    o.flags = v.flags;
    o.along = 0.0;
    var ca = g.view_proj * vec4(v.a, 1.0);
    var cb = g.view_proj * vec4(v.b, 1.0);
    // Keep the part of the segment in front of the eye.
    let eps = 1e-3;
    if (ca.w < eps && cb.w < eps) {
        o.clip = vec4(2.0, 2.0, 2.0, 1.0);
        return o;
    }
    if (ca.w < eps) {
        ca = mix(ca, cb, (eps - ca.w) / (cb.w - ca.w));
    }
    if (cb.w < eps) {
        cb = mix(cb, ca, (eps - cb.w) / (ca.w - cb.w));
    }
    let half_vp = g.viewport.xy * 0.5;
    var sa = ca.xy / ca.w * half_vp;
    var sb = cb.xy / cb.w * half_vp;
    // Edge style: Jitter. The extra strokes of an edge stray a little at each end.
    let stroke = (v.flags >> 28u) & 3u;
    if (stroke != 0u) {
        let amp = g.edge.w * g.params.y;
        sa += jitter_offset(v.a, f32(stroke)) * amp;
        sb += jitter_offset(v.b, f32(stroke) + 0.5) * amp;
    }
    let d = sb - sa;
    let len = length(d);
    var dir = vec2(1.0, 0.0);
    if (len > 1e-4) {
        dir = d / len;
    }
    let profile = is_profile(v);
    // View > Edges off: only profiles, guides, axes and the selection remain.
    if ((style() & 1u) == 0u && !profile && (v.flags & 4u) == 0u) {
        o.clip = vec4(2.0, 2.0, 2.0, 1.0);
        return o;
    }
    var width = v.width;
    if (profile) {
        width = g.params.z;
    }
    // Edge style: Depth cue. The nearer the point, the thicker the line, from 1x at the far
    // end of the model up to `fog.z`x (1 to 4 px) at the near end; the quad tapers.
    if ((style() & 64u) != 0u) {
        let d = dot(mix(v.a, v.b, corner.x) - g.eye.xyz, g.fwd.xyz);
        let t = 1.0 - clamp((d - g.viewport.z) / max(g.viewport.w - g.viewport.z, 1e-3), 0.0, 1.0);
        width = width * (1.0 + (g.fog.z - 1.0) * t);
    }
    var half = 0.5 * width * g.params.y;
    let edge = (v.flags & 16u) != 0u;
    // Edge style: Extension. Edges run a little past their ends, like a hand sketch.
    var ext = half;
    if ((style() & 4u) != 0u && edge) {
        ext = half + g.edge.x * g.params.y;
    }
    // The span of the edge this quad covers (in pixels and clip space): all of it, or for
    // the end caps of Edge style > Endpoints, the last pixels at one end, thicker.
    var qa = sa;
    var qb = sb;
    var cqa = ca;
    var cqb = cb;
    var start = 0.0;
    let cap = (v.flags >> 5u) & 3u;
    if (cap != 0u) {
        if ((style() & 512u) == 0u) {
            o.clip = vec4(2.0, 2.0, 2.0, 1.0);
            return o;
        }
        let t = clamp(g.edge.y * g.params.y / max(len, 1e-4), 0.0, 1.0);
        if (cap == 1u) {
            qb = mix(sa, sb, t);
            cqb = mix(ca, cb, t);
        } else {
            qa = mix(sa, sb, 1.0 - t);
            cqa = mix(ca, cb, 1.0 - t);
            start = len * (1.0 - t);
        }
        half = half * g.edge.z;
        ext = half;
    }
    var c = vec4(qa / half_vp * cqa.w, cqa.z, cqa.w);
    if (corner.x > 0.5) {
        c = vec4(qb / half_vp * cqb.w, cqb.z, cqb.w);
    }
    let side = vec2(-dir.y, dir.x) * corner.y * half;
    let capv = dir * (corner.x * 2.0 - 1.0) * ext;
    c = vec4(c.xy + (side + capv) / half_vp * c.w, c.z, c.w);
    // Reversed Z: a slightly larger depth wins over the face the edge lies on.
    c.z = c.z * (1.0 + g.params.x) + g.params.x * 1e-5 * c.w;
    o.clip = c;
    o.along = start + mix(-ext, length(qb - qa) + ext, corner.x);
    o.world = mix(v.a, v.b, corner.x);
    o.dist = dot(o.world - g.eye.xyz, g.fwd.xyz);
    return o;
}

// Edge style › Dashes: whether the point `x` (points from the start of the line) is drawn,
// for the dash pattern of the edge's tag (see `scene::dash_pattern`).
fn dash_on(pattern: u32, x: f32) -> bool {
    var seg = array<f32, 6>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    switch pattern {
        case 1u: { seg = array<f32, 6>(2.0, 3.0, 0.0, 0.0, 0.0, 0.0); }
        case 2u: { seg = array<f32, 6>(6.0, 4.0, 0.0, 0.0, 0.0, 0.0); }
        case 3u: { seg = array<f32, 6>(14.0, 5.0, 0.0, 0.0, 0.0, 0.0); }
        case 4u: { seg = array<f32, 6>(10.0, 3.0, 2.0, 3.0, 0.0, 0.0); }
        case 5u: { seg = array<f32, 6>(18.0, 3.0, 5.0, 3.0, 0.0, 0.0); }
        case 6u: { seg = array<f32, 6>(8.0, 3.0, 2.0, 3.0, 2.0, 3.0); }
        case 7u: { seg = array<f32, 6>(18.0, 3.0, 4.0, 3.0, 4.0, 3.0); }
        default: { return true; }
    }
    let period = seg[0] + seg[1] + seg[2] + seg[3] + seg[4] + seg[5];
    var r = x - floor(x / period) * period;
    for (var k = 0; k < 6; k += 2) {
        if (r < seg[k]) {
            return true;
        }
        r -= seg[k] + seg[k + 1];
        if (r < 0.0) {
            return false;
        }
    }
    return false;
}

// Whether a fragment of a line is left out by its style: hidden geometry, tag dashes.
fn line_hole(i: LineOut) -> bool {
    if ((i.flags & 1u) != 0u) {
        let period = 7.0 * g.params.y;
        if (fract(i.along / period) > 0.55) {
            return true;
        }
    }
    let pattern = (i.flags >> 24u) & 15u;
    if (pattern != 0u && !dash_on(pattern, max(i.along, 0.0) / g.params.y)) {
        return true;
    }
    return false;
}

@fragment
fn fs_line(i: LineOut) -> @location(0) vec4<f32> {
    if (clipped(i.world, i.flags >> 8u) || line_hole(i)) {
        discard;
    }
    return vec4(fogged(grounded(srgb_to_linear(i.color.rgb), i.world), i.dist), i.color.a);
}

// Edge style › Back Edges: the parts of edges hidden behind faces, dashed.
@fragment
fn fs_line_back(i: LineOut) -> @location(0) vec4<f32> {
    let wanted = (style() & 32u) != 0u || (i.flags & 8u) != 0u;
    if (!wanted || (i.flags & 1u) != 0u || fract(i.along / (5.0 * g.params.y)) > 0.5) {
        discard;
    }
    if (clipped(i.world, i.flags >> 8u)) {
        discard;
    }
    return vec4(fogged(grounded(srgb_to_linear(i.color.rgb), i.world), i.dist), i.color.a);
}