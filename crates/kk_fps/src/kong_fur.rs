//! Kong's fur: the Jade `MDF FUR` modifier (type 0x2a "dynamic fur", `MDFmodifier_FUR.c`), ported as shell fur.
//!
//! What the original does (KingKong8.exe) [C]:
//! * loader `fn@0x0096d2e0` reads the modifier (version 2): `len, a, b, layers, k14, k18, k1c, k24, k20`;
//! * apply `fn@0x0096a880` / `fn@0x00a1c990`: shell step = (len / 100) / layers, texcoord shift per shell =
//!   (a / 10000, b / 10000);
//! * draw `fn@0x00a4c090` (OGLShiftDraw.c): for shell i = 0..layers the skinned mesh is re-drawn with every vertex
//!   pushed out along its normal by i * step, its texcoords shifted by i * shift, and alpha test GL_GEQUAL i / layers
//!   against the material's fur layer, a 512 px strand texture whose alpha is the strand height (RGB ~27/255);
//! * the fur layer is the second layer of Kong's multitexture material (`MATmultitexture.c` `fn@0x009d2770`), its UV
//!   matrix packed in two words (scale = float(w & 0xfffe0000), offset = float(w << 16), a 16-step rotation in four
//!   stolen mantissa bits; `fn@0x009a5470` / `fn@0x009a53b0`).
//!
//! Values from the user's level 05C stream (ff01eabc; the 10B Kong uses the same GEOs and textures):
//! * `S_Kong_Weta_Def` (body):      len 12,  a 2, b 10,   10 layers; fur layer 0x83009e48, words (0xc1360001, 0x41bb0000)
//! * `S_XE_KongBrasG/D` (arms):     len 42,  a 1, b -40,  22 layers; same material as the body
//! * `S_Kong_Weta_Def_TeteHDef01`:  len 15.05, a 2, b 20, 12 layers; face fur layer 0x5600d6bd, words (0xc0023c05, 0xbffc3f00)
//! Per-vertex fur length [C]: the PC fur vertex shader (`vsfur.hlsl`, `fn@0x00a054b0`) does
//! `RLI.a = 1 - RLI.a; pos += normal * g_fFurNormalOffset * RLI.a`, so the length comes from the alpha of the object's
//! RLI array (the per-instance vertex colours of the GAO visual, `RLI\x80` records after each Kong GEO, one u32 per GEO
//! vertex), not from the GEO. Kong's RLI alphas: body 1301 verts (about half long fur, the chest/palms/feet bare),
//! arms 318/376, head 2025 (only the scalp, cheeks and jaw line furred: 12% of the render vertices; the face is bare).
//! `kong/kong_fur_rli.bin` holds that alpha per kong.glb render vertex (kk_extract recipe `kong_fur_rli`).
//! Not ported: the dynamic part (per-vertex velocity buffers, k14..k24 stiffness) and the root shade, which is
//! replaced by a mild darkening of the inner shells [G].

use bevy::asset::embedded_asset;
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

pub type FurMaterial = ExtendedMaterial<StandardMaterial, FurExt>;

#[derive(Clone, Copy, Debug, Default, ShaderType, Reflect)]
pub struct FurUniform {
    pub shell: Vec4,
    pub m: Vec4,
    pub off_shift: Vec4,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct FurExt {
    #[uniform(100)]
    pub fur: FurUniform,
    #[texture(101)]
    #[sampler(102)]
    pub fur_texture: Handle<Image>,
}

impl MaterialExtension for FurExt {
    fn vertex_shader() -> ShaderRef {
        "embedded://kk_fps/kong_fur.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "embedded://kk_fps/kong_fur.wgsl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

/// One fur modifier + its fur layer.
#[derive(Clone, Copy, Debug)]
pub struct FurSpec {
    /// total length (m) = len / 100
    pub len: f32,
    pub layers: u32,
    /// texcoord shift per shell (a / 10000, b / 10000)
    pub shift: Vec2,
    /// fur layer UV matrix (m0, m1, m2, m3) and offset (m4, m5), Jade layout u' = m0 u + m2 v + m4, v' = m1 u + m3 v + m5
    pub m: [f32; 6],
    pub texture: &'static str,
}

/// Decode a multitexture layer's packed UV words (`fn@0x009a5470` + `fn@0x009a53b0`) [C].
pub fn layer_uv_matrix(a: u32, b: u32) -> [f32; 6] {
    const SIN16: [f32; 16] = [
        0.0, 0.382_683_4, 0.707_106_8, 0.923_879_5, 1.0, 0.923_879_5, 0.707_106_8, 0.382_683_4, 0.0, -0.382_683_4, -0.707_106_8,
        -0.923_879_5, -1.0, -0.923_879_5, -0.707_106_8, -0.382_683_4,
    ];
    let special = |w: u32| w == 0 || w == 0x0800_1000;
    let su = if special(a) { 1.0 } else { f32::from_bits(a & 0xfffe_0000) };
    let sv = if special(b) { 1.0 } else { f32::from_bits(b & 0xfffe_0000) };
    let ou = if special(a) { 0.0 } else { f32::from_bits(a << 16) };
    let ov = if special(b) { 0.0 } else { f32::from_bits(b << 16) };
    let idx = ((((b >> 2) & 0x4000) | (a & 0x1_0000)) >> 13 | (a & 1) << 2 | (b & 1)) as usize;
    let (s, c) = (SIN16[idx & 15], SIN16[(idx + 4) & 15]);
    [su * c, -sv * s, su * s, sv * c, ou * su, ov * sv]
}

pub fn spec_for(material_name: &str) -> Option<FurSpec> {
    let body_m = layer_uv_matrix(0xc136_0001, 0x41bb_0000);
    match material_name {
        "kong_body" => Some(FurSpec { len: 0.12, layers: 10, shift: Vec2::new(2.0e-4, 10.0e-4), m: body_m, texture: "kong/fur_detail.png" }),
        "kong_arm_r" | "kong_arm_l" => Some(FurSpec { len: 0.42, layers: 22, shift: Vec2::new(1.0e-4, -40.0e-4), m: body_m, texture: "kong/fur_detail.png" }),
        "kong_head" => Some(FurSpec {
            len: 0.150_513,
            layers: 12,
            shift: Vec2::new(2.0e-4, 20.0e-4),
            m: layer_uv_matrix(0xc002_3c05, 0xbffc_3f00),
            texture: "kong/fur_head.png",
        }),
        _ => None,
    }
}

/// RLI alpha per kong.glb vertex (0 = full-length fur, 255 = none); `None` when the asset is missing.
fn rli_alpha() -> Option<Vec<u8>> {
    std::fs::read(crate::mods::resolve("kong/kong_fur_rli.bin")).ok()
}

/// Copy of a Kong part mesh with the fur length factor `1 - RLI.a` in the vertex colour alpha.
fn with_fur_length(mesh: &Mesh, rli: &[u8]) -> Option<Mesh> {
    let n = mesh.count_vertices();
    if rli.len() < n {
        return None;
    }
    let mut m = mesh.clone();
    let cols: Vec<[f32; 4]> = (0..n).map(|i| [1.0, 1.0, 1.0, 1.0 - rli[i] as f32 / 255.0]).collect();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, cols);
    Some(m)
}

/// Smooth vertex normals from the triangles, welded across UV seams by position (area weighted).
///
/// Kong's body GEO (`S_Kong_Weta_Def`, 1301 verts) carries normals that disagree with its own triangles on 543
/// vertices (median dot with the geometric normal 0.3, the upper torso mostly inverted), while the arm and head
/// GEOs agree (arms 1.0, head 0.96) [C: measured on the user's ff00018c]. With those normals the shells of
/// the body were pushed inside it (no body fur) and the torso lit as if turned away. The game shows a furred,
/// normally lit torso, so the body gets geometric normals here [L]; how the engine treats that array is open.
pub fn weld_smooth_normals(mesh: &mut Mesh) -> bool {
    use bevy::render::mesh::{Indices, VertexAttributeValues};
    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).cloned() else { return false };
    let idx: Vec<usize> = match mesh.indices() {
        Some(Indices::U32(v)) => v.iter().map(|&i| i as usize).collect(),
        Some(Indices::U16(v)) => v.iter().map(|&i| i as usize).collect(),
        None => return false,
    };
    let key = |p: &[f32; 3]| ((p[0] * 1.0e4).round() as i64, (p[1] * 1.0e4).round() as i64, (p[2] * 1.0e4).round() as i64);
    let mut weld: std::collections::HashMap<(i64, i64, i64), usize> = std::collections::HashMap::new();
    let group: Vec<usize> = pos.iter().map(|p| { let n = weld.len(); *weld.entry(key(p)).or_insert(n) }).collect();
    let mut acc = vec![Vec3::ZERO; weld.len()];
    for t in idx.chunks_exact(3) {
        let (a, b, c) = (Vec3::from(pos[t[0]]), Vec3::from(pos[t[1]]), Vec3::from(pos[t[2]]));
        let n = (b - a).cross(c - a);
        for &v in t {
            acc[group[v]] += n;
        }
    }
    let used: std::collections::HashSet<usize> = idx.iter().copied().collect();
    let old = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(VertexAttributeValues::Float32x3(v)) => v.clone(),
        _ => vec![[0.0, 1.0, 0.0]; pos.len()],
    };
    let normals: Vec<[f32; 3]> = (0..pos.len())
        .map(|v| if used.contains(&v) { acc[group[v]].normalize_or(Vec3::from(old[v])).to_array() } else { old[v] })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    if mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_some() {
        let _ = mesh.generate_tangents();
    }
    true
}

pub fn enabled() -> bool {
    std::env::var("KK_NO_FUR").is_err()
}

pub struct KongFurPlugin;

impl Plugin for KongFurPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "kong_fur.wgsl");
        app.add_plugins(MaterialPlugin::<FurMaterial>::default())
            .add_systems(Update, spawn_shells);
    }
}

#[derive(Component)]
pub struct FurShell;

/// Alpha-test reference of the shell at fraction `f` (i / N) of the fur length. The engine uses i / N [C], but
/// with the PC strand texture that leaves the outer half of the coat at 41 %..1 % coverage, which reads as a
/// see-through halo on this renderer (no fur-specific lighting, no blur); the remaster keeps the inner shells
/// solid and tapers later: 0.75 f^1.4 gives 100 % at the root, ~77 % at mid length, ~13 % at the tips [G].
pub fn shell_alpha_ref(f: f32) -> f32 {
    0.75 * f.clamp(0.0, 1.0).powf(1.4)
}

/// Marks a Kong mesh whose shells exist.
#[derive(Component)]
struct FurDone;

fn repeat_loader(s: &mut ImageLoaderSettings) {
    s.is_srgb = false;
    s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
}

#[allow(clippy::type_complexity)]
fn spawn_shells(
    mut commands: Commands,
    server: Res<AssetServer>,
    q: Query<
        (Entity, &Mesh3d, &MeshMaterial3d<StandardMaterial>, Option<&bevy::render::mesh::skinning::SkinnedMesh>, &ChildOf, &Transform, Option<&bevy::gltf::GltfMaterialName>),
        (With<crate::kong::KongMesh>, Without<FurDone>),
    >,
    std_mats: Res<Assets<StandardMaterial>>,
    mut fur_mats: ResMut<Assets<FurMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut rli: Local<Option<Option<Vec<u8>>>>,
) {
    if !enabled() {
        return;
    }
    let rli = rli.get_or_insert_with(|| {
        let r = rli_alpha();
        if r.is_none() {
            warn!("kong/kong_fur_rli.bin missing: Kong's fur covers every vertex (face included)");
        }
        r
    });
    for (e, mesh, mat, skin, parent, tf, mname) in &q {
        let Some(name) = mname.map(|n| n.0.as_str()) else { continue };
        let Some(base) = std_mats.get(&mat.0) else { continue };
        let Some(src_mesh) = meshes.get(&mesh.0) else { continue };
        commands.entity(e).insert(FurDone);
        let Some(spec) = spec_for(name) else { continue };
        let shell_mesh = match rli.as_deref().and_then(|r| with_fur_length(src_mesh, r)) {
            Some(m) => {
                let furred = m.attribute(Mesh::ATTRIBUTE_COLOR).map_or(0, |a| match a {
                    bevy::render::mesh::VertexAttributeValues::Float32x4(v) => v.iter().filter(|c| c[3] > 0.5).count(),
                    _ => 0,
                });
                info!("kong fur: {name} {} shells, {} m, {furred}/{} vertices over half length", spec.layers, spec.len, m.count_vertices());
                meshes.add(m)
            }
            None => {
                info!("kong fur: {name} {} shells, {} m (no RLI mask)", spec.layers, spec.len);
                mesh.0.clone()
            }
        };
        let tex: Handle<Image> = server.load_with_settings(spec.texture, repeat_loader);
        let n = spec.layers.max(1);
        for i in 1..=n {
            let f = i as f32 / n as f32;
            let mut b = base.clone();
            b.alpha_mode = AlphaMode::Mask(0.5);
            // the shell shader writes lit colour directly: keep it forward even when the scene renders deferred
            b.opaque_render_method = bevy::material::OpaqueRendererMethod::Forward;
            b.double_sided = true;
            b.cull_mode = None;
            let m = fur_mats.add(FurMaterial {
                base: b,
                extension: FurExt {
                    fur: FurUniform {
                        // shell offset, alpha ref, inner shells a little darker [G]
                        shell: Vec4::new(spec.len * f, shell_alpha_ref(f), 0.72 + 0.28 * f, 0.0),
                        m: Vec4::new(spec.m[0], spec.m[1], spec.m[2], spec.m[3]),
                        off_shift: Vec4::new(spec.m[4], spec.m[5], spec.shift.x * i as f32, spec.shift.y * i as f32),
                    },
                    fur_texture: tex.clone(),
                },
            });
            let mut ec = commands.spawn((
                Name::new(format!("{name}_fur{i}")),
                FurShell,
                Mesh3d(shell_mesh.clone()),
                MeshMaterial3d(m),
                *tf,
                Visibility::default(),
                NotShadowCaster,
                bevy::camera::visibility::NoFrustumCulling,
                ChildOf(parent.parent()),
            ));
            if let Some(s) = skin {
                ec.insert(s.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_words_decode_like_the_engine() {
        // body fur layer: scale (-11.375, 23.25), rotation index 6 (135 deg)
        let m = layer_uv_matrix(0xc136_0001, 0x41bb_0000);
        let r = |x: f32| (x * 1000.0).round() / 1000.0;
        assert_eq!([r(m[0]), r(m[1]), r(m[2]), r(m[3])], [8.043, -16.44, -8.043, -16.44]);
        // identity words (layer 0 of the same material)
        let id = layer_uv_matrix(0, 0x3f80_0000);
        assert_eq!(id, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    }
}
