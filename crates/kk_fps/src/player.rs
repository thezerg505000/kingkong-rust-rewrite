//! Jack: first-person controller, camera, wound model and the arms viewmodel.
//!
//! Logic from `kk_mechanics`: movement speed/ramp (`jack::MoveState`, J*), free look
//! (`jack::look_step`, C*), eye height and FOV smoothing (`jack::smooth_eye/smooth_fov`), and the
//! wound model (`wounds::Wounds`, H01..H04). Input mapping (keyboard/mouse/pad), collision and the
//! viewmodel stay here.

use crate::anim::{GameState, Rigs};
use crate::spec::*;
use crate::world::{Arena, VIEW_LAYER};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::render::view::RenderLayers;
use bevy::window::{CursorGrabMode, PrimaryWindow};
use kk_mechanics::jack as jk;
use kk_mechanics::wounds::{HitOutcome, WoundState, Wounds};
use std::f32::consts::PI;

/// Mouse look, radians per pixel. [G] (CAM_MouseSensitivity is a level variable)
pub const MOUSE_SENS: f32 = 0.0022;
/// Look sensitivity fed to `jack::look_step` (`G+0x4dbc`, default unknown) [G]: 0.05 makes the
/// recovered factors (2.0, 1.75) come out as 2.0 / 1.75 rad/s at full stick (x2 * sens * 10 * factor).
pub const LOOK_SENS: f32 = 0.05;

#[derive(Component)]
pub struct Player {
    pub yaw: f32,
    pub pitch: f32,
    pub vel: Vec3,
    pub eye: f32,
    pub crouch: bool,
    pub run: bool,
    pub aiming: bool,
    pub moving: f32,
    /// wound model: statuses 0 healthy / 1 wounded / 2 recovering / 3 dead (H01..H04)
    pub wounds: Wounds,
    /// movement state (smoothed speed + weapon-ready ramp), `jack::MoveState`
    pub mv: jk::MoveState,
    /// look ramp (`jack::LookState`)
    pub look: jk::LookState,
    pub death_cause: &'static str,
    pub bob_phase: f32,
    pub kick: f32,
}

impl Player {
    pub fn new() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
            vel: Vec3::ZERO,
            eye: EYE_STAND,
            crouch: false,
            run: false,
            aiming: false,
            moving: 0.0,
            wounds: Wounds::new(0),
            mv: jk::MoveState::default(),
            look: jk::LookState::default(),
            death_cause: "",
            bob_phase: 0.0,
            kick: 0.0,
        }
    }
    pub fn alive(&self) -> bool {
        self.wounds.state != WoundState::Dead
    }
    /// Stimulus "paf" with the given flag word (`H_exec_ch_Stimulus_Paf`): V-Rex bite 0x4104 wounds a
    /// healthy/recovering Jack, the grab kill 0x4a10 carries the 0x200 kill bit (H02, X04).
    /// Returns the outcome; a kill records `cause`.
    pub fn paf(&mut self, cause: &'static str, flags: u32) -> HitOutcome {
        let was_alive = self.alive();
        let out = self.wounds.hit(flags);
        if was_alive && !self.alive() {
            self.death_cause = cause;
        }
        out
    }
}

#[derive(Component)]
pub struct MainCam;
#[derive(Component)]
pub struct ViewCam;
/// Parent of the arms scene; carries procedural bob / ADS / swap offsets.
#[derive(Component)]
pub struct ViewModel;
/// The arms SceneRoot plus the rig entities found once it spawned.
#[derive(Component)]
pub struct ArmsScene;
#[derive(Component)]
pub struct ArmsRig {
    pub cam_bone: Entity,
    pub socket: Entity,
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerSet;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Playing), spawn_player)
            .add_systems(
                Update,
                (
                    cursor_grab,
                    look,
                    movement,
                    wounds,
                    anchor_viewmodel,
                    viewmodel_offsets,
                )
                    .chain()
                    .in_set(PlayerSet)
                    .run_if(in_state(GameState::Playing))
                    // frozen while the player is Kong (kong.rs)
                    .run_if(crate::kong::jack_active),
            );
    }
}

fn spawn_player(mut commands: Commands, rigs: Res<Rigs>, arena: Res<Arena>) {
    let mut jack = Player::new();
    jack.yaw = arena.player_yaw;
    let level = arena.level.is_some();
    let player = commands
        .spawn((
            Name::new("Jack"),
            jack,
            Transform::from_translation(arena.player_spawn),
            Visibility::default(),
        ))
        .id();
    let cam = commands
        .spawn((
            Name::new("MainCam"),
            MainCam,
            // depth readable by the god-ray occlusion mask
            Camera3d {
                depth_texture_usages: (bevy::render::render_resource::TextureUsages::RENDER_ATTACHMENT
                    | bevy::render::render_resource::TextureUsages::TEXTURE_BINDING)
                    .into(),
                ..default()
            },
            Camera {
                hdr: true,
                ..default()
            },
            Projection::from(PerspectiveProjection {
                fov: vertical_fov(FOV_DEFAULT),
                near: 0.05,
                far: 2000.0,
                ..default()
            }),
            bevy::core_pipeline::tonemapping::Tonemapping::TonyMcMapface,
            // both cameras share one view target, so they must agree on MSAA (ViewCam is Off)
            Msaa::Off,
            if level {
                // thick teal jungle haze of the original level [G density]
                DistanceFog {
                    color: crate::world::FOG_COLOR,
                    falloff: FogFalloff::Exponential { density: 0.020 },
                    ..default()
                }
            } else {
                DistanceFog {
                    color: Color::srgb(0.52, 0.58, 0.55),
                    falloff: FogFalloff::Linear { start: 40.0, end: 260.0 },
                    ..default()
                }
            },
            Transform::from_xyz(0.0, EYE_STAND, 0.0),
        ))
        .id();
    commands.entity(player).add_child(cam);

    // Viewmodel camera: draws only the arms/weapon layer on top, with its own fixed FOV
    // so the arms never clip into the world and do not zoom with ADS.
    let view_cam = commands
        .spawn((
            Name::new("ViewCam"),
            ViewCam,
            Camera3d::default(),
            Camera {
                order: 1,
                hdr: true,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            Projection::from(PerspectiveProjection {
                fov: vertical_fov(FOV_DEFAULT),
                near: 0.01,
                far: 10.0,
                ..default()
            }),
            // The viewmodel camera shares the world camera's view target, so its tonemapping
            // pass would re-tonemap and re-grade the whole frame (that was the teal cast).
            // Draw it untonemapped instead.
            bevy::core_pipeline::tonemapping::Tonemapping::None,
            RenderLayers::layer(VIEW_LAYER),
            // same MSAA as the world camera, so both share one view target (the god-ray
            // post-process needs Msaa::Off on the world camera)
            Msaa::Off,
            // own neutral fill so blued steel and wood read instead of going black [G]
            AmbientLight { color: Color::srgb(0.85, 0.85, 0.85), brightness: 900.0, ..default() },
            Transform::default(),
        ))
        .id();
    commands.entity(cam).add_child(view_cam);

    let vm = commands
        .spawn((
            Name::new("ViewModel"),
            ViewModel,
            Transform::default(),
            Visibility::default(),
        ))
        .id();
    commands.entity(cam).add_child(vm);
    let arms_scene = rigs.arms_scene.clone();
    let arms = commands
        .spawn((
            Name::new("ArmsScene"),
            ArmsScene,
            SceneRoot(arms_scene),
            Transform::from_rotation(Quat::from_rotation_y(PI)),
        ))
        .observe(on_arms_ready)
        .id();
    commands.entity(vm).add_child(arms);
}

fn on_arms_ready(
    trigger: Trigger<bevy::scene::SceneInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    names: Query<&Name>,
    meshes: Query<(), With<Mesh3d>>,
    players: Query<Entity, With<AnimationPlayer>>,
    rigs: Res<Rigs>,
) {
    let root = trigger.target();
    let mut cam_bone = None;
    let mut socket = None;
    for e in children.iter_descendants(root) {
        if let Ok(n) = names.get(e) {
            match n.as_str() {
                "B_Jaf_Camera" => cam_bone = Some(e),
                "WeaponSocket" => socket = Some(e),
                _ => {}
            }
        }
        if meshes.contains(e) {
            commands.entity(e).insert((
                RenderLayers::layer(VIEW_LAYER),
                bevy::pbr::NotShadowCaster,
                bevy::pbr::NotShadowReceiver,
                // skinned AABBs stay at the bind pose; the animated arms sit ~0.6 m higher
                bevy::render::view::NoFrustumCulling,
            ));
        }
    }
    let player = crate::anim::attach_graph(&mut commands, root, &children, &players, &rigs.arms.graph);
    match (cam_bone, socket, player) {
        (Some(cam_bone), Some(socket), Some(player)) => {
            commands.entity(root).insert((
                ArmsRig { cam_bone, socket },
                crate::anim::RigPlayer {
                    player,
                    current: String::new(),
                },
            ));
        }
        _ => error!("arms glb is missing B_Jaf_Camera / WeaponSocket / AnimationPlayer"),
    }
}

fn cursor_grab(
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let Ok(mut w) = windows.single_mut() else { return };
    if mouse.just_pressed(MouseButton::Left) && w.cursor_options.grab_mode == CursorGrabMode::None {
        w.cursor_options.grab_mode = CursorGrabMode::Locked;
        w.cursor_options.visible = false;
    }
    if keys.just_pressed(KeyCode::Escape) {
        w.cursor_options.grab_mode = CursorGrabMode::None;
        w.cursor_options.visible = true;
    }
}

fn look(
    time: Res<Time>,
    motion: Res<AccumulatedMouseMotion>,
    windows: Query<&Window, With<PrimaryWindow>>,
    gamepads: Query<&Gamepad>,
    mut q: Query<(&mut Player, &mut Transform), Without<MainCam>>,
    mut cam: Query<(&mut Transform, &mut Projection), With<MainCam>>,
    weapons: Res<crate::weapons::Arsenal>,
) {
    let Ok((mut p, mut tf)) = q.single_mut() else { return };
    let Ok((mut ctf, mut proj)) = cam.single_mut() else { return };
    let dt = time.delta_secs();
    let grabbed = windows
        .single()
        .map(|w| w.cursor_options.grab_mode != CursorGrabMode::None)
        .unwrap_or(false);
    if p.alive() {
        let mode = jk::LookMode { aim: p.aiming, run: p.run, ..Default::default() };
        let (fx, fy, _) = jk::look_factors(mode);
        if grabbed {
            // Mouse keeps the same normal/aim ratio as the stick factors.
            let k = MOUSE_SENS * fx / jk::LOOK_NORMAL.0;
            p.yaw -= motion.delta.x * k;
            p.pitch -= motion.delta.y * k * (fy / fx);
        }
        // Right stick: CM_Cam free look (raw stick, 0.01 snap, yaw ramp above 0.95) [C]
        let mut stick = (0.0, 0.0);
        for g in &gamepads {
            let s = g.right_stick();
            if s.length() > Vec2::new(stick.0, stick.1).length() {
                stick = (s.x, s.y);
            }
        }
        let inp = jk::LookInput { stick, sensitivity: LOOK_SENS, invert_y: false, mode, locked_chase: false };
        let mut st = p.look;
        let (dyaw, dpitch) = jk::look_step(&mut st, &inp, dt);
        p.look = st;
        p.yaw += dyaw;
        p.pitch -= dpitch; // stick up looks up (the pitch axis sign is [L])
        // recoil kick recovers smoothly [G]
        p.pitch += p.kick;
        p.kick = 0.0;
        // forward.z clamped to +-0.95 (CM_Cam) [C]
        let limit = jk::PITCH_LIMIT_SIN.asin();
        p.pitch = p.pitch.clamp(-limit, limit);
    }
    tf.rotation = Quat::from_rotation_y(p.yaw);
    let roll = if p.alive() { 0.0 } else { (p.wounds.dead_time / DEATH_SEQUENCE_S).min(1.0) * 0.6 };
    ctf.rotation = Quat::from_rotation_x(p.pitch) * Quat::from_rotation_z(roll);
    // eye height smoothing (CM_Cam: target 1.6 / crouch 0.8, k = 5*dt)
    let target_eye = if !p.alive() { 0.35 } else { jk::eye_target(0.0, false, p.crouch, false) };
    p.eye = jk::smooth_eye(p.eye, target_eye, dt);
    // head bob [L amplitudes 0.03..0.06, freq ~ step rate]
    let bob = (p.bob_phase).sin().abs() * 0.04 * p.moving;
    ctf.translation = Vec3::new(0.0, p.eye + bob, 0.0);
    // FOV / ADS: target 1.2 / 0.6 / sniper 0.3, smoothed with k = 5*dt (CM_Cam) [C]
    let target = jk::fov_target(p.aiming, false, weapons.current().id as i32, None);
    if let Projection::Perspective(pp) = &mut *proj {
        let cur_h = 2.0 * ((pp.fov * 0.5).tan() / 0.75).atan();
        pp.fov = vertical_fov(jk::smooth_fov(cur_h, target, dt));
    }
}

fn movement(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    arena: Res<Arena>,
    mut q: Query<(&mut Player, &mut Transform)>,
    mut steps: EventWriter<crate::events::JackEvent>,
) {
    let Ok((mut p, mut tf)) = q.single_mut() else { return };
    let dt = time.delta_secs();
    let mut stick = Vec2::ZERO;
    // |stick| as H_exec_select_action sees it (`jack::stick_magnitude`: per-axis 0.15 deadzone, not clamped)
    let mut mag = 0.0_f32;
    let mut run = keys.pressed(KeyCode::ShiftLeft);
    let mut crouch_toggle = keys.just_pressed(KeyCode::ControlLeft) || keys.just_pressed(KeyCode::KeyC);
    let mut aim = mouse.pressed(MouseButton::Right);
    if keys.pressed(KeyCode::KeyW) { stick.y += 1.0; }
    if keys.pressed(KeyCode::KeyS) { stick.y -= 1.0; }
    if keys.pressed(KeyCode::KeyD) { stick.x += 1.0; }
    if keys.pressed(KeyCode::KeyA) { stick.x -= 1.0; }
    if stick.length() > 1.0 { stick = stick.normalize(); }
    mag = mag.max(stick.length());
    for g in &gamepads {
        let raw = g.left_stick();
        let m = jk::stick_magnitude(raw.x, raw.y);
        if m > mag {
            mag = m;
            stick = Vec2::new(jk::normalize_stick(raw.x), jk::normalize_stick(raw.y));
        }
        run |= g.pressed(GamepadButton::LeftThumb);
        crouch_toggle |= g.just_pressed(GamepadButton::East);
        aim |= g.pressed(GamepadButton::LeftTrigger2);
    }
    if !p.alive() {
        stick = Vec2::ZERO;
        mag = 0.0;
        aim = false;
    }
    if crouch_toggle && p.alive() {
        p.crouch = !p.crouch;
    }
    p.aiming = aim;
    p.run = run && !p.crouch && !aim && stick.y > 0.1;

    // GG_Exec_Joy: below 0.25 the stick reports "no movement" (speed reset); above it the direction is
    // the stick direction in the camera frame and joy-norm = (|v|-0.25)/0.675 [C]. The speed table
    // (walk-ready 2.5 / run 4.5 / crouch 1.5, aim 0.5, 0.1 s ready ramp, 4*dt smoothing) is MoveState.
    let fwd = Vec3::new(-p.yaw.sin(), 0.0, -p.yaw.cos());
    let right = Vec3::new(fwd.z * -1.0, 0.0, fwd.x);
    let world = fwd * stick.y + right * stick.x;
    let dir = if mag < jk::MOVE_STICK_MIN || world.length_squared() < 1e-8 {
        None
    } else {
        let d = world.normalize();
        let joy_norm = ((mag - jk::MOVE_STICK_MIN) / jk::MOVE_STICK_SPAN).clamp(0.0, 1.0);
        Some(jk::MoveDir { dir: [d.x, d.z], joy_norm })
    };
    let (crouch, aiming, running) = (p.crouch, p.aiming, p.run);
    let mut mv = p.mv;
    mv.input.stick_mag = mag;
    mv.input.dir = dir;
    mv.input.run = running;
    mv.input.crouch = crouch;
    mv.input.aim = aiming;
    let out = mv.step(dt);
    p.mv = mv;
    p.vel = Vec3::new(out.velocity[0], 0.0, out.velocity[1]);
    let speed = out.speed;
    p.moving = (speed / jk::WALK_SPEED).min(1.5);
    let before = p.bob_phase;
    p.bob_phase += dt * speed * 2.6;
    // one footstep per half bob cycle
    if speed > 0.6 && (before / std::f32::consts::PI).floor() != (p.bob_phase / std::f32::consts::PI).floor() {
        steps.write(crate::events::JackEvent::Footstep);
    }
    let old = tf.translation;
    let pos = arena.move_to(old, old + p.vel * dt, 0.35);
    tf.translation = pos;
}

fn wounds(
    time: Res<Time>,
    mut q: Query<&mut Player>,
    mut ev: EventWriter<crate::events::JackEvent>,
    mut last: Local<Option<WoundState>>,
) {
    let Ok(mut p) = q.single_mut() else { return };
    let dt = time.delta_secs();
    // status transitions -> sound events (wounded again from recovering counts too)
    let now = p.wounds.state;
    if *last != Some(now) {
        match now {
            WoundState::Wounded if last.is_some() => { ev.write(crate::events::JackEvent::Wounded); }
            WoundState::Dead => { ev.write(crate::events::JackEvent::Died); }
            _ => {}
        }
        *last = Some(now);
    }
    // cooldown / wound timer (-> recovering; Jack never heals without Ann) / death timer
    p.wounds.tick(dt);
}

/// Keep the rig's B_Jaf_Camera bone at the camera origin, looking down -Z.
/// The arms glb is authored with the camera bone at Jade (0, 0.155, 1.279) during clips;
/// using the live bone keeps the arms framed for every clip.
fn anchor_viewmodel(
    mut arms: Query<(&ArmsRig, &GlobalTransform, &mut Transform), With<ArmsScene>>,
    bones: Query<&GlobalTransform>,
) {
    for (rig, root_gt, mut tf) in &mut arms {
        let Ok(bone) = bones.get(rig.cam_bone) else { continue };
        let rel = root_gt.affine().inverse() * bone.affine();
        let p = Vec3::from(rel.translation);
        let rot = Quat::from_rotation_y(PI);
        tf.rotation = rot;
        tf.translation = -(rot * p);
    }
}

fn viewmodel_offsets(
    time: Res<Time>,
    q: Query<&Player>,
    arsenal: Res<crate::weapons::Arsenal>,
    mut vm: Query<&mut Transform, With<ViewModel>>,
) {
    let Ok(p) = q.single() else { return };
    let Ok(mut tf) = vm.single_mut() else { return };
    let t = time.elapsed_secs();
    // weapon bob [G]: small figure-eight while moving
    let bob = Vec3::new(
        (p.bob_phase * 0.5).sin() * 0.008,
        -(p.bob_phase).sin().abs() * 0.01,
        0.0,
    ) * p.moving;
    let breathe = Vec3::new(0.0, (t * 1.3).sin() * 0.002, 0.0);
    // swap dip
    let dip = Vec3::new(0.0, -0.35 * arsenal.swap_dip(), 0.0);
    let aim_scale = if p.aiming { 0.25 } else { 1.0 };
    let target = (bob + breathe) * aim_scale + dip;
    tf.translation = tf.translation.lerp(target, (12.0 * time.delta_secs()).min(1.0));
    // procedural recoil: snap back toward the camera and tip the muzzle up [G]
    let r = arsenal.recoil * arsenal.recoil;
    tf.translation += Vec3::new(0.0, 0.004, 0.035) * r;
    tf.rotation = Quat::from_rotation_x(0.06 * r);
}
