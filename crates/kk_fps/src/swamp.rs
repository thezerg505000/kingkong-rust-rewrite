//! Level 07D "Kong Saves Ann" swamp scene (`KK_SCENE=swamp07d`, set by the `b10*` batches).
//!
//! The reference clip is a flooded, rain-lashed swamp under a heavy grey-green fog. The level data gives:
//! * fog LGT_FOG RGB(140,143,129), near 0, far 140 [C] (`atmos_07d.json`), zone ambient (39,48,43) and
//!   (78,96,93) [C], key LGT_Front_map (166,159,132), fill LGT_back_map (68,66,45) [C colours];
//! * water: flat quads Xe_Water_Arene / Arene02 at y = -1.651 and the 87x73 m plane 7D_DEC_Part5_Water at
//!   -1.6 [C geometry], materials tagged `water`; the floor under them is -1..-3 m, i.e. ankle to knee deep;
//! * no rain emitter record (shared SFX_RainSnow actor driven by scripts): screen-space rain [G].
//!
//! Everything about presentation here is `[G]`, tuned against the reference frames: fog colour and density,
//! light levels, the water surface (dark, slightly murky, reflective, scrolling ripple normals), low mist
//! cards drifting over the water, Ann (untextured, headless export: plain skin + cloth + a head ball).

use crate::anim::GameState;
use crate::fx::FxAssets;
use crate::player::MainCam;
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::camera::visibility::RenderLayers;
use rand::Rng;

/// 07D fog colour: LGT_FOG (140,143,129) [C], brightened by the PC after-effects pass like 03E's [G].
pub const FOG_RGB_07D: [f32; 3] = [0.50, 0.57, 0.53];
/// 05C marsh fog: LD_Changefog zone 2 (39,45,43) near 1 far 100 and zone 1 (64,68,65) near 10 far 150 [C];
/// 05C_LGT_FOG (104,112,106) near 7 far 65 [C] (arena 1). The PC after-effects brighten the frame: colour tuned
/// to the reference clip's marsh frames [G].
pub const FOG_RGB_05C: [f32; 3] = [0.38, 0.45, 0.39];
pub const FOG_DENSITY: f32 = 0.016;

pub fn fog_rgb() -> [f32; 3] {
    if crate::scene::marsh05c() { env3("KK_FOG_RGB").unwrap_or(FOG_RGB_05C) } else { FOG_RGB_07D }
}

pub fn fog_density() -> f32 {
    let d = if crate::scene::marsh05c() { 0.024 } else { FOG_DENSITY };
    std::env::var("KK_FOG_DENSITY").ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

fn env3(k: &str) -> Option<[f32; 3]> {
    let v: Vec<f32> = std::env::var(k).ok()?.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (v.len() == 3).then(|| [v[0], v[1], v[2]])
}

/// Water plane height of the flooded area [C]: 07D Xe_Water_Arene -1.651; 05C DEC_05C_D_marais_eau -38.7
/// (marsh floor DEC_05C_D_solmarais_* -39.6..-37.0, i.e. ankle deep).
pub fn water_y() -> f32 {
    if crate::scene::marsh05c() { -38.7 } else { -1.651 }
}
/// RLI-baked vertex colours multiplier of the level meshes (see world.rs for 03E) [G].
pub const LEVEL_RGB_SCALE: f32 = 0.42;

#[derive(Component)]
pub struct WaterSurface;
#[derive(Component)]
pub struct MistCard {
    base: Vec3,
    phase: f32,
    alpha: f32,
}
#[derive(Component)]
pub struct Ann;
/// A level mesh that may be hidden while it stands between the camera and the fighters.
#[derive(Component)]
pub struct LevelProp;
#[derive(Component)]
pub struct HiddenByCamera;

#[derive(Resource, Default)]
pub struct SwampWater {
    pub materials: Vec<Handle<StandardMaterial>>,
}

pub struct SwampPlugin;

impl Plugin for SwampPlugin {
    fn build(&self, app: &mut App) {
        if !crate::scene::swamp() {
            return;
        }
        app.init_resource::<SwampWater>()
            .insert_resource(ClearColor(fog_color()))
            .add_systems(OnEnter(GameState::Playing), spawn_env)
            .add_systems(
                Update,
                (camera_setup, animate_water, drift_mist, ripples, populate_swamp).run_if(in_state(GameState::Playing)),
            )
            // after this frame's camera move has propagated, before visibility: no one-frame lag on camera cuts
            .add_systems(
                PostUpdate,
                occluder_hide
                    .run_if(in_state(GameState::Playing))
                    .after(bevy::transform::TransformSystems::Propagate)
                    .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
            );
    }
}

fn fog_color() -> Color {
    let c = fog_rgb();
    Color::srgb(c[0], c[1], c[2])
}

fn spawn_env(mut commands: Commands, mut ambient: ResMut<GlobalAmbientLight>) {
    if crate::scene::marsh05c() {
        // 05C sector 7 (marsh): zone-2 ambient (63,74,74) [C colour]; key Xe_light_Spot_s07_Main (131,143,137) at
        // (154.8, 11.7, -232.7) aimed down over the marsh [C colour/position, direction L]; omnis s07 (118,134,126) [C]
        *ambient = GlobalAmbientLight { color: Color::srgb_u8(63, 74, 74), brightness: 1300.0, ..default() };
        commands.spawn((
            Name::new("SwampKey"),
            DirectionalLight { illuminance: 2400.0, shadow_maps_enabled: false, color: Color::srgb_u8(131, 143, 137), ..default() },
            Transform::default().looking_to(Vec3::new(-24.8, -49.7, 42.7), Vec3::Y),
            RenderLayers::layer(0),
        ));
        commands.spawn((
            Name::new("SwampFill"),
            DirectionalLight { illuminance: 600.0, shadow_maps_enabled: false, color: Color::srgb_u8(118, 134, 126), ..default() },
            Transform::default().looking_to(Vec3::new(0.5, -0.4, -0.6), Vec3::Y),
            RenderLayers::layer(0),
        ));
        return;
    }
    *ambient = GlobalAmbientLight { color: Color::srgb(0.70, 0.82, 0.78), brightness: 380.0, ..default() };
    // key LGT_Front_map (166,159,132) [C colour], high and a little behind the fight [G direction]
    commands.spawn((
        Name::new("SwampKey"),
        DirectionalLight { illuminance: 1500.0, shadow_maps_enabled: false, color: Color::srgb_u8(166, 159, 132), ..default() },
        Transform::default().looking_to(Vec3::new(-0.35, -0.85, -0.4), Vec3::Y),
        RenderLayers::layer(0),
    ));
    // fill LGT_back_map (68,66,45) from the opposite side [C colour]
    commands.spawn((
        Name::new("SwampFill"),
        DirectionalLight { illuminance: 700.0, shadow_maps_enabled: false, color: Color::srgb(0.58, 0.68, 0.66), ..default() },
        Transform::default().looking_to(Vec3::new(0.4, -0.5, 0.6), Vec3::Y),
        RenderLayers::layer(0),
    ));
}

/// Fog, grading and tone on the world camera once it exists.
fn camera_setup(mut commands: Commands, cams: Query<Entity, Added<MainCam>>) {
    for e in &cams {
        let mut c = commands.entity(e);
        c.insert(DistanceFog {
            color: fog_color(),
            falloff: FogFalloff::ExponentialSquared { density: fog_density() },
            ..default()
        });
        c.insert(bevy::render::view::ColorGrading {
            global: if crate::scene::marsh05c() {
                // the clip's marsh is a warmer, greener grey than the raw zone colours (PC after-effects) [G]
                bevy::render::view::ColorGradingGlobal { temperature: 0.0, tint: -0.008, post_saturation: 0.85, exposure: 0.62, ..default() }
            } else {
                bevy::render::view::ColorGradingGlobal { temperature: -0.02, post_saturation: 0.82, ..default() }
            },
            midtones: bevy::render::view::ColorGradingSection { contrast: if crate::scene::marsh05c() { 1.15 } else { 1.0 }, ..default() },
            ..default()
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Level scene post-processing
// ---------------------------------------------------------------------------------------------

/// Tileable value-noise normal map for the water ripples.
fn ripple_normal_image() -> Image {
    const N: usize = 128;
    let mut rng: rand::rngs::StdRng = rand::SeedableRng::seed_from_u64(11);
    let lattice: Vec<f32> = (0..16 * 16).map(|_| rng.gen()).collect();
    let height = |x: f32, y: f32| -> f32 {
        let mut acc = 0.0;
        for (o, amp) in [(4.0f32, 1.0f32), (8.0, 0.5), (16.0, 0.25)] {
            let (fx, fy) = (x * o, y * o);
            let (ix, iy) = (fx.floor() as i32, fy.floor() as i32);
            let (tx, ty) = (fx.fract(), fy.fract());
            let (tx, ty) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
            let p = o as i32;
            let l = |i: i32, j: i32| lattice[((j.rem_euclid(p) * 16 / p.max(1)) as usize % 16) * 16 + ((i.rem_euclid(p) * 16 / p.max(1)) as usize % 16)];
            let a = l(ix, iy) * (1.0 - tx) + l(ix + 1, iy) * tx;
            let b = l(ix, iy + 1) * (1.0 - tx) + l(ix + 1, iy + 1) * tx;
            acc += (a * (1.0 - ty) + b * ty) * amp;
        }
        acc
    };
    let mut data = Vec::with_capacity(N * N * 4);
    for j in 0..N {
        for i in 0..N {
            let (x, y) = (i as f32 / N as f32, j as f32 / N as f32);
            let e = 1.0 / N as f32;
            let dx = height(x + e, y) - height(x - e, y);
            let dy = height(x, y + e) - height(x, y - e);
            let n = Vec3::new(-dx * 6.0, -dy * 6.0, 1.0).normalize();
            data.extend_from_slice(&[((n.x * 0.5 + 0.5) * 255.0) as u8, ((n.y * 0.5 + 0.5) * 255.0) as u8, ((n.z * 0.5 + 0.5) * 255.0) as u8, 255]);
        }
    }
    let mut img = Image::new(
        Extent3d { width: N as u32, height: N as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    img
}

/// Observer on the level scene.
pub fn on_level_ready(
    trigger: On<bevy::world_serialization::WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    names: Query<&Name>,
    q: Query<&MeshMaterial3d<StandardMaterial>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut water: ResMut<SwampWater>,
) {
    let normal = images.add(ripple_normal_image());
    // one shared water material: dark, murky, slightly reflective, ripples scrolled by `animate_water`
    let water_mat = mats.add(StandardMaterial {
        base_color: if crate::scene::marsh05c() { Color::srgba(0.20, 0.28, 0.24, 0.78) } else { Color::srgba(0.13, 0.20, 0.18, 0.80) },
        perceptual_roughness: if crate::scene::marsh05c() { 0.6 } else { 0.10 },
        reflectance: if crate::scene::marsh05c() { 0.12 } else { 0.7 },
        metallic: 0.0,
        alpha_mode: AlphaMode::Blend,
        normal_map_texture: Some(normal),
        double_sided: true,
        cull_mode: None,
        uv_transform: bevy::math::Affine2::from_scale(Vec2::splat(0.35)),
        ..default()
    });
    water.materials.push(water_mat.clone());
    let mut done = std::collections::HashSet::new();
    let (mut n_water, mut n_hidden) = (0, 0);
    for e in children.iter_descendants(trigger.entity) {
        let name = names.get(e).map(|n| n.as_str().to_string()).unwrap_or_default();
        let lower = name.to_lowercase();
        let hide = lower.contains("fog") || lower.contains("faisceau") || lower.contains("brume") || lower.contains("cascade")
            || lower.contains("ld_sky") || lower.contains("ocl_") || lower.contains("wf_")
            || (lower.contains("ode_") && !lower.contains("yucca"));
        if hide {
            commands.entity(e).insert(Visibility::Hidden);
            n_hidden += 1;
            if std::env::var("KK_SWAMP_DEBUG").is_ok() {
                info!("swamp hides {name}");
            }
            continue;
        }
        if std::env::var("KK_SWAMP_TINT").is_ok() {
            if let Ok(h) = q.get(e) {
                let hsh = name.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
                let col = Color::hsl((hsh % 360) as f32, 1.0, 0.5);
                info!("swamp tint {name} hue {}", hsh % 360);
                let m = mats.add(StandardMaterial { base_color: col, unlit: true, ..default() });
                commands.entity(e).insert(MeshMaterial3d(m));
            }
            continue;
        }
        if q.get(e).is_ok() && !lower.contains("water") && !lower.contains("_eau") && !lower.contains("ecume") {
            commands.entity(e).insert(LevelProp);
        }
        let is_water = (lower.contains("water") || lower.contains("_eau")) && !lower.contains("waterfall") && !lower.contains("ecume");
        if is_water {
            if q.get(e).is_ok() {
                commands.entity(e).insert((MeshMaterial3d(water_mat.clone()), WaterSurface, NotShadowCaster));
                n_water += 1;
            }
            continue;
        }
        if lower.contains("ecume") {
            // foam strips: leave their texture, additive-ish blend is not decoded; hide the flat quads that z-fight the water
            commands.entity(e).insert(Visibility::Hidden);
            continue;
        }
        if let Ok(h) = q.get(e) {
            if !done.insert(h.0.id()) {
                continue;
            }
            if let Some(mut m) = mats.get_mut(&h.0) {
                if m.alpha_mode == AlphaMode::Blend && m.base_color_texture.is_none() {
                    continue;
                }
                m.perceptual_roughness = 1.0;
                m.reflectance = 0.08;
                // Jade draws level geometry without back-face culling (the basin walls are single-sided shells seen
                // from either side); Bevy culls back faces by default and the walls vanished [L]
                m.cull_mode = None;
                m.double_sided = true;
                let c = m.base_color.to_linear();
                let k = std::env::var("KK_LEVEL_RGB").ok().and_then(|v| v.parse().ok()).unwrap_or(if crate::scene::marsh05c() { 1.0 } else { LEVEL_RGB_SCALE });
                m.base_color = Color::LinearRgba(LinearRgba::new(c.red * k, c.green * k, c.blue * k, c.alpha));
            }
        }
    }
    info!("swamp level ready: {n_water} water meshes, {n_hidden} hidden fx/ode/sky meshes");
}

/// Ripples: scroll the shared normal map in two directions over time [G].
fn animate_water(time: Res<Time>, water: Res<SwampWater>, mut mats: ResMut<Assets<StandardMaterial>>) {
    let t = time.elapsed_secs();
    for h in &water.materials {
        if let Some(mut m) = mats.get_mut(h) {
            m.uv_transform = bevy::math::Affine2::from_scale_angle_translation(Vec2::splat(0.30), 0.0, Vec2::new(t * 0.020, t * 0.013));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Mist
// ---------------------------------------------------------------------------------------------

/// Low mist cards drifting over the water around the fight (the level's own fog cards cannot be used: their
/// edge fade lives in vertex data the exporter drops).
pub fn spawn_mist(commands: &mut Commands, fx: &FxAssets, mats: &mut Assets<StandardMaterial>, centre: Vec3) {
    let mut rng = rand::thread_rng();
    for i in 0..46 {
        let a = rng.gen_range(0.0..std::f32::consts::TAU);
        let r = rng.gen_range(10.0..44.0);
        let base = Vec3::new(centre.x + a.cos() * r, water_y() + rng.gen_range(0.6..3.2), centre.z + a.sin() * r);
        let s = rng.gen_range(14.0..30.0);
        let a_card: f32 = rng.gen_range(0.08..0.18);
        let m = mats.add(StandardMaterial {
            base_color: Color::srgba(0.78, 0.86, 0.82, a_card),
            base_color_texture: Some(fx.smoke.clone()),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            fog_enabled: true,
            ..default()
        });
        commands.spawn((
            Name::new("MistCard"),
            MistCard { base, phase: i as f32 * 1.7, alpha: a_card },
            Mesh3d(fx.quad.clone()),
            MeshMaterial3d(m),
            Transform::from_translation(base).with_scale(Vec3::new(s, s * 0.35, 1.0)),
            NotShadowCaster,
            NotShadowReceiver,
            RenderLayers::layer(0),
        ));
    }
}

fn drift_mist(time: Res<Time>, cam: Query<&GlobalTransform, With<MainCam>>, mut q: Query<(&MistCard, &mut Transform, &MeshMaterial3d<StandardMaterial>), Without<MainCam>>, mut mats: ResMut<Assets<StandardMaterial>>) {
    let t = time.elapsed_secs();
    let Ok(cam) = cam.single() else { return };
    let cam_rot = cam.compute_transform().rotation;
    for (m, mut tf, mh) in &mut q {
        tf.translation = m.base + Vec3::new((t * 0.18 + m.phase).sin() * 2.5, (t * 0.11 + m.phase).sin() * 0.25, (t * 0.14 + m.phase * 0.7).cos() * 2.5);
        // vertical axis billboard: cards stay upright and turn toward the camera
        let to = cam.translation() - tf.translation;
        let yaw = to.x.atan2(to.z);
        let _ = cam_rot;
        tf.rotation = Quat::from_rotation_y(yaw);
        // fade cards out as the camera nears them (a card in front of the lens would white out the frame)
        let f = ((to.length() - 6.0) / 14.0).clamp(0.0, 1.0);
        if let Some(mut mat) = mats.get_mut(&mh.0) {
            mat.base_color = Color::srgba(0.78, 0.86, 0.82, m.alpha * f);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Ripple rings on the water
// ---------------------------------------------------------------------------------------------

#[derive(Component)]
pub struct Ripple {
    age: f32,
    life: f32,
    r0: f32,
    r1: f32,
    alpha: f32,
}

#[derive(Resource)]
pub struct RippleAssets {
    mesh: Handle<Mesh>,
    tex: Handle<Image>,
}

fn ring_image() -> Image {
    const N: u32 = 128;
    let mut data = Vec::with_capacity((N * N * 4) as usize);
    for j in 0..N {
        for i in 0..N {
            let (x, y) = ((i as f32 + 0.5) / N as f32 * 2.0 - 1.0, (j as f32 + 0.5) / N as f32 * 2.0 - 1.0);
            let r = (x * x + y * y).sqrt();
            // a bright thin ring near r = 0.8 with a soft outer skirt and a faint inner second ring
            let ring = (-((r - 0.80) / 0.07).powi(2)).exp();
            let ring2 = (-((r - 0.55) / 0.05).powi(2)).exp() * 0.5;
            let a = ((ring + ring2) * (1.0 - ((r - 0.95) / 0.05).clamp(0.0, 1.0))).clamp(0.0, 1.0);
            data.extend_from_slice(&[255, 255, 255, (a * 255.0) as u8]);
        }
    }
    Image::new(Extent3d { width: N, height: N, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default())
}

/// Spawn an expanding ring on the water surface.
pub fn spawn_ripple(commands: &mut Commands, assets: &RippleAssets, mats: &mut Assets<StandardMaterial>, pos: Vec3, r1: f32, life: f32, alpha: f32) {
    let m = mats.add(StandardMaterial {
        base_color: Color::srgba(0.85, 0.92, 0.9, alpha),
        base_color_texture: Some(assets.tex.clone()),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    commands.spawn((
        Name::new("Ripple"),
        Ripple { age: 0.0, life, r0: r1 * 0.15, r1, alpha },
        Mesh3d(assets.mesh.clone()),
        MeshMaterial3d(m),
        Transform::from_translation(Vec3::new(pos.x, water_y() + 0.04, pos.z)).with_scale(Vec3::splat(r1 * 0.3)),
        NotShadowCaster,
        NotShadowReceiver,
    ));
}

/// Queue a ripple from code that only has `Commands` (the splash helper).
pub fn queue_ripple(commands: &mut Commands, pos: Vec3, r1: f32, life: f32, alpha: f32) {
    commands.queue(move |world: &mut World| {
        let Some(a) = world.get_resource::<RippleAssets>() else { return };
        let (mesh, tex) = (a.mesh.clone(), a.tex.clone());
        let Some(mut mats) = world.remove_resource::<Assets<StandardMaterial>>() else { return };
        let m = mats.add(StandardMaterial {
            base_color: Color::srgba(0.85, 0.92, 0.9, alpha),
            base_color_texture: Some(tex),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        });
        world.insert_resource(mats);
        world.spawn((
            Name::new("Ripple"),
            Ripple { age: 0.0, life, r0: r1 * 0.15, r1, alpha },
            Mesh3d(mesh),
            MeshMaterial3d(m),
            Transform::from_translation(Vec3::new(pos.x, water_y() + 0.04, pos.z)).with_scale(Vec3::splat(r1 * 0.3)),
            NotShadowCaster,
            NotShadowReceiver,
        ));
    });
}

pub fn setup_ripple_assets(commands: &mut Commands, meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) {
    commands.insert_resource(RippleAssets {
        mesh: meshes.add(Plane3d::default().mesh().size(2.0, 2.0)),
        tex: images.add(ring_image()),
    });
}

fn ripples(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Ripple, &mut Transform, &MeshMaterial3d<StandardMaterial>)>, mut mats: ResMut<Assets<StandardMaterial>>) {
    let dt = time.delta_secs();
    for (e, mut r, mut tf, m) in &mut q {
        r.age += dt;
        if r.age >= r.life {
            commands.entity(e).despawn();
            continue;
        }
        let f = r.age / r.life;
        let s = r.r0 + (r.r1 - r.r0) * (1.0 - (1.0 - f).powi(2));
        tf.scale = Vec3::splat(s);
        if let Some(mut mat) = mats.get_mut(&m.0) {
            mat.base_color = Color::srgba(0.85, 0.92, 0.9, r.alpha * (1.0 - f).powf(1.3));
        }
    }
}

/// One-shot: ripple assets, mist cards and Ann at the safe edge, once the fight arena and the FX assets exist.
fn populate_swamp(
    mut commands: Commands,
    mut done: Local<bool>,
    ctl: Option<Res<crate::kong::KongCtl>>,
    fx: Option<Res<FxAssets>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    server: Res<AssetServer>,
) {
    if *done {
        return;
    }
    let (Some(ctl), Some(fx)) = (ctl, fx) else { return };
    let Some(fa) = ctl.farena.as_ref() else { return };
    *done = true;
    setup_ripple_assets(&mut commands, &mut meshes, &mut images);
    spawn_mist(&mut commands, &fx, &mut mats, fa.center);
    // Ann: 2.2 m to the side of Jack's vantage, facing the fight (plain skin + cloth, the export has no head: ball added)
    let side = Vec3::new(fa.jack_yaw.cos(), 0.0, -fa.jack_yaw.sin()) * 2.2;
    let mut pos = fa.jack + side;
    if let Some(y) = fa.floor_at(pos.x, pos.z) {
        pos.y = y;
    }
    if crate::scene::marsh05c() {
        // 05C: Ann waits on the branch D_BV_ANN_Branche (135.7, -36.1, -171.7) [C] (WP_ANN_NO_GRAB next to it)
        pos = Vec3::new(135.7, -36.1, -171.7);
    }
    let to = fa.center - pos;
    let yaw = to.x.atan2(to.z) + std::f32::consts::PI;
    let skin = mats.add(StandardMaterial { base_color: Color::srgb(0.62, 0.45, 0.36), perceptual_roughness: 0.7, ..default() });
    commands
        .spawn((
            Name::new("Ann"),
            Ann,
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
        ))
        .with_children(|c| {
            c.spawn((
                WorldAssetRoot(server.load(GltfAssetLabel::Scene(0).from_asset("kong/ann.glb"))),
                Transform::default(),
            ));
            c.spawn((Mesh3d(meshes.add(Sphere::new(0.11))), MeshMaterial3d(skin), Transform::from_xyz(0.0, 1.52, 0.0)));
        });
}

/// Level meshes (rock slabs, cliff walls, pillars) that stand between the camera and the fighters, or that the
/// camera sits inside, are hidden while they do (what the original does with its camera occluders; the cramped
/// 07D canyon has no room for a camera 12 m behind Kong). Floors (flat or very wide) never hide [G].
fn occluder_hide(
    mut commands: Commands,
    ctl: Option<Res<crate::kong::KongCtl>>,
    cam: Query<&GlobalTransform, With<MainCam>>,
    props: Query<(Entity, &GlobalTransform, &bevy::camera::primitives::Aabb, &Mesh3d, Has<HiddenByCamera>, Option<&Name>), With<LevelProp>>,
    time: Res<Time>,
    meshes: Res<Assets<Mesh>>,
    mut cache: Local<std::collections::HashMap<Entity, Vec<[Vec3; 3]>>>,
) {
    let (Some(ctl), Ok(cam)) = (ctl, cam.single()) else { return };
    if !(ctl.player_control || ctl.cinematic) {
        return;
    }
    let c = cam.translation();
    let (k, r) = (ctl.kong_world(), ctl.rex_world());
    // Kong's legs, chest and head, the rex's body and the middle of the fight must stay in view [G]
    let targets = [k + Vec3::Y * 1.2, k + Vec3::Y * 3.5, k + Vec3::Y * 6.0, r + Vec3::Y * 3.0, k.lerp(r, 0.5) + Vec3::Y * 2.0];
    let dbg = std::env::var("KK_OCC_DEBUG").ok().and_then(|v| v.parse::<f32>().ok()).is_some_and(|t0| (ctl.t - t0).abs() < 0.03);
    for (e, gt, aabb, mesh, was, pname) in &props {
        let m = gt.affine();
        let ce = m.transform_point3(Vec3::from(aabb.center));
        let he = Vec3::from(aabb.half_extents);
        // world half extents (|M| * he, per axis)
        let ex = Vec3::new(
            m.matrix3.x_axis.x.abs() * he.x + m.matrix3.y_axis.x.abs() * he.y + m.matrix3.z_axis.x.abs() * he.z,
            m.matrix3.x_axis.y.abs() * he.x + m.matrix3.y_axis.y.abs() * he.y + m.matrix3.z_axis.y.abs() * he.z,
            m.matrix3.x_axis.z.abs() * he.x + m.matrix3.y_axis.z.abs() * he.y + m.matrix3.z_axis.z.abs() * he.z,
        );
        let (mn, mx) = (ce - ex, ce + ex);
        let size = mx - mn;
        let floorish = size.y < 2.5 || (size.x > 45.0 && size.z > 45.0 && size.y < 12.0);
        // coarse: the box is crossed by a camera->fighter segment; exact: one of the mesh's triangles is
        // (the basin walls are concave shells whose boxes contain the whole fight) [G]
        let mut hit = false;
        if !floorish {
            let segs: Vec<Vec3> = targets.iter().copied().filter(|t| seg_aabb(c, *t, mn - Vec3::splat(0.2), mx + Vec3::splat(0.2))).collect();
            if !segs.is_empty() {
                let tris = cache.entry(e).or_insert_with(|| world_tris(&meshes, &mesh.0, gt));
                hit = segs.iter().any(|t| tris.iter().any(|tri| seg_tri(c, *t, tri)));
            }
        }
        if dbg && targets.iter().any(|t| seg_aabb(c, *t, mn, mx)) {
            info!("occ t {:.3} cam {:?} {:?} size {:?} floorish {} hit {}", ctl.t, c, pname.map(|n| n.as_str()), size, floorish, hit);
        }
        if hit && !was {
            commands.entity(e).insert((Visibility::Hidden, HiddenByCamera));
        } else if !hit && was {
            commands.entity(e).insert(Visibility::Inherited).remove::<HiddenByCamera>();
        }
    }
}

fn world_tris(meshes: &Assets<Mesh>, h: &Handle<Mesh>, gt: &GlobalTransform) -> Vec<[Vec3; 3]> {
    let Some(mesh) = meshes.get(h) else { return Vec::new() };
    let Some(bevy::render::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return Vec::new() };
    let m = gt.affine();
    let w: Vec<Vec3> = pos.iter().map(|p| m.transform_point3(Vec3::from(*p))).collect();
    let idx: Vec<usize> = match mesh.indices() {
        Some(i) => i.iter().collect(),
        None => (0..w.len()).collect(),
    };
    idx.chunks_exact(3).map(|t| [w[t[0]], w[t[1]], w[t[2]]]).collect()
}

/// Segment a..b against a triangle (Moller-Trumbore, both faces), ignoring the last 1.5 m near the target.
fn seg_tri(a: Vec3, b: Vec3, t: &[Vec3; 3]) -> bool {
    let d = b - a;
    let len = d.length();
    if len < 1e-4 {
        return false;
    }
    let dir = d / len;
    let e1 = t[1] - t[0];
    let e2 = t[2] - t[0];
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-7 {
        return false;
    }
    let inv = 1.0 / det;
    let s = a - t[0];
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    let dist = e2.dot(q) * inv;
    dist > 0.0 && dist < len - 1.5
}

/// Slab test: does the segment a..b cross the box?
fn seg_aabb(a: Vec3, b: Vec3, mn: Vec3, mx: Vec3) -> bool {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for k in 0..3 {
        let (o, dd, lo, hi) = (a[k], d[k], mn[k], mx[k]);
        if dd.abs() < 1e-6 {
            if o < lo || o > hi {
                return false;
            }
        } else {
            let (mut u0, mut u1) = ((lo - o) / dd, (hi - o) / dd);
            if u0 > u1 {
                std::mem::swap(&mut u0, &mut u1);
            }
            t0 = t0.max(u0);
            t1 = t1.min(u1);
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}
