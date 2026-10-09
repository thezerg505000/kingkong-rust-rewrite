//! Plain test area: `KK_SCENE=testarea` (set automatically by the `t*` batches) replaces level 03E by a flat
//! grid ground with neutral light, a neutral horizon and distance markers. Jack spawns at the origin looking
//! down -Z; creatures and Kong are placed by the batch scripts (`tbatch.rs`) or by hand through
//! `creatures::spawn_creature`.
//!
//! Keys in the test area (interactive): none beyond the normal slice keys; creature spawning is scripted.

use crate::anim::GameState;
use crate::creatures::Creature;
use crate::player::MainCam;
use crate::rex::Rex;
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::camera::visibility::RenderLayers;
use std::sync::OnceLock;

/// Is the test area the active scene? `KK_SCENE=testarea`, or a test-area batch (`KK_BATCH=t1_..`).
pub fn active() -> bool {
    static A: OnceLock<bool> = OnceLock::new();
    *A.get_or_init(|| {
        std::env::var("KK_SCENE").map(|s| s == "testarea").unwrap_or(false)
            || std::env::var("KK_BATCH").map(|b| is_test_batch(&b)).unwrap_or(false)
    })
}

/// `t1`, `t1_creature_lineup`, ... (any batch starting with `t<digit>`)
pub fn is_test_batch(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() >= 2 && b[0] == b't' && b[1].is_ascii_digit()
}

pub const SKY: Color = Color::srgb(0.62, 0.72, 0.82);

/// Show a text label above every creature.
#[derive(Resource)]
pub struct ShowLabels(pub bool);

#[derive(Component)]
struct LabelUi {
    target: LabelTarget,
    half_w: f32,
}

enum LabelTarget {
    Fixed(Vec3),
    Creature(Entity),
}

pub struct TestAreaPlugin;

impl Plugin for TestAreaPlugin {
    fn build(&self, app: &mut App) {
        if !active() {
            return;
        }
        app.insert_resource(ShowLabels(true))
            .insert_resource(ClearColor(SKY))
            .insert_resource(GlobalAmbientLight { color: Color::srgb(0.9, 0.93, 1.0), brightness: 700.0, ..default() })
            .add_systems(OnEnter(GameState::Playing), spawn_scene)
            .add_systems(Update, (remove_rex, neutral_fog, labels).run_if(in_state(GameState::Playing)))
            .add_systems(PostUpdate, label_visibility.before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate));
    }
}

/// Grid texture: 8 m tile, a line per metre, heavier at the tile edge; with a mip chain so it does not shimmer.
fn grid_image() -> Image {
    const N: usize = 512;
    let mut level: Vec<[f32; 3]> = Vec::with_capacity(N * N);
    for y in 0..N {
        for x in 0..N {
            // 64 px per metre
            let (mx, my) = (x % 64, y % 64);
            let edge = x < 3 || y < 3;
            let line = mx < 2 || my < 2;
            let base = 0.66;
            let v: f32 = if edge { 0.16 } else if line { 0.40 } else { base };
            // faint checker in the 1 m cells to read speed and scale
            let chk: f32 = if ((x / 64) + (y / 64)) % 2 == 0 { 0.0 } else { -0.035 };
            let c: f32 = (v + if line || edge { 0.0 } else { chk }).clamp(0.0, 1.0);
            level.push([c * 0.97, c, c * 0.95]);
        }
    }
    let mut data: Vec<u8> = Vec::new();
    let mut size = N;
    let mut mips = 0;
    loop {
        for p in &level {
            for c in p {
                data.push((c.clamp(0.0, 1.0) * 255.0) as u8);
            }
            data.push(255);
        }
        mips += 1;
        if size == 1 {
            break;
        }
        let ns = size / 2;
        let mut next = Vec::with_capacity(ns * ns);
        for y in 0..ns {
            for x in 0..ns {
                let mut s = [0.0f32; 3];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let p = level[(y * 2 + dy) * size + x * 2 + dx];
                    for i in 0..3 {
                        s[i] += p[i] * 0.25;
                    }
                }
                next.push(s);
            }
        }
        level = next;
        size = ns;
    }
    let mut img = Image::new(
        Extent3d { width: N as u32, height: N as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![0u8; N * N * 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    img.data = Some(data);
    img.texture_descriptor.mip_level_count = mips;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        anisotropy_clamp: 16,
        ..ImageSamplerDescriptor::linear()
    });
    img
}

fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let size = 3000.0;
    commands.spawn((
        Name::new("TestGround"),
        Mesh3d(meshes.add(Plane3d::default().mesh().size(size, size))),
        MeshMaterial3d(mats.add(StandardMaterial {
            base_color_texture: Some(images.add(grid_image())),
            perceptual_roughness: 1.0,
            reflectance: 0.05,
            uv_transform: bevy::math::Affine2::from_scale(Vec2::splat(size / 8.0)),
            ..default()
        })),
    ));
    commands.spawn((
        Name::new("Sun"),
        DirectionalLight { illuminance: 11000.0, shadow_maps_enabled: true, color: Color::srgb(1.0, 0.97, 0.9), ..default() },
        Transform::from_xyz(40.0, 80.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::from_layers(&[0, crate::world::VIEW_LAYER]),
        bevy::light::CascadeShadowConfigBuilder { maximum_distance: 160.0, first_cascade_far_bound: 12.0, ..default() }.build(),
    ));
    // distance markers: a post every 10 m down Jack's view axis (-Z) and along +/-X, labelled
    let post = meshes.add(Cuboid::new(0.25, 2.0, 0.25));
    let tall = meshes.add(Cuboid::new(0.4, 6.0, 0.4));
    let colors = [Color::srgb(0.85, 0.2, 0.15), Color::srgb(0.15, 0.4, 0.85), Color::srgb(0.9, 0.75, 0.1)];
    let mat = |c: Color, mats: &mut Assets<StandardMaterial>| mats.add(StandardMaterial { base_color: c, perceptual_roughness: 0.8, ..default() });
    let (red, blue, yellow) = (mat(colors[0], &mut mats), mat(colors[1], &mut mats), mat(colors[2], &mut mats));
    for i in 1..=15 {
        let d = i as f32 * 10.0;
        let big = i % 5 == 0;
        for (p, m) in [(Vec3::new(0.0, 0.0, -d), &red), (Vec3::new(d, 0.0, 0.0), &blue), (Vec3::new(-d, 0.0, 0.0), &blue), (Vec3::new(0.0, 0.0, d), &yellow)] {
            let h = if big { 3.0 } else { 1.0 };
            commands.spawn((
                Mesh3d(if big { tall.clone() } else { post.clone() }),
                MeshMaterial3d(m.clone()),
                Transform::from_translation(p + Vec3::Y * h),
            ));
            if i <= 3 && p.z <= 0.0 {
                spawn_label(&mut commands, p + Vec3::Y * (2.0 * h + 0.5), &format!("{d:.0} m"));
            }
        }
    }
    // Jack's spot
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(0.5, 0.05))),
        MeshMaterial3d(mat(Color::srgb(0.1, 0.1, 0.1), &mut mats)),
        Transform::from_xyz(0.0, 0.03, 0.0),
    ));
}

fn spawn_label(commands: &mut Commands, at: Vec3, text: &str) -> Entity {
    spawn_label_ui(commands, LabelTarget::Fixed(at), text)
}

fn spawn_label_ui(commands: &mut Commands, target: LabelTarget, text: &str) -> Entity {
    commands
        .spawn((
            LabelUi { target, half_w: text.len() as f32 * 4.3 + 4.0 },
            Node { position_type: PositionType::Absolute, padding: UiRect::axes(Val::Px(4.0), Val::Px(1.0)), ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            Text::new(text.to_string()),
            TextFont { font_size: FontSize::Px(15.0), ..default() },
            TextColor(Color::WHITE),
            Visibility::Hidden,
        ))
        .id()
}

/// The Rex belongs to level 03E: not in the test area.
fn remove_rex(mut commands: Commands, q: Query<Entity, With<Rex>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

/// Neutral horizon: fog colour = sky colour, far.
fn neutral_fog(mut cams: Query<&mut DistanceFog, Added<MainCam>>) {
    for mut f in &mut cams {
        f.color = SKY;
        f.falloff = FogFalloff::Linear { start: 300.0, end: 1400.0 };
    }
}

#[derive(Component)]
struct HasLabel;

#[allow(clippy::too_many_arguments)]
fn labels(
    mut commands: Commands,
    show: Res<ShowLabels>,
    new: Query<(Entity, &Creature), Without<HasLabel>>,
    cam: Query<(&Camera, &GlobalTransform), With<MainCam>>,
    targets: Query<(&Creature, &GlobalTransform)>,
    mut uis: Query<(Entity, &LabelUi, &mut Node)>,
) {
    for (e, c) in &new {
        let k = &crate::creatures::KINDS[c.kind];
        let txt = if c.scale != 1.0 { format!("{} [{} x{:.2}]", c.label, k.display.split(" x").next().unwrap_or(k.display), c.scale) } else { format!("{} [{}]", c.label, k.display) };
        let ui = spawn_label_ui(&mut commands, LabelTarget::Creature(e), &txt);
        let _ = ui;
        commands.entity(e).insert(HasLabel);
    }
    let Ok((camera, cgt)) = cam.single() else { return };
    for (e, l, mut node) in &mut uis {
        let world = match l.target {
            LabelTarget::Fixed(p) => p,
            LabelTarget::Creature(t) => match targets.get(t) {
                Ok((c, gt)) => {
                    if !show.0 {
                        node.left = Val::Px(-1000.0);
                        continue;
                    }
                    let k = &crate::creatures::KINDS[c.kind];
                    let h = if k.height_m > 0.0 { k.height_m } else { 4.5 };
                    gt.translation() + Vec3::Y * (h * c.scale * 0.95 + 0.4)
                }
                Err(_) => {
                    commands.entity(e).despawn();
                    continue;
                }
            },
        };
        // labels of things behind the camera or very far are skipped
        let to = world - cgt.translation();
        if to.dot(*cgt.forward()) <= 0.5 || to.length() > 400.0 {
            node.left = Val::Px(-1000.0);
            continue;
        }
        match camera.world_to_viewport(cgt, world) {
            Ok(p) => {
                node.left = Val::Px(p.x - l.half_w);
                node.top = Val::Px(p.y - 10.0);
            }
            Err(_) => node.left = Val::Px(-1000.0),
        }
    }
}

/// `hud::clean_hud` hides every UI node in batches; the labels are part of the picture.
fn label_visibility(mut q: Query<&mut Visibility, With<LabelUi>>) {
    for mut v in &mut q {
        *v = Visibility::Visible;
    }
}
