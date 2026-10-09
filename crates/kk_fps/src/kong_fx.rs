//! Effects of the Kong fight, in the spirit of the reference clip (rain, heavy water / mud splashes at the
//! feet and at impacts, camera shake on heavy hits and roars, motion blur on fast moves, a hit flash).
//! Reuses the particle machinery of `fx.rs` and the game's own sprites (`fx/dust_splatter_512.png` is the
//! water crown, `splat_dots.png` the droplets, `smoke_puff_512.png` the mist, `rock_chips.png` the mud).
//! All sizes, speeds and counts are `[G]`: nothing of the original fight effects is recovered yet.

use crate::events::RexEvent;
use crate::fx::{rand_unit, spawn_particle, CameraShake, FxAssets, Particle, ShakeParams, rex_shake_k};
use crate::kong::{KongBones, KongCtl, KongSceneRoot};
use crate::player::MainCam;
use bevy::core_pipeline::motion_blur::MotionBlur;
use bevy::prelude::*;
use kk_mechanics::kong::fight::*;
use rand::Rng;

/// Motion blur needs the depth + motion-vector prepasses (GPU); off on software GL and with `KK_NO_MOTION_BLUR`.
pub fn motion_blur_ok() -> bool {
    std::env::var("KK_SOFTWARE_GL").is_err() && std::env::var("KK_NO_MOTION_BLUR").is_err()
}

pub fn new_motion_blur() -> MotionBlur {
    MotionBlur { shutter_angle: 0.3, samples: 4 }
}

type Mats<'a> = &'a mut Assets<StandardMaterial>;

/// Water + mud splash at a ground point. `scale` ~1 for a Kong footfall, ~2.5 for a heavy blow, ~3.5 for a slam,
/// ~5 for a body falling. In the swamp the splashes are the reference's: huge white sheets that rise several
/// metres and hang for seconds, a spray column, falling droplets, a low mist skirt and expanding rings on the
/// surface [G sizes, tuned to s_030..s_046].
pub fn splash(commands: &mut Commands, mats: Mats, fx: &FxAssets, pos: Vec3, scale: f32, mud: bool) {
    let mut rng = rand::thread_rng();
    let s = scale.max(0.3);
    let swamp = crate::scene::swamp();
    // the swamp reference splashes are about twice the size and live twice as long
    let (big, hang) = if swamp { (1.45f32, 1.6f32) } else { (1.0, 1.0) };
    let h0 = if swamp { crate::swamp::water_y().max(pos.y - 0.2) } else { pos.y };
    let pos = Vec3::new(pos.x, h0, pos.z);
    // the crown: bright water sheets rising and widening (dust_splatter is a white spray)
    let crowns = (2.0 + s * 1.8 * big) as usize;
    for i in 0..crowns {
        let ang = rng.gen_range(0.0..std::f32::consts::TAU);
        let out = Vec3::new(ang.cos(), 0.0, ang.sin()) * rng.gen_range(0.2..1.6) * s.sqrt() * big;
        spawn_particle(commands, mats, fx, &fx.dust, false, pos + Vec3::Y * 0.3 + out * 0.4, Particle {
            vel: out + Vec3::Y * rng.gen_range(1.8..3.8) * s.sqrt() * big.sqrt(),
            gravity: 3.0,
            drag: 1.2,
            age: 0.0,
            life: rng.gen_range(0.75..1.15) * hang,
            size: ((0.9 * s * big).min(4.0), ((2.4 * s + i as f32 * 0.2) * big).min(7.5)),
            color: [
                Color::srgba(0.86, 0.92, 0.92, if swamp { 0.70 } else { 0.88 }).to_linear(),
                Color::srgba(0.74, 0.83, 0.83, if swamp { 0.36 } else { 0.50 }).to_linear(),
                LinearRgba::NONE,
            ],
            spin: rng.gen_range(0.0..6.28),
            view_layer: false,
        });
    }
    // spray column: a few tall soft plumes (swamp only; the 03E ground splash stays low)
    if swamp && s > 1.6 {
        for _ in 0..(1.0 + s * 1.5) as usize {
            let out = Vec3::new(rng.gen_range(-1.0..1.0), 0.0, rng.gen_range(-1.0..1.0)) * s.sqrt();
            spawn_particle(commands, mats, fx, &fx.smoke, false, pos + Vec3::Y * 0.5 + out * 0.3, Particle {
                vel: out * 0.6 + Vec3::Y * rng.gen_range(3.5..6.5) * s.sqrt(),
                gravity: 4.5,
                drag: 0.9,
                age: 0.0,
                life: rng.gen_range(1.3..2.2),
                size: ((0.8 * s).min(2.5), (3.0 * s).min(7.0)),
                color: [Color::srgba(0.90, 0.95, 0.94, 0.72).to_linear(), Color::srgba(0.80, 0.88, 0.87, 0.42).to_linear(), LinearRgba::NONE],
                spin: rng.gen_range(0.0..6.28),
                view_layer: false,
            });
        }
    }
    // droplets thrown out and up
    for _ in 0..((5.0 + s * 5.0) * big) as usize {
        let v = (Vec3::new(rng.gen_range(-1.0..1.0), 0.0, rng.gen_range(-1.0..1.0)).normalize_or_zero() * rng.gen_range(1.5..4.5) + Vec3::Y * rng.gen_range(3.0..8.0)) * s.sqrt();
        spawn_particle(commands, mats, fx, &fx.splat, false, pos + Vec3::Y * 0.2, Particle {
            vel: v,
            gravity: 11.0,
            drag: 0.5,
            age: 0.0,
            life: rng.gen_range(0.5..0.95) * hang.sqrt(),
            size: (0.35 * s.sqrt() * big.sqrt(), 0.8 * s.sqrt() * big.sqrt()),
            color: [Color::srgba(0.88, 0.94, 0.94, 0.95).to_linear(), Color::srgba(0.78, 0.88, 0.88, 0.6).to_linear(), LinearRgba::NONE],
            spin: rng.gen_range(0.0..6.28),
            view_layer: false,
        });
    }
    // low mist skirt hanging over the water, lingering
    for _ in 0..(1.0 + s * big) as usize {
        spawn_particle(commands, mats, fx, &fx.smoke, false, pos + Vec3::Y * 0.5 + rand_unit(&mut rng) * 0.4, Particle {
            vel: Vec3::new(rng.gen_range(-0.9..0.9), rng.gen_range(0.3..0.9), rng.gen_range(-0.9..0.9)),
            gravity: -0.05,
            drag: 1.1,
            age: 0.0,
            life: rng.gen_range(1.4..2.2) * hang,
            size: ((1.2 * s * big.sqrt()).min(3.0), (3.2 * s * big).min(5.0)),
            color: [Color::srgba(0.74, 0.80, 0.80, 0.2).to_linear(), Color::srgba(0.68, 0.76, 0.76, 0.1).to_linear(), LinearRgba::NONE],
            spin: rng.gen_range(0.0..6.28),
            view_layer: false,
        });
    }
    if mud && s > 1.2 && !swamp {
        for _ in 0..(2.0 + s) as usize {
            let v = Vec3::new(rng.gen_range(-3.0..3.0), rng.gen_range(3.0..6.0), rng.gen_range(-3.0..3.0));
            spawn_particle(commands, mats, fx, &fx.chips, false, pos + Vec3::Y * 0.2, Particle {
                vel: v,
                gravity: 12.0,
                drag: 0.4,
                age: 0.0,
                life: 0.7,
                size: (0.5, 0.8),
                color: [Color::srgba(0.33, 0.27, 0.17, 1.0).to_linear(), Color::srgba(0.28, 0.23, 0.15, 0.8).to_linear(), LinearRgba::NONE],
                spin: rng.gen_range(0.0..6.28),
                view_layer: false,
            });
        }
    }
    if swamp {
        // rings on the surface: one per splash, big ones get a second, later ring
        crate::swamp::queue_ripple(commands, pos, 2.2 * s.sqrt() + 0.8 * s, 1.5 * hang, 0.55);
        if s > 1.8 {
            crate::swamp::queue_ripple(commands, pos, 3.6 * s.sqrt() + 1.5 * s, 2.4 * hang, 0.35);
        }
    }
}

/// Short impact flash with sparks (a landed blow).
pub fn impact_flash(commands: &mut Commands, mats: Mats, fx: &FxAssets, pos: Vec3, scale: f32) {
    let mut rng = rand::thread_rng();
    for i in 0..3 {
        spawn_particle(commands, mats, fx, &fx.flash[i % 3], true, pos, Particle {
            vel: Vec3::ZERO,
            gravity: 0.0,
            drag: 0.0,
            age: 0.0,
            life: 0.14,
            size: (1.2 * scale, 3.2 * scale + i as f32 * 0.4),
            color: [LinearRgba::new(2.6, 2.1, 1.5, 1.0), LinearRgba::new(1.2, 0.6, 0.3, 0.7), LinearRgba::NONE],
            spin: rng.gen_range(0.0..6.28),
            view_layer: false,
        });
    }
    for _ in 0..10 {
        let v = rand_unit(&mut rng).normalize_or_zero() * rng.gen_range(5.0..11.0) * scale.sqrt();
        spawn_particle(commands, mats, fx, &fx.spark, true, pos, Particle {
            vel: v,
            gravity: 7.0,
            drag: 1.0,
            age: 0.0,
            life: 0.4,
            size: (0.16, 0.06),
            color: [crate::fx::rgba(0xf080c0ff), crate::fx::rgba(0x8040a0ff), LinearRgba::NONE],
            spin: 0.0,
            view_layer: false,
        });
    }
}

fn send_shake(shake: &mut CameraShake, amp: f32, freq: f32, mult: f32, k: f32) {
    shake.send(ShakeParams { amp_v: amp * k, freq_v: freq, amp_h: amp * 0.6 * k, freq_h: freq * 0.7, decay: 0.15, decay_mult: mult });
}

#[allow(clippy::too_many_arguments)]
/// First sound definition of `names` the user's sound set has (the 07D / Kong banks only exist when
/// the extractor built every `.smd` of Sound_Common.bf); the last name is the fallback.
fn pick(defs: &crate::sfx::SoundDefs, names: &[&'static str]) -> &'static str {
    names.iter().copied().find(|n| defs.0.contains_key(*n)).unwrap_or(names[names.len() - 1])
}

#[allow(clippy::too_many_arguments)]
pub fn kong_effects(
    mut commands: Commands,
    mut ctl: ResMut<KongCtl>,
    fx: Option<Res<FxAssets>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut shake: ResMut<CameraShake>,
    mut rumble: EventWriter<crate::fx::Rumble>,
    mut sfx: EventWriter<crate::sfx::PlaySfx>,
    cam: Query<&GlobalTransform, With<MainCam>>,
    defs: Res<crate::sfx::SoundDefs>,
    mut advantage_armed: Local<Option<bool>>,
) {
    let Some(fx) = fx else { return };
    let listener = cam.single().map(|g| g.translation()).unwrap_or(Vec3::ZERO);
    let c = &mut *ctl;
    let evs = c.frame_events.clone();
    let (kp, rp) = (c.kong_world(), c.rex_world());
    let mut rng = rand::thread_rng();
    for e in &evs {
        match e {
            FightEvent::Hit { attacker, victim, damage, pos, .. } => {
                let ground = if *victim == Actor::Rex { rp } else { kp };
                let p = c.world((pos[0], pos[1]), ground.y + pos[2].max(1.0));
                let heavy = (*damage as f32 / 10.0).clamp(0.6, 2.4);
                if *damage > 0 {
                    impact_flash(&mut commands, &mut mats, &fx, p, 0.8 + heavy * 0.5);
                    c.stats.flashes += 1;
                    splash(&mut commands, &mut mats, &fx, ground, 1.5 + heavy, true);
                    c.stats.splashes += 1;
                }
                let k = rex_shake_k(listener.distance(p)).max(0.35);
                if *attacker == Actor::Rex {
                    send_shake(&mut shake, 0.05 * heavy.sqrt(), 40.0, 0.97, k);
                    c.stats.shakes += 1;
                    if c.player_control {
                        rumble.write(crate::fx::Rumble { strength: 140.0, duration: 8.0, source: "kong_hurt" });
                    }
                } else {
                    send_shake(&mut shake, 0.03 * heavy, 45.0, 0.96, k);
                    c.stats.shakes += 1;
                    if c.player_control {
                        rumble.write(crate::fx::Rumble { strength: 90.0 * heavy, duration: 5.0, source: "kong_hit" });
                    }
                }
            }
            FightEvent::CameraShake { amplitude, frequency, decay, .. } => {
                let k = rex_shake_k(listener.distance(kp)).max(0.4);
                send_shake(&mut shake, *amplitude, *frequency, decay.clamp(0.95, 1.08), k);
                c.stats.shakes += 1;
            }
            FightEvent::FuryShout { .. } | FightEvent::VictoryPound | FightEvent::PoundStart => {
                // water crown ring around Kong: the pound / shout slams the water
                if !matches!(e, FightEvent::PoundStart) {
                    // (a light ring: big crowns all round Kong veil him in white from any camera) [G]
                    for i in 0..6 {
                        let a = i as f32 / 6.0 * std::f32::consts::TAU;
                        let p = kp + Vec3::new(a.cos(), 0.0, a.sin()) * rng.gen_range(4.0..6.0);
                        splash(&mut commands, &mut mats, &fx, p, 1.3, i % 2 == 0);
                        c.stats.splashes += 1;
                        c.stats.ring_splashes += 1;
                    }
                    let k = rex_shake_k(listener.distance(kp)).max(0.4);
                    send_shake(&mut shake, 0.05, 30.0, 1.03, k);
                    c.stats.shakes += 1;
                }
            }
            FightEvent::ThrowImpact { .. } | FightEvent::Slam { .. } | FightEvent::KoStart { .. } | FightEvent::RexDied => {
                splash(&mut commands, &mut mats, &fx, rp, 5.0, true);
                splash(&mut commands, &mut mats, &fx, rp + Vec3::new(2.5, 0.0, 1.5), 3.4, true);
                splash(&mut commands, &mut mats, &fx, rp + Vec3::new(-3.0, 0.0, -1.0), 3.0, true);
                c.stats.splashes += 2;
                let k = rex_shake_k(listener.distance(rp)).max(0.4);
                send_shake(&mut shake, 0.06, 35.0, 1.0, k);
                c.stats.shakes += 1;
                if c.player_control {
                    rumble.write(crate::fx::Rumble { strength: 200.0, duration: 10.0, source: "kong_slam" });
                }
            }
            FightEvent::FinisherSuccess => {
                impact_flash(&mut commands, &mut mats, &fx, rp + Vec3::Y * 3.0, 2.0);
                splash(&mut commands, &mut mats, &fx, rp, 3.0, true);
                c.stats.flashes += 1;
                c.stats.splashes += 1;
                // Kong's bank (07D _PJ_Kong) slot 0x12 Kong_break_jaw, rex bank KTrex_jaw_break [C names, L moment]
                sfx.write(crate::sfx::PlaySfx::at(pick(&defs, &["Kong_break_jaw", "Trex_growl"]), rp + Vec3::Y * 2.0));
                sfx.write(crate::sfx::PlaySfx::at(pick(&defs, &["KTrex_jaw_break", "Trex_growl"]), rp + Vec3::Y * 2.0));
            }
            // sound slots of the KT rex's bank (07D J_PNJ_KTREX_2, resource cb 0xa346e0) [C]: 3 KTrex_paf_small,
            // 4 KTrex_paf_big; the 03E rex hit sound when the user's sound set lacks them
            FightEvent::RexPaf { sound, .. } => {
                let want = if *sound == 3 { "KTrex_paf_small" } else { "KTrex_paf_big" };
                sfx.write(crate::sfx::PlaySfx::at(pick(&defs, &[want, "Trex_take_shoot"]), rp + Vec3::Y * 3.0));
            }
            FightEvent::RexGroundHit => {
                sfx.write(crate::sfx::PlaySfx::at(pick(&defs, &["KTrex_paf_small", "Trex_take_shoot"]), rp + Vec3::Y * 1.5));
            }
            // KT_ETAT_charge: sound 6 = KTrex_attack [C]
            FightEvent::RexAttack { kind: kk_mechanics::kong::fight::RexMove::Charge } => {
                sfx.write(crate::sfx::PlaySfx::at(pick(&defs, &["KTrex_attack", "Trex_attack"]), rp + Vec3::Y * 3.0));
            }
            // Kong's bank (07D _PJ_Kong): 9 / 10 Kong_paf_big / Kong_paf_small [C names, L choice by strength]
            FightEvent::KongStunned { strength, .. } => {
                splash(&mut commands, &mut mats, &fx, kp, 1.6, false);
                c.stats.splashes += 1;
                let want = if *strength >= 2 { "Kong_paf_big" } else { "Kong_paf_small" };
                sfx.write(crate::sfx::PlaySfx::at(pick(&defs, &[want]), kp + Vec3::Y * 4.0));
            }
            // the mash clip begins: Kong's sound 0xe Kong_grab_trex (k_ETAT_finish, end of 0xe6) [C]
            FightEvent::Anim { actor: Actor::Kong, id: 0xe7, .. } => {
                sfx.write(crate::sfx::PlaySfx::at(pick(&defs, &["Kong_grab_trex"]), kp + Vec3::Y * 4.0));
            }
            _ => {}
        }
    }
    // k_ETAT_finish 0x15: Kong's sound 0x38 Kong_grab_advantage once the cursor passes 0.75, re-armed
    // below 0.25 [C]
    if let Some(f) = c.fight.kong.finisher.as_ref().filter(|f| f.won_t.is_none()) {
        let u = f.mash.progress / FINISH_ANIM_LEN;
        let armed = advantage_armed.get_or_insert(true);
        if *armed && u > 0.75 {
            *armed = false;
            sfx.write(crate::sfx::PlaySfx::at(pick(&defs, &["Kong_grab_advantage"]), kp + Vec3::Y * 4.0));
        } else if u < 0.25 {
            *armed = true;
        }
    }
    // dodge / roll / charge dash: a spray trail behind Kong while he moves fast in a combat phase
    if c.kong_speed > 6.5 && rng.gen_bool(0.35) {
        splash(&mut commands, &mut mats, &fx, kp, 0.9, false);
        c.stats.splashes += 1;
    }
}

/// Kong's feet and knuckles touching the water: detected from the animated bones (like the rex's toes).
#[allow(clippy::too_many_arguments)]
pub fn kong_footsteps(
    mut commands: Commands,
    mut ctl: ResMut<KongCtl>,
    fx: Option<Res<FxAssets>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut shake: ResMut<CameraShake>,
    bones: Query<&KongBones, With<KongSceneRoot>>,
    gts: Query<&GlobalTransform>,
    cam: Query<&GlobalTransform, With<MainCam>>,
    mut sfx: EventWriter<crate::sfx::PlaySfx>,
    mut down: Local<[bool; 4]>,
) {
    let Some(fx) = fx else { return };
    let Ok(b) = bones.single() else { return };
    let listener = cam.single().map(|g| g.translation()).unwrap_or(Vec3::ZERO);
    let c = &mut *ctl;
    let base = c.kong_y;
    for (i, e) in b.contacts.iter().enumerate() {
        let Ok(g) = gts.get(*e) else { continue };
        let p = g.translation();
        let h = p.y - base;
        // ankles/knuckles planted below ~0.9 m, lifted above ~1.5 m [G, from the idle/run clips]
        if down[i] {
            if h > 1.5 {
                down[i] = false;
            }
        } else if h < 0.9 {
            down[i] = true;
            // only while moving: planted idle feet do not "step"
            if c.kong_speed > 0.6 {
                let sc = (0.9 + c.kong_speed * 0.14).min(2.2);
                splash(&mut commands, &mut mats, &fx, Vec3::new(p.x, base, p.z), sc, c.kong_speed > 5.0);
                c.stats.splashes += 1;
                c.stats.footsteps += 1;
                let k = rex_shake_k(listener.distance(p)).max(0.3);
                send_shake(&mut shake, 0.012 + c.kong_speed * 0.002, 50.0, 0.95, k);
                if i < 2 {
                    sfx.write(crate::sfx::PlaySfx { def: "Trex_footsteps_near", pos: Some(p), gain: 0.55 });
                }
            }
        }
    }
}

/// The rex's footfalls throw water as well.
pub fn rex_splash(
    mut commands: Commands,
    mut ev: EventReader<RexEvent>,
    mut ctl: ResMut<KongCtl>,
    fx: Option<Res<FxAssets>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let Some(fx) = fx else {
        ev.clear();
        return;
    };
    for e in ev.read() {
        if let RexEvent::Footstep { pos, strong } = e {
            let at = Vec3::new(pos.x, ctl.rex_y, pos.z);
            splash(&mut commands, &mut mats, &fx, at, if *strong { 2.8 } else { 1.8 }, *strong);
            ctl.stats.splashes += 1;
            ctl.stats.rex_footsteps += 1;
        }
    }
}

/// Shutter angle follows how fast Kong is moving: strikes and dodges blur, standing does not.
pub fn motion_blur_update(ctl: Res<KongCtl>, time: Res<Time>, mut cam: Query<&mut MotionBlur, With<MainCam>>) {
    let Ok(mut mb) = cam.single_mut() else { return };
    let k = &ctl.fight.kong;
    let phase_fast = !matches!(k.phase, kk_mechanics::kong::combat::Phase::None | kk_mechanics::kong::combat::Phase::Recover);
    let target = if phase_fast {
        0.9
    } else {
        (0.15 + ctl.kong_speed / 10.0).clamp(0.15, 0.8)
    };
    let f = (6.0 * time.delta_secs()).min(1.0);
    mb.shutter_angle += (target - mb.shutter_angle) * f;
}
