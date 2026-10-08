//! Kong's third-person camera.
//!
//! Recovered (`kk_mechanics::kong::camera`, `CM_Kong_Mode_Normal@0x757030`): the mode selector order,
//! the normal mode parameters (distance 15, height 3, look-ahead 7) and the look height 3.8 over the
//! feet. The framing below uses them as the base and adds what the reference clip shows [G]: the camera
//! sits behind-and-beside Kong so Kong and the rex are side by side, pulls in close on Kong's face for the
//! chest pound / roar, and frames the jaw-break from the side.
//!
//! Collision [G]: every desired camera position is tested with a ray from Kong's head (`LevelCollision::ray_hit`:
//! solid boxes, rock/wall columns, ground) and pulled in on a hit; both sides of the Kong -> rex line are tried
//! and the freer one is kept with hysteresis; the smoothed position is clipped again so the camera can never be
//! eased through a wall. Two styles: `Player` (the game's fight selector camera) and `Cinema` (the batch /
//! AI-watching camera: lower, closer, a slow drift and a little hand-held sway, like the reference clip).

use crate::kong::{ground_y_pub, KongCtl};
use crate::player::{MainCam, Player};
use crate::world::Arena;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, PrimaryWindow};
use kk_mechanics::kong::camera::{select, CamMode, SelectorInputs, NORMAL_LOOK_HEIGHT, NORMAL_PARAMS};
use kk_mechanics::kong::combat::Phase;
use kk_mechanics::kong::fight::KongMode;

#[derive(Debug, Clone)]
pub struct KongCam {
    pub init: bool,
    pub pos: Vec3,
    pub look: Vec3,
    /// manual orbit around the fight (mouse / right stick), radians
    pub orbit: f32,
    /// 0..1 blend into the face close-up / the finisher framing
    pub close: f32,
    pub fin: f32,
    pub fov: f32,
    pub mode: CamMode,
    /// which side of the Kong -> rex line the camera hangs on (+1 / -1); flips when that side is a wall
    pub side: f32,
    /// pull-in statistics for the batch checks: frames in which the ray hit / total frames, closest approach to Kong
    pub hits: u32,
    pub frames: u32,
    pub min_dist: f32,
    /// frames in which the clipped camera still had a solid between it and Kong's head (must stay 0)
    pub blocked: u32,
    /// close-up in progress for the batch shots
    pub clip_t: f32,
}

impl Default for KongCam {
    fn default() -> Self {
        Self {
            init: false,
            pos: Vec3::ZERO,
            look: Vec3::ZERO,
            orbit: 0.0,
            close: 0.0,
            fin: 0.0,
            fov: 0.95,
            mode: CamMode::Normal,
            side: 1.0,
            hits: 0,
            frames: 0,
            min_dist: f32::MAX,
            blocked: 0,
            clip_t: 0.0,
        }
    }
}

/// Vertical FOV of the Kong camera and of the close-ups [G].
const FOV_NORMAL: f32 = 0.98;
const FOV_CLOSE: f32 = 0.88;

fn expo(dt: f32, rate: f32) -> f32 {
    1.0 - (-dt * rate).exp()
}

/// Pull `want` toward `from` until the segment is free of solids (keeps `keep` metres before the hit).
/// Returns the clipped point and whether the ray hit.
fn clip_ray(arena: &Arena, from: Vec3, want: Vec3, keep: f32, min_len: f32) -> (Vec3, bool) {
    let Some(level) = &arena.level else { return (want, false) };
    let d = want - from;
    let len = d.length();
    if len < 1e-3 {
        return (want, false);
    }
    let dir = d / len;
    match level.ray_hit(from, dir, len) {
        Some(t) => (from + dir * (t - keep).max(min_len.min(len)), true),
        None => (want, false),
    }
}

/// Fraction of the way (0..1) the ray from `from` to `want` stays free.
fn free_fraction(arena: &Arena, from: Vec3, want: Vec3) -> f32 {
    let Some(level) = &arena.level else { return 1.0 };
    let d = want - from;
    let len = d.length().max(1e-3);
    level.ray_hit(from, d / len, len).map_or(1.0, |t| t / len)
}

#[allow(clippy::too_many_arguments)]
pub fn kong_camera(
    time: Res<Time>,
    arena: Res<Arena>,
    mut ctl: ResMut<KongCtl>,
    jack: Query<&Transform, (With<Player>, Without<MainCam>)>,
    mut cam: Query<(&mut Transform, &mut Projection, &GlobalTransform), With<MainCam>>,
    motion: Res<AccumulatedMouseMotion>,
    windows: Query<&Window, With<PrimaryWindow>>,
    gamepads: Query<&Gamepad>,
) {
    let c = &mut *ctl;
    if !(c.player_control || c.cinematic) {
        return;
    }
    let (Ok(jt), Ok((mut ctf, mut proj, cgt))) = (jack.single(), cam.single_mut()) else { return };
    let dt = time.delta_secs().min(0.1);
    let cinema = c.cinematic && !c.player_control;
    let grabbed = windows.single().map(|w| w.cursor_options.grab_mode != CursorGrabMode::None).unwrap_or(false);

    // manual orbit, recentres slowly
    let mut orbit_in = 0.0;
    if grabbed {
        orbit_in -= motion.delta.x * 0.0035;
    }
    for g in &gamepads {
        orbit_in -= g.right_stick().x * 1.8 * dt;
    }
    c.cam.orbit = ((c.cam.orbit + orbit_in) * (1.0 - 0.35 * dt)).clamp(-2.2, 2.2);

    let k = c.kong_world();
    let r = c.rex_world();
    let head = if c.kong_head.distance(k) > 0.5 { c.kong_head } else { k + Vec3::Y * 5.0 };
    let to = Vec2::new(r.x - k.x, r.z - k.z);
    let dist = to.length().max(0.5);
    let dir = if dist > 0.6 { to / dist } else { CamDir::fallback(c) };
    let perp0 = Vec2::new(-dir.y, dir.x);
    let mid = k.lerp(r, 0.45);

    // ---- normal framing: [C] distance 15 / height 3 / look height 3.8; wider when the fighters part
    let base_d = NORMAL_PARAMS[0] * if cinema { 0.80 } else { 1.0 };
    let wide = ((dist - 8.0) * 0.5).clamp(0.0, 9.0);
    let dcam = base_d + wide;
    let (theta, hgt) = if cinema { (0.80f32, 2.3) } else { (0.95f32, NORMAL_PARAMS[1] + 0.8) };
    // slow drift of the viewing angle in the cinema style
    let drift = if cinema { (c.t * 0.21).sin() * 0.16 } else { 0.0 };
    let (so, co) = (c.cam.orbit + drift).sin_cos();
    let offset_for = |sd: f32| {
        // behind Kong (away from the rex) and to one side
        let a = (-dir * theta.cos() + perp0 * (theta.sin() * sd)) * dcam;
        Vec2::new(a.x * co - a.y * so, a.x * so + a.y * co)
    };
    let look_h = k.y + NORMAL_LOOK_HEIGHT;
    let n_look = Vec3::new(mid.x, look_h, mid.z);
    let anchor = n_look.lerp(head, 0.35);
    let want_for = |sd: f32| {
        let o = offset_for(sd);
        Vec3::new(mid.x + o.x, k.y + hgt + (dist - 8.0).max(0.0) * 0.06, mid.z + o.y)
    };
    // both sides: how far the ray from the anchor stays free; keep the side we are on unless it is clearly worse
    let (fa, fb) = (free_fraction(&arena, anchor, want_for(1.0)), free_fraction(&arena, anchor, want_for(-1.0)));
    let (cur, oth) = if c.cam.side > 0.0 { (fa, fb) } else { (fb, fa) };
    if cur < 0.75 && oth > cur + 0.2 {
        c.cam.side = -c.cam.side;
    }
    let side = c.cam.side;
    let mut n_pos = want_for(side);
    // pushing the camera right through a pillar is not an option: pull in along the ray
    let (clipped, hit) = clip_ray(&arena, anchor, n_pos, 0.8, 1.5);
    n_pos = clipped;
    // if the pull-in left it too close (cramped), lift it for a look down over the obstacle
    if hit && n_pos.distance(anchor) < 9.0 {
        n_pos.y += (9.0 - n_pos.distance(anchor)) * 0.5;
    }

    // ---- finisher: side-on, a little higher, between the two heads
    let perp = perp0 * side;
    let f_want = Vec3::new(mid.x + perp.x * 11.0, k.y + 5.0, mid.z + perp.y * 11.0);
    let f_look = Vec3::new(mid.x, k.y + 4.6, mid.z);
    let (f_pos, _) = clip_ray(&arena, f_look, f_want, 0.8, 1.5);

    // ---- roar / chest pound close-up: in front of Kong's head, looking up at the face (reference s_054..s_056)
    let kf = KongCtl::dir_xz(c.fight.kong.facing);
    let kr = Vec2::new(-kf.y, kf.x);
    let mut c_best = (f32::MAX, Vec3::ZERO);
    for sgn in [1.0f32, -1.0] {
        let want = Vec3::new(head.x + kf.x * 11.5 + kr.x * 3.2 * sgn, head.y - 1.4, head.z + kf.y * 11.5 + kr.y * 3.2 * sgn);
        let fr = free_fraction(&arena, head, want);
        if (1.0 - fr) < c_best.0 {
            c_best = (1.0 - fr, want);
        }
    }
    let (c_pos, _) = clip_ray(&arena, head, c_best.1, 0.8, 1.2);
    let c_look = head - Vec3::Y * 0.9;

    // blend factors
    let f = &c.fight;
    let roaring = (f.kong.phase == Phase::ChestPound && f.kong.anim_t > 0.35) || (f.kong.mode == KongMode::Victory && f.kong.victory_t > 0.2);
    let finishing = f.kong.mode == KongMode::Finisher;
    c.cam.close += ((roaring as u8 as f32) - c.cam.close) * expo(dt, 3.0);
    c.cam.fin += ((finishing as u8 as f32) - c.cam.fin) * expo(dt, 2.0);
    c.cam.mode = select(&SelectorInputs { finish: finishing, ..Default::default() });

    let t_pos = n_pos.lerp(f_pos, c.cam.fin).lerp(c_pos, c.cam.close);
    let t_look = n_look.lerp(f_look, c.cam.fin).lerp(c_look, c.cam.close);

    if !c.cam.init {
        // start from where the first-person camera is, so the cut is an eased glide
        c.cam.init = true;
        c.cam.pos = cgt.translation();
        c.cam.look = cgt.translation() + cgt.forward().as_vec3() * 10.0;
        if cinema {
            // a cut, not a glide
            c.cam.pos = t_pos;
            c.cam.look = t_look;
        }
    }
    let fast = 3.2 + 4.0 * c.cam.close;
    c.cam.pos = c.cam.pos.lerp(t_pos, expo(dt, fast));
    c.cam.look = c.cam.look.lerp(t_look, expo(dt, 6.0 + 4.0 * c.cam.close));
    // the eased position may still sweep through a wall (Kong moved, side flipped): clip it again, instantly
    c.cam.frames += 1;
    let (p2, hit2) = clip_ray(&arena, head, c.cam.pos, 0.6, 0.9);
    if hit2 {
        c.cam.hits += 1;
        if std::env::var("KK_CAM_DEBUG").is_ok() {
            info!("cam pull-in: head {:?} want {:?} -> {:?} t {:.1} close {:.2} fin {:.2}", head, c.cam.pos, p2, c.t, c.cam.close, c.cam.fin);
        }
        c.cam.pos = p2;
    }
    if let Some(l) = arena.level.as_ref() {
        // pulled in to almost Kong's head (low camera over a rising bank, Kong down by a wall): look for a clear spot
        // a little higher and around, keeping the same general side [G]
        if c.cam.pos.distance(head) < 3.0 {
            let back = (c.cam.pos - head).with_y(0.0).normalize_or(Vec3::X);
            'search: for (up, out) in [(2.0f32, 4.0f32), (3.0, 5.0), (4.5, 6.0), (6.0, 7.0)] {
                for rot in [0.0f32, 0.45, -0.45, 0.9, -0.9, 1.4, -1.4] {
                    let d = Quat::from_rotation_y(rot) * back;
                    let cand = head + d * out + Vec3::Y * up;
                    if l.segment_clear(head, cand) {
                        c.cam.pos = cand;
                        break 'search;
                    }
                }
            }
        }
        if !l.segment_clear(head, c.cam.pos) {
            // degenerate case (Kong down against a wall): go straight up / toward the arena centre until the ray is free
            let inward = (c.center - head).with_y(0.0).normalize_or_zero();
            for (up, inw) in [(3.0, 0.0), (4.5, 2.0), (6.0, 4.0), (9.0, 6.0)] {
                let cand = head + Vec3::Y * up + inward * inw;
                if l.segment_clear(head, cand) {
                    c.cam.pos = cand;
                    break;
                }
            }
        }
    }
    if arena.level.as_ref().is_some_and(|l| !l.segment_clear(head, c.cam.pos)) {
        c.cam.blocked += 1;
        if c.cam.blocked <= 6 && std::env::var("KK_CAM_DEBUG").is_ok() {
            info!("cam blocked: head {:?} cam {:?} t {:.1} hit {:?}", head, c.cam.pos, c.t, arena.level.as_ref().and_then(|l| l.ray_hit(head, (c.cam.pos - head).normalize(), c.cam.pos.distance(head))));
        }
    }
    c.cam.min_dist = c.cam.min_dist.min(c.cam.pos.distance(head));
    // never inside the ground / under the water
    let g = ground_y_pub(&arena, c, c.cam.pos.x, c.cam.pos.z, k.y + 3.0);
    let floor = if crate::scene::swamp() { g.max(crate::swamp::water_y()) } else { g };
    c.cam.pos.y = c.cam.pos.y.max(floor + 1.5);

    // hand-held sway in the cinema style
    let mut eye = c.cam.pos;
    if cinema {
        let t = c.t;
        eye += Vec3::new((t * 0.9).sin() * 0.06, (t * 1.3).sin() * 0.05, (t * 0.7).cos() * 0.06);
    }
    let world = Transform::from_translation(eye).looking_at(c.cam.look, Vec3::Y);
    // the camera is a child of Jack (frozen): express the world pose in his frame
    let jm = Mat4::from_scale_rotation_translation(Vec3::ONE, jt.rotation, jt.translation);
    *ctf = Transform::from_matrix(jm.inverse() * world.compute_matrix());
    // camera-relative stick for the pad
    let fwd = world.forward().as_vec3();
    c.cam_fwd = Vec2::new(fwd.x, fwd.z).normalize_or(Vec2::new(0.0, -1.0));
    // fov
    let want = FOV_NORMAL + (FOV_CLOSE - FOV_NORMAL) * c.cam.close;
    c.cam.fov += (want - c.cam.fov) * expo(dt, 3.0);
    if let Projection::Perspective(pp) = &mut *proj {
        pp.fov = c.cam.fov;
    }
}

/// Fallback direction when Kong and the rex coincide: Kong's facing.
struct CamDir;
impl CamDir {
    fn fallback(c: &KongCtl) -> Vec2 {
        KongCtl::dir_xz(c.fight.kong.facing)
    }
}
