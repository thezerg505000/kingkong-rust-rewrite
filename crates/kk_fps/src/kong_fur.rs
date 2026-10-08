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
//! The per-vertex length scale (vertex colour alpha) is unused: Kong's GEOs carry no colours / alpha 255 [C].
//! Not ported: the dynamic part (per-vertex velocity buffers, k14..k24 stiffness) and the root shade, which is
//! replaced by a mild darkening of the inner shells [G].

use bevy::asset::embedded_asset;
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, NotShadowCaster};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderRef, ShaderType};

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

pub fn enabled() -> bool {
    std::env::var("KK_NO_FUR").is_err()
}

pub struct KongFurPlugin;

impl Plugin for KongFurPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "kong_fur.wgsl");
        app.add_plugins(MaterialPlugin::<FurMaterial> { prepass_enabled: false, shadows_enabled: false, ..default() })
            .add_systems(Update, spawn_shells);
    }
}

#[derive(Component)]
pub struct FurShell;

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
) {
    if !enabled() {
        return;
    }
    for (e, mesh, mat, skin, parent, tf, mname) in &q {
        let Some(name) = mname.map(|n| n.0.as_str()) else { continue };
        let Some(base) = std_mats.get(&mat.0) else { continue };
        commands.entity(e).insert(FurDone);
        let Some(spec) = spec_for(name) else { continue };
        let tex: Handle<Image> = server.load_with_settings(spec.texture, repeat_loader);
        let n = spec.layers.max(1);
        for i in 1..=n {
            let f = i as f32 / n as f32;
            let mut b = base.clone();
            b.alpha_mode = AlphaMode::Mask(0.5);
            b.double_sided = true;
            b.cull_mode = None;
            let m = fur_mats.add(FurMaterial {
                base: b,
                extension: FurExt {
                    fur: FurUniform {
                        // shell offset, alpha ref i/N, inner shells a little darker [G]
                        shell: Vec4::new(spec.len * f, f, 0.72 + 0.28 * f, 0.0),
                        m: Vec4::new(spec.m[0], spec.m[1], spec.m[2], spec.m[3]),
                        off_shift: Vec4::new(spec.m[4], spec.m[5], spec.shift.x * i as f32, spec.shift.y * i as f32),
                    },
                    fur_texture: tex.clone(),
                },
            });
            let mut ec = commands.spawn((
                Name::new(format!("{name}_fur{i}")),
                FurShell,
                Mesh3d(mesh.0.clone()),
                MeshMaterial3d(m),
                *tf,
                Visibility::default(),
                NotShadowCaster,
                bevy::render::view::NoFrustumCulling,
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
