//! Overcast sky with the sun peeking through, used as the god-ray source.
//!
//! Data used [C]:
//! * Cloud texture: tex_1f007698 (256x256, tileable), the texture of both of 03E's sky domes
//!   ENV_Ciel and ENV_Ciel2 (extracted to assets/sky/ciel_1f007698.png).
//! * Sun azimuth: the level's only god-ray object LD_03E_GodRay sits at glTF (35, 7.1, -46.4),
//!   straight behind the T-Rex as seen from the player start (34.5, y, -93). The ray
//!   intensity peaks when the camera faces against its ray axis (-0.595, 0, -0.801), i.e. when
//!   the player looks south toward the Rex, which is how the master reference frame is framed
//!   (bright gap above and behind the Rex).
//!
//! Guessed [G]:
//! * Sun elevation (SUN_ELEV_DEG) and the glow falloffs/brightness; the original's sky is an
//!   LDR texture lifted by its after-effects pass, its sun gap is not a separate object.
//! * The god-ray light position follows the visible sun (eye + sun_dir * R) rather than the
//!   GAO's own position, so the rays radiate from the gap the player sees. The AI intensity
//!   formula still uses the GAO's ray axis [C].
//!
//! `KK_OLD_SKY=1` keeps the level's own fogged ENV_Ciel dome and skips this layer.

use crate::anim::GameState;
use crate::godray::GodRaySource;
use crate::player::MainCam;
use crate::world::Arena;
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use bevy::render::mesh::VertexAttributeValues;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::camera::visibility::RenderLayers;

/// Cloud sphere radius (m). Must stay beyond `godray.wgsl`'s SKY_DEPTH distance (400 m).
pub const SKY_RADIUS: f32 = 900.0;
/// [G] sun elevation above the horizon, high enough to peek over the Rex's head
pub const SUN_ELEV_DEG: f32 = 27.0;
/// player start -> LD_03E_GodRay, horizontal [C positions], turned SUN_AZ_OFFSET_DEG to the
/// player's left so the gap peeks beside the Rex's head as in the master frame [G]
const SUN_AZ: Vec2 = Vec2::new(35.0 - 34.5, -46.4 - -93.0);
pub const SUN_AZ_OFFSET_DEG: f32 = 12.0;

pub fn enabled() -> bool {
    std::env::var("KK_OLD_SKY").is_err()
}

pub fn sun_dir() -> Vec3 {
    let a0 = SUN_AZ.normalize();
    let (sn, cs) = SUN_AZ_OFFSET_DEG.to_radians().sin_cos();
    // rotate about +Y; facing +Z the player's left is +X
    let az = Vec2::new(a0.x * cs + a0.y * sn, -a0.x * sn + a0.y * cs);
    let e = SUN_ELEV_DEG.to_radians();
    Vec3::new(az.x * e.cos(), e.sin(), az.y * e.cos()).normalize()
}

#[derive(Component)]
pub struct CloudSky;
#[derive(Component)]
pub struct SunDisc;

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Playing), spawn_sky)
            .add_systems(PostUpdate, follow_camera.before(TransformSystems::Propagate).run_if(in_state(GameState::Playing)));
    }
}

fn srgb_lin(c: [f32; 3]) -> Vec3 {
    let f = |x: f32| if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) };
    Vec3::new(f(c[0]), f(c[1]), f(c[2]))
}

/// Per-vertex sky colour (linear, HDR): horizon haze -> darker zenith, plus the sun gap.
fn sky_color(v: Vec3, sun: Vec3) -> Vec3 {
    // the cloud texture's mean is ~0.25 linear; these gains bring it to the fog/haze colour
    // the rest of the frame fades into (atmos::FOG_RGB) [G, tuned to the master frame]
    let fog = srgb_lin([160.0 / 255.0, 204.0 / 255.0, 206.0 / 255.0]) / 0.25;
    let zen = fog * Vec3::new(0.70, 0.74, 0.76);
    let h = v.y.clamp(0.0, 1.0).powf(0.6);
    let base = fog.lerp(zen, h);
    let g = v.dot(sun).max(0.0);
    // sun gap: thin cloud brightening toward the sun (wide), and a hot core (narrow)
    let glow = 0.35 * g.powf(4.0) + 1.1 * g.powf(24.0) + 3.0 * g.powf(160.0);
    let warm = Vec3::new(1.0, 0.98, 0.9);
    base * (1.0 + glow * warm)
}

/// Soft radial disc for the sun core (white, alpha falloff).
fn disc_image() -> Image {
    let n = 128u32;
    let mut data = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let dx = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let dy = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let r = (dx * dx + dy * dy).sqrt();
            let a = (1.0 - r).clamp(0.0, 1.0).powf(2.5) * 0.6 + (1.0 - r / 0.25).clamp(0.0, 1.0).powf(1.5) * 0.4;
            data.extend_from_slice(&[255, 255, 255, (a.min(1.0) * 255.0) as u8]);
        }
    }
    Image::new(
        Extent3d { width: n, height: n, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn spawn_sky(
    mut commands: Commands,
    arena: Res<Arena>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    if !enabled() || arena.level.is_none() || crate::scene::swamp() {
        return;
    }
    let sun = sun_dir();
    let mut mesh = Sphere::new(SKY_RADIUS).mesh().uv(128, 64);
    let normals: Vec<Vec3> = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(p)) => p.iter().map(|p| Vec3::from(*p).normalize()).collect(),
        _ => vec![],
    };
    let colors: Vec<[f32; 4]> = normals.iter().map(|v| sky_color(*v, sun).extend(1.0).to_array()).collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    let tex: Handle<Image> = assets.load_with_settings("sky/ciel_1f007698.png", |s: &mut ImageLoaderSettings| {
        s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            ..ImageSamplerDescriptor::linear()
        });
    });
    let mat = mats.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(tex),
        // tile the 256px cloud texture: 5x around, 3x pole to pole [G]
        uv_transform: bevy::math::Affine2::from_scale(Vec2::new(5.0, 3.0)),
        unlit: true,
        fog_enabled: false,
        cull_mode: None,
        ..default()
    });
    commands.spawn((
        Name::new("CloudSky"),
        CloudSky,
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(mat),
        Transform::default(),
        NotShadowCaster,
        NotShadowReceiver,
        RenderLayers::layer(0),
        bevy::camera::visibility::NoFrustumCulling,
    ));
    // sun core peeking through the clouds: additive HDR disc in front of the sphere
    let disc = mats.add(StandardMaterial {
        base_color: Color::linear_rgb(5.0, 4.8, 4.2),
        base_color_texture: Some(images.add(disc_image())),
        unlit: true,
        fog_enabled: false,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        ..default()
    });
    commands.spawn((
        Name::new("SunDisc"),
        SunDisc,
        Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial3d(disc),
        Transform::from_scale(Vec3::splat(SKY_RADIUS * 0.16)),
        NotShadowCaster,
        NotShadowReceiver,
        RenderLayers::layer(0),
        bevy::camera::visibility::NoFrustumCulling,
    ));
}

fn follow_camera(
    cam: Query<&GlobalTransform, With<MainCam>>,
    mut sky: Query<&mut Transform, (With<CloudSky>, Without<SunDisc>)>,
    mut disc: Query<&mut Transform, (With<SunDisc>, Without<CloudSky>)>,
    mut src: Query<&mut GodRaySource>,
) {
    let Ok(gt) = cam.single() else { return };
    let eye = gt.translation();
    let sun = sun_dir();
    for mut t in &mut sky {
        t.translation = eye;
    }
    for mut t in &mut disc {
        t.translation = eye + sun * (SKY_RADIUS * 0.94);
        t.look_at(eye, Vec3::Y);
    }
    if enabled() {
        for mut s in &mut src {
            s.pos = eye + sun * (SKY_RADIUS * 0.94);
        }
    }
}
