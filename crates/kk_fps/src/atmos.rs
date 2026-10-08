//! Level 03E atmosphere: fog, moonlight and light shafts, from the level's own data.
//!
//! Sources (research/pc/atmos/ATMOS_FINDINGS.md, atmos_03e.json):
//! * Fog [C]: Jade linear fog `f = sat((viewZ - near) / (far - near)) * density`. The level's
//!   fog object gives near 1, density 1; LD_03E_ChangeFog (crossed before the T-Rex arena)
//!   switches it to colour dword 0x002f2f25 = RGB(37,47,47) (Jade colours are 0xAABBGGRR,
//!   checked on the muzzle-flash colour 0x0060a7ff = orange) and the far zone to 150 m.
//! * Moonlight [C colour]: every moon light record is RGB(151,180,186). Positions are the
//!   level's Xe_Light_Spot_Moon_* GAOs [C]; Jade lights shine down their local -Y axis [L:
//!   inferred, it aims Arene_T_Rex03 at the arena centre]; cone angles from the spot light
//!   records [C values, record-to-GAO pairing G]; intensities are [G].
//! * Light shafts: the original draws camera-aligned slices inside each spot's frustum,
//!   modulated by a cookie and two scrolling noise textures and the shadow map, then blurs
//!   and adds them (pslightshaft.hlsl). Bevy's volumetric fog is the same idea done by
//!   ray-marching: shadowed spot/directional lights scattering in a fog volume whose density
//!   is a scrolling 3D noise texture. Density and scattering are [G].

use crate::anim::GameState;
use crate::player::MainCam;
use crate::world::{Arena, VIEW_LAYER};
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{FogVolume, VolumetricFog};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::view::RenderLayers;

/// Recovered fog colours: zone fog RGB(93,93,93), zone ambient2 RGB(119,119,119), ChangeFog
/// RGB(37,47,47) [C]. Which zone covers the arena is not recovered, and the original's PC
/// after-effects pass brightens the frame (reference screenshots show a pale grey-green haze),
/// so the colour used is tuned to the reference shots [G]; distances stay recovered.
pub const FOG_RGB: [u8; 3] = [160, 204, 206];
/// fog near / far of the level's wide zone (zone 2: near 1, far 105) [C]
pub const FOG_NEAR: f32 = 1.0;
// the level's wide fog zone (zone 2: near 1, far 105) [C]
pub const FOG_FAR: f32 = 105.0;
/// moon light colour [C]
pub const MOON_RGB: [u8; 3] = [151, 180, 186];

pub fn fog_color() -> Color {
    Color::srgb_u8(FOG_RGB[0], FOG_RGB[1], FOG_RGB[2])
}
pub fn moon_color() -> Color {
    Color::srgb_u8(MOON_RGB[0], MOON_RGB[1], MOON_RGB[2])
}

/// Moon spots around the T-Rex arena: (GAO name, position, local Y axis row) [C],
/// (range, inner, outer cone) from the spot records [C values, pairing G].
const MOON_SPOTS: [(&str, [f32; 3], [f32; 3], f32, f32, f32); 3] = [
    ("Xe_Light_Spot_Moon_Arene_T_Rex03", [30.2, 21.58, -98.29], [-0.328, 0.608, -0.723], 60.0, 0.386, 0.61),
    ("Xe_Light_Spot_Moon_Arene_T_Rex", [36.58, 8.14, -98.84], [0.059, -0.058, 0.997], 40.0, 0.226, 0.982),
    ("Xe_Light_Spot_Moon_Start", [36.33, 10.31, -85.71], [0.0, 0.0, -1.0], 40.0, 0.729, 1.199),
];

#[derive(Component)]
pub struct MistVolume;

pub struct AtmosPlugin;

impl Plugin for AtmosPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Playing), spawn_atmos)
            .add_systems(Update, (camera_fog, scroll_mist).run_if(in_state(GameState::Playing)));
    }
}

fn level(arena: &Arena) -> bool {
    arena.level.is_some() && !crate::scene::swamp()
}

/// Shafts need shadow maps and Bevy's volumetric pass, which cannot be translated to GLSL
/// (software-GL test runs); KK_NO_SHAFTS also turns them off on slow machines.
pub fn shafts_enabled() -> bool {
    std::env::var("KK_NO_SHAFTS").is_err() && std::env::var("KK_SOFTWARE_GL").is_err()
}

/// Tileable 3D value noise (two octaves), the stand-in for the shaft shader's two scrolling
/// noise textures.
fn noise3d(n: u32, seed: u32) -> Image {
    let hash = |x: i32, y: i32, z: i32, p: i32| -> f32 {
        let (x, y, z) = (x.rem_euclid(p), y.rem_euclid(p), z.rem_euclid(p));
        let mut h = (x as u32).wrapping_mul(374761393)
            ^ (y as u32).wrapping_mul(668265263)
            ^ (z as u32).wrapping_mul(2147483647)
            ^ seed.wrapping_mul(1274126177);
        h = (h ^ (h >> 13)).wrapping_mul(1274126177);
        ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
    };
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let octave = |x: f32, y: f32, z: f32, p: i32| -> f32 {
        let (ix, iy, iz) = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
        let (fx, fy, fz) = (smooth(x.fract()), smooth(y.fract()), smooth(z.fract()));
        let mut acc = 0.0;
        for dz in 0..2 {
            for dy in 0..2 {
                for dx in 0..2 {
                    let w = (if dx == 1 { fx } else { 1.0 - fx })
                        * (if dy == 1 { fy } else { 1.0 - fy })
                        * (if dz == 1 { fz } else { 1.0 - fz });
                    acc += w * hash(ix + dx, iy + dy, iz + dz, p);
                }
            }
        }
        acc
    };
    let mut data = Vec::with_capacity((n * n * n) as usize);
    for z in 0..n {
        for y in 0..n {
            for x in 0..n {
                let s = |p: i32| (x as f32 / n as f32 * p as f32, y as f32 / n as f32 * p as f32, z as f32 / n as f32 * p as f32);
                let (a, b, c) = s(4);
                let (d, e, f) = s(8);
                let v = octave(a, b, c, 4) * 0.65 + octave(d, e, f, 8) * 0.35;
                // patchy: thin areas between banks of mist
                let v = ((v - 0.3) / 0.55).clamp(0.0, 1.0);
                data.push((v * 255.0) as u8);
            }
        }
    }
    let mut img = Image::new(
        Extent3d { width: n, height: n, depth_or_array_layers: n },
        TextureDimension::D3,
        data,
        TextureFormat::R8Unorm,
        RenderAssetUsages::default(),
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    img
}

fn spawn_atmos(mut commands: Commands, arena: Res<Arena>, mut images: ResMut<Assets<Image>>, mut ambient: ResMut<AmbientLight>, mut clear: ResMut<ClearColor>) {
    if !level(&arena) {
        return;
    }
    let shafts = shafts_enabled();
    clear.0 = fog_color();
    // Zone ambient is a dim neutral grey (16-19 /255) [C]; Bevy has no per-object lightmaps
    // (the original bakes RLI vertex light), so the level fill is raised to compensate [G].
    *ambient = AmbientLight { color: Color::srgb(0.95, 0.92, 0.82), brightness: 220.0, ..default() };

    // Moon: the main key light, along Arene_T_Rex03's -Y (high spot aimed into the arena)
    let dir = -Vec3::new(-0.328, 0.608, -0.723);
    let mut moon = commands.spawn((
        Name::new("Moon"),
        // key light neutral: the reference shots are overcast daylight; the moon colour stays on
        // the level's own spot lights [G]
        // colour of 03E's only level-local directional light, record 08001b3e: RGB(169,186,184) [C]
        DirectionalLight { illuminance: 950.0, shadows_enabled: false, color: Color::srgb_u8(169, 186, 184), ..default() },
        Transform::default().looking_to(dir, Vec3::Y),
        RenderLayers::layer(0),
        bevy::pbr::CascadeShadowConfigBuilder { num_cascades: 2, maximum_distance: 70.0, ..default() }.build(),
    ));
    if shafts {
        // 03E has no shaft lights: the key light stays non-volumetric and shadowless
        // (the reference frames show flat, diffuse overcast light) [C/L]
        let _ = &mut moon;
    }
    for (name, pos, y_row, range, inner, outer) in MOON_SPOTS {
        let d = -Vec3::from(y_row);
        let p = Vec3::from(pos);
        // Bevy's outer angle is the half-angle and must stay below pi/2
        let outer = (outer * 0.5).min(1.2);
        let inner = (inner * 0.5).min(outer * 0.95);
        let mut e = commands.spawn((
            Name::new(name),
            SpotLight {
                intensity: 2.0e4,
                range,
                color: moon_color(),
                shadows_enabled: false,
                inner_angle: inner,
                outer_angle: outer,
                ..default()
            },
            Transform::from_translation(p).looking_to(d, if d.y.abs() > 0.95 { Vec3::Z } else { Vec3::Y }),
            RenderLayers::layer(0),
        ));
        // no VolumetricLight here: the engine only draws shafts for type-5 lights
        // (fn@0x00a186c0) and none of the 49 lights in 03E is type 5 [C]
        let _ = &mut e;
    }
    // the level's screen-space god ray source (LD_03E_GodRay.gao) [C]
    commands.spawn((
        Name::new("LD_03E_GodRay"),
        crate::godray::GodRaySource { pos: crate::godray::GODRAY_POS, ray: crate::godray::GODRAY_RAY },
        Transform::from_translation(crate::godray::GODRAY_POS),
    ));
    // sky fill straight down in the level sky-light colour (record 08001b3e): stands in for the
    // original's hemisphere/RLI sky term so the Rex gets a top-to-belly gradient [G]
    commands.spawn((
        Name::new("SkyFill"),
        DirectionalLight { illuminance: 700.0, shadows_enabled: false, color: Color::srgb_u8(169, 186, 184), ..default() },
        Transform::default().looking_to(Vec3::new(0.05, -1.0, 0.1), Vec3::Z),
        RenderLayers::layer(0),
    ));
    if shafts {
        // mist bank over the courtyard and the T-Rex passage (DEC_*Brume* sprites sit here)
        let tex = images.add(noise3d(32, 3));
        commands.spawn((
            Name::new("MistVolume"),
            MistVolume,
            FogVolume {
                fog_color: Color::srgb(0.85, 0.87, 0.82),
                density_factor: 0.02,
                density_texture: Some(tex),
                absorption: 0.15,
                scattering: 0.35,
                scattering_asymmetry: 0.6,
                light_tint: Color::WHITE,
                light_intensity: 1.0,
                ..default()
            },
            Transform::from_xyz(35.0, 11.0, -78.0).with_scale(Vec3::new(70.0, 22.0, 90.0)),
        ));
    }
}

/// Give the world camera the recovered fog (and the volumetric pass) once it exists.
fn camera_fog(mut commands: Commands, arena: Res<Arena>, cams: Query<Entity, Added<MainCam>>) {
    if !level(&arena) {
        return;
    }
    for e in &cams {
        let mut c = commands.entity(e);
        // grade toward the reference shots' warmer, less saturated grey-green [G]
        c.insert(bevy::render::view::ColorGrading {
            global: bevy::render::view::ColorGradingGlobal { temperature: 0.01, post_saturation: 1.0, ..default() },
            ..default()
        });
        c.insert(DistanceFog {
            color: fog_color(),
            // Jade uses linear fog (near 1, far 105) [C]; the master frame shows a clear near field
            // and fast far wash-out, matched with exponential-squared density 0.02 [G]
            falloff: FogFalloff::ExponentialSquared { density: 0.013 },
            ..default()
        });
        if crate::godray::enabled() {
            c.insert((crate::godray::GodRay::default(), Msaa::Off));

        }
        if shafts_enabled() {
            // bloom needs a renderable Rg11b10 target (not on software GL)
            c.insert(bevy::core_pipeline::bloom::Bloom {
                intensity: 0.2,
                prefilter: bevy::core_pipeline::bloom::BloomPrefilter { threshold: 0.35, threshold_softness: 0.4 },
                ..bevy::core_pipeline::bloom::Bloom::NATURAL
            });
            c.insert(VolumetricFog { ambient_color: Color::srgb(0.80, 0.84, 0.78), ambient_intensity: 0.05, jitter: 0.0, step_count: 48 });
        }
    }
    let _ = VIEW_LAYER;
}

/// Scroll the mist noise like the shader's two scrolling noise layers [G speed].
fn scroll_mist(time: Res<Time>, mut q: Query<&mut FogVolume, With<MistVolume>>) {
    for mut v in &mut q {
        let t = time.elapsed_secs();
        v.density_texture_offset = Vec3::new(t * 0.006, t * 0.002, t * 0.004);
    }
}
