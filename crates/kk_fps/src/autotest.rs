//! Scripted smoke test: `KK_AUTOTEST=<dir>` drives inputs, saves screenshots and a log,
//! then exits. Used to verify the slice without a person at the keyboard.

use crate::anim::GameState;
use crate::player::Player;
use crate::rex::Rex;
use crate::weapons::Arsenal;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use std::io::Write;
use std::path::PathBuf;

#[derive(Resource)]
struct AutoTest {
    dir: PathBuf,
    t: f32,
    step: usize,
    log: std::fs::File,
    last_log: f32,
}

#[derive(Clone, Copy)]
enum Act {
    Shot(&'static str),
    Press(KeyCode),
    Click,
    HoldFire(bool),
    Aim(bool),
    Quit,
}

const SCRIPT: &[(f32, Act)] = &[
    (2.0, Act::Shot("01_colt_hip")),
    (2.5, Act::Click),
    (2.53, Act::Shot("02_colt_hip_fire")),
    (3.0, Act::Aim(true)),
    (3.8, Act::Shot("03_colt_aim")),
    (4.0, Act::Click),
    (4.05, Act::Shot("04_colt_aim_fire")),
    (4.6, Act::Aim(false)),
    (4.8, Act::Press(KeyCode::KeyR)),
    (5.4, Act::Shot("05_colt_reload")),
    (7.0, Act::Press(KeyCode::Digit2)),
    (8.0, Act::Shot("06_tommy_hip")),
    (8.2, Act::HoldFire(true)),
    (8.6, Act::Shot("07_tommy_fire")),
    (9.4, Act::HoldFire(false)),
    (9.5, Act::Aim(true)),
    (10.3, Act::Shot("08_tommy_aim")),
    (10.5, Act::Aim(false)),
    (10.6, Act::Press(KeyCode::Digit3)),
    (11.6, Act::Shot("09_shotgun_hip")),
    (11.8, Act::Aim(true)),
    (12.6, Act::Shot("10_shotgun_aim")),
    (12.7, Act::Click),
    (12.75, Act::Shot("11_shotgun_aim_fire")),
    (13.2, Act::Aim(false)),
    (13.3, Act::Press(KeyCode::KeyR)),
    (13.7, Act::Shot("12_shotgun_reload")),
    (15.5, Act::Press(KeyCode::Digit4)),
    (16.5, Act::Shot("13_sniper_hip")),
    (16.6, Act::Aim(true)),
    (17.6, Act::Shot("14_sniper_scope")),
    (17.7, Act::Click),
    (18.2, Act::Aim(false)),
    (18.3, Act::Press(KeyCode::Digit3)),
    (20.0, Act::Shot("15_rex_close")),
    (20.5, Act::Click),
    (21.2, Act::Click),
    (21.9, Act::Click),
    (22.0, Act::Shot("16_rex_shot")),
    (24.0, Act::Shot("17_rex_bite")),
    (27.0, Act::Shot("18_after")),
    (31.0, Act::Shot("19_end")),
    (32.0, Act::Quit),
];

pub struct AutoTestPlugin;

impl Plugin for AutoTestPlugin {
    fn build(&self, app: &mut App) {
        let Ok(dir) = std::env::var("KK_AUTOTEST") else { return };
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).ok();
        let log = std::fs::File::create(dir.join("autotest.log")).expect("log");
        // Deterministic 30 Hz simulation regardless of how slow the renderer is.
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f32(1.0 / 30.0),
        ));
        app.insert_resource(AutoTest { dir, t: 0.0, step: 0, log, last_log: -1.0 })
            .add_systems(PreUpdate, drive.after(bevy::input::InputSystem).run_if(in_state(GameState::Playing)));
    }
}

#[allow(clippy::too_many_arguments)]
fn drive(
    mut commands: Commands,
    time: Res<Time>,
    mut at: ResMut<AutoTest>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    players: Query<&Player>,
    rex: Query<(&Rex, &Transform)>,
    ptf: Query<&Transform, With<Player>>,
    arsenal: Res<Arsenal>,
    mut exit: EventWriter<AppExit>,
    mut held: Local<(bool, bool)>,
) {
    // Fixed simulated step keeps the script deterministic even on a slow software renderer.
    at.t += time.delta_secs();
    if held.0 { mouse.press(MouseButton::Left); }
    if held.1 { mouse.press(MouseButton::Right); }
    if at.t - at.last_log >= 0.5 {
        at.last_log = at.t;
        let (p, pt) = (players.single().ok(), ptf.single().ok());
        let line = match (rex.single(), p, pt) {
            (Ok((r, rt)), Some(p), Some(pt)) => format!(
                "t={:5.2} jack={:?} pos=({:.1},{:.1}) weapon={} mag={} | rex={} hp={:.0} pos=({:.1},{:.1}) dist={:.1} gait={} speed={:.1}",
                at.t, p.wounds.state, pt.translation.x, pt.translation.z, arsenal.current().name,
                arsenal.mag[arsenal.index], r.label(), r.hp, rt.translation.x, rt.translation.z,
                rt.translation.distance(pt.translation), r.gait, r.speed
            ),
            _ => format!("t={:5.2} (entities not ready)", at.t),
        };
        println!("{line}");
        writeln!(at.log, "{line}").ok();
    }
    while at.step < SCRIPT.len() && SCRIPT[at.step].0 <= at.t {
        let act = SCRIPT[at.step].1;
        at.step += 1;
        match act {
            Act::Shot(name) => {
                let path = at.dir.join(format!("{name}.png"));
                commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
            }
            Act::Press(k) => keys.press(k),
            Act::Click => mouse.press(MouseButton::Left),
            Act::HoldFire(on) => {
                held.0 = on;
                if !on { mouse.release(MouseButton::Left); }
            }
            Act::Aim(on) => {
                held.1 = on;
                if !on { mouse.release(MouseButton::Right); }
            }
            Act::Quit => {
                exit.write(AppExit::Success);
            }
        }
    }
    // release one-shot presses next frame
    for k in [KeyCode::KeyR, KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4] {
        if keys.pressed(k) && !keys.just_pressed(k) { keys.release(k); }
    }
    if !held.0 && mouse.pressed(MouseButton::Left) && !mouse.just_pressed(MouseButton::Left) {
        mouse.release(MouseButton::Left);
    }
}

// ---------------------------------------------------------------------------
// Gallery: KK_GALLERY=<dir> renders every weapon in every listed arms clip.
// ---------------------------------------------------------------------------

pub const GALLERY_CLIPS: &[&str] = &[
    "idle_c15", "idle_c16", "idle_c17", "idle_short_c19", "idle_short_c20", "idle_short_c21",
    "idle_short_c23", "idle_short_c24", "idle_short_c25", "idle_short_c38", "idle_c14",
    "idle_c01", "idle_short_c07", "reload_c37", "reload_l_short_c36", "reload_l_short_c40",
    "fire_c23", "fire_c24", "move_c39",
];

#[derive(Resource)]
struct Gallery {
    dir: PathBuf,
    jobs: Vec<(usize, String)>,
    i: usize,
    t: f32,
}

pub struct GalleryPlugin;

impl Plugin for GalleryPlugin {
    fn build(&self, app: &mut App) {
        let Ok(dir) = std::env::var("KK_GALLERY") else { return };
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).ok();
        let mut jobs = Vec::new();
        for w in 0..4 {
            for c in GALLERY_CLIPS {
                jobs.push((w, c.to_string()));
            }
        }
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f32(1.0 / 30.0),
        ))
        .insert_resource(Gallery { dir, jobs, i: 0, t: -1.5 })
        .add_systems(Update, gallery.run_if(in_state(GameState::Playing)));
    }
}

fn gallery(
    mut commands: Commands,
    time: Res<Time>,
    mut g: ResMut<Gallery>,
    mut arsenal: ResMut<Arsenal>,
    rigs: Res<crate::anim::Rigs>,
    mut exit: EventWriter<AppExit>,
) {
    g.t += time.delta_secs();
    if g.i >= g.jobs.len() {
        if g.t > 1.0 {
            exit.write(AppExit::Success);
        }
        return;
    }
    let (w, clip) = g.jobs[g.i].clone();
    if g.t < 0.0 {
        return;
    }
    // first frame of a job: select weapon + clip
    if arsenal.index != w || arsenal.weapon_entity.is_none() {
        arsenal.index = w;
        arsenal.wanted_weapon = Some(w);
        arsenal.action = crate::weapons::ArmsAction::Idle;
        g.t = 0.0;
    }
    let full = rigs.arms.find(&clip).map(|s| s.to_string());
    arsenal.debug_clip = full.and_then(|f| rigs.arms.names.iter().position(|n| *n == f));
    if g.t >= 0.5 {
        let path = g.dir.join(format!("w{}_{}.png", w + 1, clip));
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
        g.i += 1;
        g.t = -0.1;
    }
}
