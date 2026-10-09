//! Effects recovered from KingKong8.exe: camera shake (CM_Sfx), pad rumble, muzzle flash,
//! gun smoke, muzzle light, bullet impacts, V-Rex footstep dust and roar breath.
//! Numbers and their confidence: research/pc/fx/FX_FINDINGS.md.

use crate::anim::GameState;
use crate::events::{GunEvent, RexEvent};
use crate::player::{ArmsRig, ArmsScene, MainCam, ViewCam};
use crate::spec::WeaponId;
use crate::spec::WEAPONS;
use crate::world::VIEW_LAYER;
use bevy::prelude::*;
use bevy::camera::visibility::RenderLayers;
use rand::Rng;

// ---------------------------------------------------------------------------
// Camera shake: exact port of CM_Sfx 0x45a5a0 / CM_Pilote message type 2.
// ---------------------------------------------------------------------------

/// (amp_v, freq_v, amp_h, freq_h, decay, decay_mult) as sent by fn@0x007d49a0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShakeParams {
    pub amp_v: f32,
    pub freq_v: f32,
    pub amp_h: f32,
    pub freq_h: f32,
    pub decay: f32,
    pub decay_mult: f32,
}

#[derive(Resource, Default, Debug)]
pub struct CameraShake {
    pub p: ShakeParams,
    pub t: f32,
    pub enabled: bool,
    /// fixed 60 Hz accumulator: decay_mult is applied once per game frame in the original
    acc: f32,
    /// last applied offsets (radians), for the tests
    pub last: Vec2,
    /// freq_v of the preset that produced `last` (tests: 50 = footstep/bite, 30 = roar record)
    pub last_freq: f32,
    /// largest vertical amplitude requested this session (tests)
    pub peak_request: f32,
}

impl CameraShake {
    /// CM_Pilote: a new message replaces the current shake and restarts t.
    pub fn send(&mut self, p: ShakeParams) {
        // keep a stronger running shake instead of cutting it short [G]
        if self.enabled && self.p.amp_v.max(self.p.amp_h) > p.amp_v.max(p.amp_h) {
            return;
        }
        self.p = p;
        self.t = 0.0;
        self.enabled = true;
        self.peak_request = self.peak_request.max(p.amp_v);
    }
}

/// T-Rex footstep shake falloff (0x486890): active within 200 m, k = 1 - max(d-10,0)/190. [C]
pub fn rex_shake_k(d: f32) -> f32 {
    if d > 200.0 { 0.0 } else { 1.0 - (d - 10.0).max(0.0) / 190.0 }
}

/// Footstep preset (C): flag 1 = 0.025*k, flag 2 = 0.05*k, freq 50, decay 0.1, mult 0.95.
pub fn rex_footstep_shake(d: f32, strong: bool) -> ShakeParams {
    let k = rex_shake_k(d);
    ShakeParams { amp_v: if strong { 0.05 } else { 0.025 } * k, freq_v: 50.0, amp_h: 0.0, freq_h: 0.0, decay: 0.1, decay_mult: 0.95 }
}

/// Roar shake. KingKong8.exe has none (every shake reachable from raptor code is the
/// footstep 0x486890 or the hide-attack 0x482b80; CINE_ROAR and msg 0x186ab reach no shake;
/// PNJ_Raptor_Shake_Cam is an uncompiled script). The values are the only camera-shake
/// record in level 03E, TrigExec_ShakeCamAndRumble @stream 0x1fcfa14 [C values]:
/// (0.075, 30, 0.15, 20, 0.15, 1.02), rumble (0,0,0) = none. Using it for the roar and the
/// distance falloff k are [G].
pub const ROAR_SHAKE: ShakeParams =
    ShakeParams { amp_v: 0.075, freq_v: 30.0, amp_h: 0.15, freq_h: 20.0, decay: 0.15, decay_mult: 1.02 };
pub fn rex_roar_shake(d: f32) -> ShakeParams {
    let k = rex_shake_k(d);
    ShakeParams { amp_v: ROAR_SHAKE.amp_v * k, amp_h: ROAR_SHAKE.amp_h * k, ..ROAR_SHAKE }
}

// ---------------------------------------------------------------------------
// Rumble: fn@0x00a69150(strength 0..255, duration*0.032 s)
// ---------------------------------------------------------------------------

#[derive(Message, Clone, Copy, Debug)]
pub struct Rumble {
    /// 0..255 as in the game
    pub strength: f32,
    /// game units (x0.032 s)
    pub duration: f32,
    pub source: &'static str,
}

/// (time, strength, duration, source) of every rumble request (tests)
#[derive(Resource, Default)]
pub struct RumbleLog(pub Vec<(f32, f32, f32, &'static str)>);

/// Gun recoil rumble per weapon (H_callback_tir): Colt/Tommy/Shotgun (50,2), Sniper (100,4). [C]
pub fn gun_rumble(id: WeaponId) -> Rumble {
    match id {
        WeaponId::SniperRifle => Rumble { strength: 100.0, duration: 4.0, source: "gun" },
        _ => Rumble { strength: 50.0, duration: 2.0, source: "gun" },
    }
}

// ---------------------------------------------------------------------------
// Particles: billboard bursts (GFX type 0xd "Explode"), sparks (type 9) and flare light.
// ---------------------------------------------------------------------------

#[derive(Component)]
pub struct Particle {
    pub vel: Vec3,
    pub gravity: f32,
    pub drag: f32,
    pub age: f32,
    pub life: f32,
    pub size: (f32, f32),
    /// colour keys at t=0, mid, end (linear RGBA)
    pub color: [LinearRgba; 3],
    pub spin: f32,
    pub view_layer: bool,
}

#[derive(Component)]
pub struct MuzzleLight {
    pub t: f32,
    pub tommy: bool,
}

#[derive(Resource)]
pub struct FxAssets {
    pub quad: Handle<Mesh>,
    pub smoke: Handle<Image>,
    pub dust: Handle<Image>,
    pub blob: Handle<Image>,
    pub mist: Handle<Image>,
    /// global FX material 0x62000f8c sub-materials 0x12, 0x13, 0x11 (decoded from the level
    /// texture bank; the flash code cycles 0x12,0x13,0x11) [C]
    pub flash: [Handle<Image>; 3],
    /// sub 0x21: spark streak
    pub spark: Handle<Image>,
    /// sub 0x25: splatter dots (tinted per use)
    pub splat: Handle<Image>,
    /// sub 0x28: rock chips
    pub chips: Handle<Image>,
    /// sub 0x08: GFX smoke
    pub fx_smoke: Handle<Image>,
}

#[derive(Resource, Default)]
pub struct FxStats {
    pub flashes: u32,
    pub smoke_puffs: u32,
    pub impacts: u32,
    pub blood: u32,
    pub dust: u32,
    pub lights: u32,
    pub breath: u32,
}

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraShake>()
            .init_resource::<RumbleLog>()
            .init_resource::<FxStats>()
            .add_message::<Rumble>()
            .add_systems(Startup, load_fx)
            .add_systems(
                Update,
                (on_gun, on_rex, apply_shake, rumble, update_particles, update_lights)
                    .chain()
                    .after(crate::rex::RexSet)
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

fn load_fx(mut commands: Commands, assets: Res<AssetServer>, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(FxAssets {
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
        smoke: assets.load("fx/smoke_puff_512.png"),
        dust: assets.load("fx/dust_splatter_512.png"),
        blob: assets.load("fx/soft_blob_64x32.png"),
        mist: assets.load("fx/mist_streak_128x256.png"),
        flash: [assets.load("fx/flash_f12.png"), assets.load("fx/flash_f13.png"), assets.load("fx/flash_f11.png")],
        spark: assets.load("fx/spark_streak.png"),
        splat: assets.load("fx/splat_dots.png"),
        chips: assets.load("fx/rock_chips.png"),
        fx_smoke: assets.load("fx/smoke_fx.png"),
    });
}

pub(crate) fn rgba(abgr: u32) -> LinearRgba {
    // game colours are ABGR dwords (0xAABBGGRR); alpha 0x80 ~ opaque on PS2-style blending
    let r = (abgr & 0xff) as f32 / 255.0;
    let g = ((abgr >> 8) & 0xff) as f32 / 255.0;
    let b = ((abgr >> 16) & 0xff) as f32 / 255.0;
    let a = (((abgr >> 24) & 0xff) as f32 / 128.0).min(1.0);
    Color::srgba(r, g, b, a).to_linear()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_particle(
    commands: &mut Commands,
    mats: &mut Assets<StandardMaterial>,
    fx: &FxAssets,
    tex: &Handle<Image>,
    additive: bool,
    pos: Vec3,
    p: Particle,
) {
    let c0 = p.color[0];
    let mat = mats.add(StandardMaterial {
        base_color: Color::from(c0),
        base_color_texture: Some(tex.clone()),
        unlit: true,
        alpha_mode: if additive { AlphaMode::Add } else { AlphaMode::Blend },
        cull_mode: None,
        ..default()
    });
    let layer = if p.view_layer { RenderLayers::layer(VIEW_LAYER) } else { RenderLayers::layer(0) };
    commands.spawn((
        Mesh3d(fx.quad.clone()),
        MeshMaterial3d(mat),
        Transform::from_translation(pos).with_scale(Vec3::splat(p.size.0)),
        bevy::light::NotShadowCaster,
        bevy::light::NotShadowReceiver,
        layer,
        p,
    ));
}

pub(crate) fn rand_unit(rng: &mut impl Rng) -> Vec3 {
    Vec3::new(rng.gen_range(-1.0..1.0), rng.gen_range(-1.0..1.0), rng.gen_range(-1.0..1.0))
}

#[allow(clippy::too_many_arguments)]
fn on_gun(
    mut commands: Commands,
    mut ev: MessageReader<GunEvent>,
    fx: Option<Res<FxAssets>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut stats: ResMut<FxStats>,
    mut rumble: MessageWriter<Rumble>,
    arms: Query<&ArmsRig, With<ArmsScene>>,
    arsenal: Res<crate::weapons::Arsenal>,
) {
    let Some(fx) = fx else { return };
    let mut rng = rand::thread_rng();
    for e in ev.read() {
        match *e {
            GunEvent::Fired { w, muzzle, dir } => {
                let def = &WEAPONS[w];
                rumble.write(gun_rumble(def.id));
                // Muzzle flash: Explode, 5 billboards, size 0.1 -> 0.5, period 0.1 s (player),
                // colour 0x0060a7ff -> 0x70000818 -> 0  [C values; size scaled x0.5 for the
                // close viewmodel camera, G]
                if let Ok(rig) = arms.single() {
                    let local = MUZZLE_LOCAL[w];
                    for i in 0..5 {
                        let p = Particle {
                            vel: Vec3::new(0.0, -0.4 - i as f32 * 0.25, 0.0),
                            gravity: 0.0,
                            drag: 0.0,
                            age: 0.0,
                            life: 0.1,
                            size: (0.05 + 0.01 * i as f32, 0.25 - 0.02 * i as f32),
                            // the sprites are already orange: the code colour 0x0060a7ff is applied
                            // as a 2x-modulate-like boost toward white-hot [G]
                            color: [LinearRgba::new(2.2, 1.7, 1.3, 1.0), rgba(0xa0000818), LinearRgba::NONE],
                            spin: rng.gen_range(0.0..6.28),
                            view_layer: true,
                        };
                        let e = {
                            let c0 = p.color[0];
                            let mat = mats.add(StandardMaterial {
                                base_color: Color::from(c0),
                                base_color_texture: Some(fx.flash[i % 3].clone()),
                                unlit: true,
                                alpha_mode: AlphaMode::Add,
                                cull_mode: None,
                                ..default()
                            });
                            commands
                                .spawn((
                                    Mesh3d(fx.quad.clone()),
                                    MeshMaterial3d(mat),
                                    Transform::from_translation(local + Vec3::new(0.0, -0.02 * i as f32, 0.0))
                                        .with_scale(Vec3::splat(p.size.0)),
                                    bevy::light::NotShadowCaster,
                                    bevy::light::NotShadowReceiver,
                                    RenderLayers::layer(VIEW_LAYER),
                                    p,
                                ))
                                .id()
                        };
                        commands.entity(rig.socket).add_child(e);
                    }
                    stats.flashes += 1;
                }
                // Gun smoke puff (H_exec_GFX_Tir): 5 puffs, grey 0x40c0a0a0 -> 0x0ac0a0a0 -> 0,
                // size 2.5 -> 5.8 game units (x0.1 = metres, G), life 1.5 s, slow rise.
                let n_smoke = if def.id == WeaponId::Shotgun { 10 } else { 5 };
                for _ in 0..n_smoke {
                    let (c0, c1) = if def.id == WeaponId::Shotgun {
                        (rgba(0xa0505050), rgba(0x20202020))
                    } else {
                        (rgba(0x40a8a8a8), rgba(0x0aa8a8a8))
                    };
                    // starts past the barrel so it never sits on the lens; alpha scaled down for
                        // the missing PS2-style 2x blend [G]
                    let (mut c0, mut c1) = (c0, c1);
                    c0.alpha *= 0.35;
                    c1.alpha *= 0.35;
                    spawn_particle(&mut commands, &mut mats, &fx, &fx.fx_smoke, false, muzzle + dir * 1.3, Particle {
                        vel: dir * rng.gen_range(0.3..0.9) + Vec3::Y * 0.25 + rand_unit(&mut rng) * 0.12,
                        gravity: -0.05,
                        drag: 1.2,
                        age: 0.0,
                        life: 1.5,
                        size: (0.12 * rng.gen_range(0.8..1.2), 0.5 * rng.gen_range(0.8..1.3)),
                        color: [c0, c1, LinearRgba::NONE],
                        spin: rng.gen_range(0.0..6.28),
                        view_layer: false,
                    });
                }
                stats.smoke_puffs += n_smoke;
                // Shotgun sparks: 20, colour 0x2080ff, speed 100..150 game units (x0.05, G)
                if def.id == WeaponId::Shotgun {
                    for _ in 0..20 {
                        let v = (dir + rand_unit(&mut rng) * 0.25).normalize() * rng.gen_range(5.0..7.5);
                        spawn_particle(&mut commands, &mut mats, &fx, &fx.spark, true, muzzle + dir * 0.3, Particle {
                            vel: v,
                            gravity: 3.0,
                            drag: 3.0,
                            age: 0.0,
                            life: 0.4,
                            size: (0.04, 0.02),
                            color: [rgba(0xff2080ff), rgba(0xa01060ff), LinearRgba::NONE],
                            spin: 0.0,
                            view_layer: false,
                        });
                    }
                }
                // Muzzle light (H_exec_GFX_Weapon): Tommy gun and shotgun only, 0.2 s flare. [C]
                if matches!(def.id, WeaponId::TommyGun | WeaponId::Shotgun) {
                    commands.spawn((
                        MuzzleLight { t: 0.2, tommy: def.id == WeaponId::TommyGun },
                        PointLight {
                            intensity: 0.0,
                            range: 8.0,
                            color: Color::srgb(1.0, 0.75, 0.45),
                            shadow_maps_enabled: false,
                            ..default()
                        },
                        // world only: lighting the viewmodel from 10 cm away burns it orange
                        Transform::from_translation(muzzle + dir * 0.6),
                        RenderLayers::layer(0),
                    ));
                    stats.lights += 1;
                }
                let _ = arsenal.index;
            }
            GunEvent::Impact { pos, normal, rex, .. } => {
                stats.impacts += 1;
                if rex {
                    // Blood: candidate LIB_GFX 0x7d1b20 (Explode count 40, reddish 0x0027a0fc ->
                    // 0x40303030); the game's raptor blood hook is an empty stub. [G]
                    for _ in 0..4 {
                        spawn_particle(&mut commands, &mut mats, &fx, &fx.fx_smoke, false, pos, Particle {
                            vel: (normal * 0.6 + rand_unit(&mut rng) * 0.4) * rng.gen_range(0.4..1.0),
                            gravity: 0.4,
                            drag: 2.0,
                            age: 0.0,
                            life: rng.gen_range(0.6..1.0),
                            size: (0.35, 1.1),
                            color: [Color::srgba(0.26, 0.02, 0.02, 0.55).to_linear(), Color::srgba(0.18, 0.02, 0.01, 0.25).to_linear(), LinearRgba::NONE],
                            spin: rng.gen_range(0.0..6.28),
                            view_layer: false,
                        });
                    }
                    for _ in 0..6 {
                        let v = (normal + rand_unit(&mut rng) * 0.7).normalize() * rng.gen_range(1.0..2.5);
                        spawn_particle(&mut commands, &mut mats, &fx, &fx.splat, false, pos, Particle {
                            vel: v,
                            gravity: 6.0,
                            drag: 1.0,
                            age: 0.0,
                            life: rng.gen_range(0.5..0.8),
                            size: (0.45, 1.1),
                            color: [Color::srgba(0.40, 0.02, 0.01, 0.9).to_linear(), Color::srgba(0.30, 0.01, 0.01, 0.7).to_linear(), LinearRgba::NONE],
                            spin: 0.0,
                            view_layer: false,
                        });
                    }
                    stats.blood += 1;
                } else {
                    // LIBGFX_GunsImpact (L): dust Explode 6 puffs size 5.05 -> 10.2 (x0.1),
                    // surface-tinted grey 0x40c0c0c0, plus 10 yellow-orange sparks 0xf080c0ff.
                    for _ in 0..6 {
                        spawn_particle(&mut commands, &mut mats, &fx, &fx.dust, false, pos + normal * 0.05, Particle {
                            vel: (normal + rand_unit(&mut rng) * 0.5) * rng.gen_range(0.3..1.0),
                            gravity: 0.3,
                            drag: 1.8,
                            age: 0.0,
                            life: 1.0,
                            size: (0.25, 0.9),
                            color: [rgba(0x60b0b8b8), rgba(0x18b0b8b8), LinearRgba::NONE],
                            spin: rng.gen_range(0.0..6.28),
                            view_layer: false,
                        });
                    }
                    // rock chips (sub 0x28) [texture C, motion G]
                    spawn_particle(&mut commands, &mut mats, &fx, &fx.chips, false, pos + normal * 0.05, Particle {
                        vel: normal * 2.0 + Vec3::Y,
                        gravity: 6.0,
                        drag: 0.8,
                        age: 0.0,
                        life: 0.5,
                        size: (0.25, 0.45),
                        color: [Color::srgba(0.75, 0.72, 0.66, 1.0).to_linear(), Color::srgba(0.7, 0.68, 0.62, 0.8).to_linear(), LinearRgba::NONE],
                        spin: rng.gen_range(0.0..6.28),
                        view_layer: false,
                    });
                    for _ in 0..10 {
                        let v = (normal + rand_unit(&mut rng) * 0.8).normalize() * rng.gen_range(2.0..5.0);
                        spawn_particle(&mut commands, &mut mats, &fx, &fx.spark, true, pos + normal * 0.03, Particle {
                            vel: v,
                            gravity: 5.0,
                            drag: 1.0,
                            age: 0.0,
                            life: 0.35,
                            size: (0.05, 0.02),
                            color: [rgba(0xf080c0ff), rgba(0x8040a0ff), LinearRgba::NONE],
                            spin: 0.0,
                            view_layer: false,
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

/// Muzzle tips in socket (Jade) space, measured from each weapon mesh: -Y end of the barrel.
pub const MUZZLE_LOCAL: [Vec3; 4] = [
    Vec3::new(-0.009, -0.215, 0.089),
    Vec3::new(0.001, -0.48, 0.108),
    Vec3::new(0.0, -0.68, 0.032),
    Vec3::new(-0.003, -0.705, 0.075),
];

#[derive(Default)]
struct RoarState {
    t: f32,
    pos: Vec3,
    active: bool,
    next_send: f32,
}

#[allow(clippy::too_many_arguments)]
fn on_rex(
    mut commands: Commands,
    mut ev: MessageReader<RexEvent>,
    fx: Option<Res<FxAssets>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut shake: ResMut<CameraShake>,
    mut rumble: MessageWriter<Rumble>,
    cam: Query<&GlobalTransform, With<MainCam>>,
    mut stats: ResMut<FxStats>,
    mut roar: Local<RoarState>,
    rex_bones: Query<&crate::rex::RexBones>,
    gts: Query<&GlobalTransform>,
    time: Res<Time>,
) {
    let Some(fx) = fx else { return };
    let listener = cam.single().map(|g| g.translation()).unwrap_or(Vec3::ZERO);
    let mut rng = rand::thread_rng();
    for e in ev.read() {
        match *e {
            RexEvent::Footstep { pos, strong } => {
                let d = pos.distance(listener);
                shake.send(rex_footstep_shake(d, strong));
                let k = rex_shake_k(d);
                if k > 0.0 {
                    // a69150(ftol(100*k), 7) [C]
                    rumble.write(Rumble { strength: (100.0 * k).floor(), duration: 7.0, source: "rex_footstep" });
                }
                // footstep dust (LIBGFX_PoussierePas candidate 0x7d4c40, G)
                for _ in 0..(if strong { 6 } else { 3 }) {
                    spawn_particle(&mut commands, &mut mats, &fx, &fx.dust, false, pos + Vec3::Y * 0.2, Particle {
                        vel: Vec3::new(rng.gen_range(-1.5..1.5), rng.gen_range(0.2..0.8), rng.gen_range(-1.5..1.5)),
                        gravity: 0.1,
                        drag: 1.2,
                        age: 0.0,
                        life: 1.6,
                        size: (0.8, 2.4),
                        color: [Color::srgba(0.42, 0.40, 0.33, 0.55).to_linear(), Color::srgba(0.42, 0.40, 0.33, 0.25).to_linear(), LinearRgba::NONE],
                        spin: rng.gen_range(0.0..6.28),
                        view_layer: false,
                    });
                }
                stats.dust += 1;
            }
            RexEvent::Roar { pos, .. } => {
                roar.active = true;
                roar.t = 0.0;
                roar.pos = pos;
                roar.next_send = 0.0;
            }
            RexEvent::BiteHit { pos } | RexEvent::BiteStart { pos } => {
                // PNJ_Raptor_ETAT_ATTAQUE_CACHE shake: 0.025*k, 50, decay 0.1, 0.95, no rumble [C]
                let k = rex_shake_k(pos.distance(listener));
                // 0x482b80 only shakes when k > 0.5 (d < ~105 m) [C]
                if k > 0.5 { shake.send(ShakeParams { amp_v: 0.025 * k, freq_v: 50.0, amp_h: 0.0, freq_h: 0.0, decay: 0.1, decay_mult: 0.95 }); }
            }
            _ => {}
        }
    }
    // Roar: shake + breath/drool mist from the jaw while the roar is loudest (0.5-2.8 s).
    if roar.active {
        roar.t += time.delta_secs();
        let jaw = rex_bones
            .single()
            .ok()
            .and_then(|b| gts.get(b.jaw).ok().map(|g| (g.translation(), g.forward().as_vec3())));
        let (head, fwd) = jaw.unwrap_or((roar.pos + Vec3::Y * 5.0, Vec3::Z));
        let to_cam = (listener - head).normalize_or_zero();
        if roar.t > 0.5 && roar.t < 2.8 {
            if roar.t >= roar.next_send {
                // one send: the record's envelope lasts ~0.7 s on its own; re-sent once
                // for the second half of the roar [G]
                roar.next_send = roar.t + 1.2;
                let d = head.distance(listener);
                shake.send(rex_roar_shake(d));
                // the 03E record's rumble vector is (0,0,0): no rumble [C]
            }
            // breath: candidate LIBGFX_BaveTRex 0x7d9b40 (Explode count 10, rising) [G]
            if rng.gen_bool(0.6) {
                let dir = (to_cam * 0.7 + fwd * 0.3).normalize_or_zero();
                spawn_particle(&mut commands, &mut mats, &fx, &fx.smoke, false, head + dir * 1.2, Particle {
                    vel: dir * rng.gen_range(3.0..6.0) + Vec3::Y * 0.6 + rand_unit(&mut rng) * 0.6,
                    gravity: -0.2,
                    drag: 1.5,
                    age: 0.0,
                    life: 1.1,
                    size: (0.5, 2.2),
                    color: [Color::srgba(0.85, 0.88, 0.86, 0.35).to_linear(), Color::srgba(0.8, 0.85, 0.85, 0.15).to_linear(), LinearRgba::NONE],
                    spin: rng.gen_range(0.0..6.28),
                    view_layer: false,
                });
                stats.breath += 1;
            }
        }
        if roar.t > 3.6 {
            roar.active = false;
        }
    }
}

/// CM_Sfx: v = -sin(t*fv)*av, h = sin(t*fh)*ah, amplitudes decay by dt*decay, decay *= mult
/// once per frame (60 Hz). Applied as small pitch/yaw offsets on the main camera.
fn apply_shake(time: Res<Time>, mut shake: ResMut<CameraShake>, mut cam: Query<&mut Transform, With<MainCam>>) {
    let Ok(mut tf) = cam.single_mut() else { return };
    if !shake.enabled {
        shake.last = Vec2::ZERO;
        return;
    }
    let dt = time.delta_secs();
    let p = shake.p;
    let v = -(shake.t * p.freq_v).sin() * p.amp_v;
    let h = (shake.t * p.freq_h).sin() * p.amp_h;
    shake.last = Vec2::new(h, v);
    shake.last_freq = p.freq_v;
    tf.rotation = tf.rotation * Quat::from_rotation_x(v) * Quat::from_rotation_y(h);
    shake.t += dt;
    shake.acc += dt;
    let mut s = shake.p;
    s.amp_v = (s.amp_v - dt * s.decay).max(0.0);
    s.amp_h = (s.amp_h - dt * s.decay).max(0.0);
    while shake.acc >= 1.0 / 60.0 {
        shake.acc -= 1.0 / 60.0;
        s.decay *= s.decay_mult;
    }
    shake.p = s;
    // the original stops at zero; with mult < 1 the decay can stall, so also stop when tiny [G]
    if (s.amp_v <= 0.0 && s.amp_h <= 0.0) || (s.amp_v < 0.002 && s.amp_h < 0.002) || shake.t > 3.0 {
        shake.enabled = false;
    }
}

fn rumble(
    mut ev: MessageReader<Rumble>,
    mut log: ResMut<RumbleLog>,
    time: Res<Time>,
    #[cfg(feature = "gamepad")] gamepads: Query<Entity, With<Gamepad>>,
    #[cfg(feature = "gamepad")] mut out: MessageWriter<bevy::input::gamepad::GamepadRumbleRequest>,
) {
    for r in ev.read() {
        log.0.push((time.elapsed_secs(), r.strength, r.duration, r.source));
        #[cfg(feature = "gamepad")]
        for g in &gamepads {
            let s = (r.strength / 255.0).clamp(0.0, 1.0);
            out.write(bevy::input::gamepad::GamepadRumbleRequest::Add {
                gamepad: g,
                duration: std::time::Duration::from_secs_f32(r.duration * 0.032),
                intensity: bevy::input::gamepad::GamepadRumbleIntensity { strong_motor: s, weak_motor: s * 0.6 },
            });
        }
    }
}

fn update_particles(
    mut commands: Commands,
    time: Res<Time>,
    cam: Query<&GlobalTransform, With<MainCam>>,
    view: Query<&GlobalTransform, With<ViewCam>>,
    parents: Query<&GlobalTransform, Without<Particle>>,
    mut q: Query<(Entity, &mut Particle, &mut Transform, &MeshMaterial3d<StandardMaterial>, Option<&ChildOf>)>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    kong: Option<Res<crate::kong::KongCtl>>,
) {
    let dt = time.delta_secs();
    let swamp_fade = crate::scene::swamp();
    // the fighters read over the spray: a splash particle nearer to the camera than Kong / the rex and inside
    // their silhouette is thinned out (the clip keeps both bodies readable through the water sheets) [G]
    let fighters: Vec<(Vec3, f32)> = kong
        .as_ref()
        .map(|c| vec![(c.kong_world() + Vec3::Y * 3.4, 3.4), (c.rex_world() + Vec3::Y * 3.0, 4.6)])
        .unwrap_or_default();
    let cam_pos = cam.single().map(|g| g.translation()).unwrap_or_default();
    let cam_rot = cam.single().map(|g| g.compute_transform().rotation).unwrap_or_default();
    let view_rot = view.single().map(|g| g.compute_transform().rotation).unwrap_or(cam_rot);
    for (e, mut p, mut tf, mat, parent) in &mut q {
        p.age += dt;
        if p.age >= p.life {
            commands.entity(e).despawn();
            continue;
        }
        let f = p.age / p.life;
        let drag = (1.0 - p.drag * dt).max(0.0);
        p.vel *= drag;
        p.vel.y -= p.gravity * dt;
        tf.translation += p.vel * dt;
        let s = p.size.0 + (p.size.1 - p.size.0) * f;
        tf.scale = Vec3::splat(s.max(0.001));
        // billboard: face the camera (account for a rotated parent)
        let target = if p.view_layer { view_rot } else { cam_rot };
        let parent_rot = parent
            .and_then(|c| parents.get(c.parent()).ok())
            .map(|g| g.compute_transform().rotation)
            .unwrap_or_default();
        tf.rotation = parent_rot.inverse() * target * Quat::from_rotation_z(p.spin + p.age * 0.6);
        let c = if f < 0.5 {
            p.color[0].mix(&p.color[1], f * 2.0)
        } else {
            p.color[1].mix(&p.color[2], (f - 0.5) * 2.0)
        };
        let mut c = c;
        if swamp_fade && !p.view_layer {
            // swamp: spray close to the camera thins out so the fighters stay readable inside the splashes [G]
            let d = tf.translation.distance(cam_pos) - 0.5 * s;
            let k = ((d - 2.5) / 9.0).clamp(0.0, 1.0);
            c.alpha *= 0.10 + 0.90 * k * k * (3.0 - 2.0 * k);
        }
        if !p.view_layer && !fighters.is_empty() {
            let to_p = tf.translation - cam_pos;
            let dp = to_p.length();
            for (fc, fr) in &fighters {
                let to_f = *fc - cam_pos;
                let df = to_f.length();
                if dp + 0.5 * s * 0.3 >= df - fr * 0.3 || df < 1e-3 || dp < 1e-3 {
                    continue; // behind (or level with) the body: the depth test already hides it
                }
                let ang = to_p.angle_between(to_f);
                let edge = (fr / df).atan() + (0.25 * s / dp).atan();
                let t = (ang / edge.max(1e-3)).clamp(0.0, 2.0);
                // inside the silhouette -> 12 %, fading back in over its rim
                let k = ((t - 0.75) / 0.45).clamp(0.0, 1.0);
                c.alpha *= 0.12 + 0.88 * k * k * (3.0 - 2.0 * k);
            }
        }
        if let Some(mut m) = mats.get_mut(&mat.0) {
            m.base_color = Color::from(c);
        }
    }
}

fn update_lights(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut MuzzleLight, &mut PointLight)>) {
    let now = time.elapsed_secs();
    for (e, mut l, mut pl) in &mut q {
        l.t -= time.delta_secs();
        if l.t <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let k = l.t / 0.2;
        // Tommy: (sin(60t)*1.5+3)*(t/0.2); Shotgun: 50*(t/0.2)  [C, unit scale G]
        let i = if l.tommy { ((now * 60.0).sin() * 1.5 + 3.0) * k } else { 50.0 * k * 0.12 };
        pl.intensity = i * 25_000.0;
        let m = (1.0 + (now * 60.0).sin()) * 0.5 * k;
        pl.color = Color::srgb(1.0, 0.72 + 0.15 * m, 0.42 + 0.3 * m);
    }
}
