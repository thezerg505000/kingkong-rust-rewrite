//! HUD: crosshair, ammo, Jack's wound state, V-Rex health and debug readouts; respawn.

use crate::anim::{GameState, RigPlayer, Rigs};
use crate::player::{ArmsScene, Player};
use kk_mechanics::wounds::{WoundState, Wounds};
use crate::rex::{Rex, RexScene, RexSettings, RexState};
use crate::spec::*;
use crate::weapons::{ArmsAction, Arsenal};
use bevy::prelude::*;

#[derive(Component)]
struct HudText;
#[derive(Component)]
struct DebugText;
#[derive(Component)]
struct RexBar;
#[derive(Component)]
struct Overlay;
#[derive(Component)]
struct CenterText;
/// Full-screen overlays that are part of the picture, not the HUD (kept when HUD is hidden).
#[derive(Component)]
struct Overlay3d;
#[derive(Component)]
struct ScopeOverlay;

pub struct HudPlugin;

/// F8: put the whole slice back (Jack, V-Rex, raptors/compies, Kong fight, destructible wall, spears,
/// bone piles). Nothing respawns by itself.
#[derive(Message, Clone, Copy, Debug, Default)]
pub struct RespawnAll;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<RespawnAll>()
            .add_systems(OnEnter(GameState::Playing), spawn_hud)
            .add_systems(
                Update,
                (respawn_key, update_hud, respawn, clean_hud, scope_view).chain().after(crate::rex::RexSet).run_if(in_state(GameState::Playing)),
            );
    }
}

fn respawn_key(keys: Res<ButtonInput<KeyCode>>, gamepads: Query<&Gamepad>, mut out: MessageWriter<RespawnAll>) {
    // (pad Select is the Jack <-> Kong switch: respawn-all is D-pad up)
    if keys.just_pressed(KeyCode::F8) || gamepads.iter().any(|g| g.just_pressed(GamepadButton::DPadUp)) {
        info!("F8: respawning everything");
        out.write(RespawnAll);
    }
}

fn radial_image(n: u32, f: impl Fn(f32) -> [u8; 4]) -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let mut d = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let u = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            d.extend_from_slice(&f((u * u + v * v).sqrt()));
        }
    }
    Image::new(Extent3d { width: n, height: n, depth_or_array_layers: 1 }, TextureDimension::D2, d, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default())
}

fn spawn_overlays(commands: &mut Commands, images: &mut Assets<Image>) {
    // subtle vignette [G]
    // edge darkening like the game's BorderBrightness after-effect (a = 1 - r^2*k) [L shape, G strength]
    let vig = images.add(radial_image(256, |r| [0, 0, 0, ((r * r * 0.22).min(0.25) * 255.0) as u8]));
    commands.spawn((
        Overlay3d,
        // stretched over the whole window (Bevy 0.17+ keeps the image's aspect ratio by default)
        ImageNode::new(vig).with_mode(bevy::ui::widget::NodeImageMode::Stretch),
        Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
        GlobalZIndex(-2),
    ));
    // scope: black mask with a clear circle + thin crosshair, shown at full sniper zoom [G]
    let mask = images.add(radial_image(512, |r| {
        let edge = ((r - 0.86) / 0.04).clamp(0.0, 1.0);
        [0, 0, 0, (edge * 255.0) as u8]
    }));
    commands
        .spawn((
            Overlay3d,
            ScopeOverlay,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::NONE),
            Visibility::Hidden,
        ))
        .with_children(|p| {
            // side bars + square mask
            // the reticle is part of the scope picture (Overlay3d), so clean-HUD mode keeps it;
            // thin cross plus heavy outer posts, the WW2 sniper-scope layout [G]
            p.spawn((Overlay3d, Node { width: Val::Vh(100.0), height: Val::Vh(100.0), ..default() }, ImageNode::new(mask)))
                .with_children(|m| {
                    let ink = Color::srgba(0.0, 0.0, 0.0, 0.92);
                    let line = |m: &mut ChildSpawnerCommands, n: Node| {
                        m.spawn((Overlay3d, n, BackgroundColor(ink)));
                    };
                    let abs = Node { position_type: PositionType::Absolute, ..default() };
                    // thin cross
                    line(m, Node { left: Val::Percent(8.0), right: Val::Percent(8.0), top: Val::Percent(49.9), height: Val::Px(1.5), ..abs.clone() });
                    line(m, Node { top: Val::Percent(8.0), bottom: Val::Percent(50.0), left: Val::Percent(49.9), width: Val::Px(1.5), ..abs.clone() });
                    // heavy posts: left, right, bottom
                    line(m, Node { left: Val::Percent(6.0), width: Val::Percent(30.0), top: Val::Percent(49.6), height: Val::Percent(0.8), ..abs.clone() });
                    line(m, Node { right: Val::Percent(6.0), width: Val::Percent(30.0), top: Val::Percent(49.6), height: Val::Percent(0.8), ..abs.clone() });
                    line(m, Node { bottom: Val::Percent(6.0), height: Val::Percent(44.0), left: Val::Percent(49.6), width: Val::Percent(0.8), ..abs.clone() });
                });
            p.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), bottom: Val::Px(0.0), width: Val::Percent(21.9), ..default() }, BackgroundColor(Color::BLACK)));
            p.spawn((Node { position_type: PositionType::Absolute, right: Val::Px(0.0), top: Val::Px(0.0), bottom: Val::Px(0.0), width: Val::Percent(21.9), ..default() }, BackgroundColor(Color::BLACK)));
        });
}

fn spawn_hud(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    spawn_overlays(&mut commands, &mut images);
    let font = |s: f32| TextFont { font_size: FontSize::Px(s), ..default() };
    // crosshair
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            top: Val::Percent(50.0),
            margin: UiRect { left: Val::Px(-7.0), top: Val::Px(-13.0), ..default() },
            ..default()
        },
        Text::new("+"),
        font(22.0),
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.8)),
    ));
    commands.spawn((
        HudText,
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(24.0),
            bottom: Val::Px(20.0),
            ..default()
        },
        Text::new(""),
        font(26.0),
        TextColor(Color::srgb(0.95, 0.92, 0.85)),
        TextLayout::justify(Justify::Right),
    ));
    commands.spawn((
        DebugText,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(14.0),
            top: Val::Px(10.0),
            ..default()
        },
        Text::new(""),
        font(14.0),
        TextColor(Color::srgba(0.9, 0.95, 0.9, 0.85)),
    ));
    // V-Rex health bar
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(30.0),
                width: Val::Percent(40.0),
                top: Val::Px(14.0),
                height: Val::Px(10.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        ))
        .with_children(|p| {
            p.spawn((
                RexBar,
                Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                BackgroundColor(Color::srgb(0.7, 0.12, 0.08)),
            ));
        });
    // wound / death overlay
    commands.spawn((
        Overlay,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::NONE),
        GlobalZIndex(-1),
    ));
    commands.spawn((
        CenterText,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(30.0),
            width: Val::Percent(40.0),
            top: Val::Percent(38.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        Text::new("Click to capture the mouse"),
        font(30.0),
        TextColor(Color::srgb(1.0, 0.95, 0.9)),
        TextLayout::justify(Justify::Center),
    ));
}

#[allow(clippy::too_many_arguments)]
fn update_hud(
    arsenal: Res<Arsenal>,
    rigs: Res<Rigs>,
    settings: Res<RexSettings>,
    players: Query<(&Player, &Transform)>,
    rex: Query<(&Rex, &Transform)>,
    arms: Query<&RigPlayer, With<ArmsScene>>,
    rex_rp: Query<&RigPlayer, With<RexScene>>,
    windows: Query<&bevy::window::CursorOptions>,
    mut hud: Query<&mut Text, (With<HudText>, Without<DebugText>, Without<CenterText>)>,
    mut dbg: Query<&mut Text, (With<DebugText>, Without<HudText>, Without<CenterText>)>,
    mut center: Query<&mut Text, (With<CenterText>, Without<HudText>, Without<DebugText>)>,
    mut bar: Query<&mut Node, With<RexBar>>,
    mut overlay: Query<&mut BackgroundColor, With<Overlay>>,
) {
    let Ok((p, ptf)) = players.single() else { return };
    let w = arsenal.current();
    let i = arsenal.index;
    let status = match arsenal.action {
        ArmsAction::Reload { .. } => "  RELOADING",
        ArmsAction::Swap { .. } => "  ...",
        _ if arsenal.mag[i] == 0 && arsenal.reserve[i] == 0 => "  EMPTY",
        _ => "",
    };
    if let Ok(mut t) = hud.single_mut() {
        t.0 = format!(
            "{}\n{} / {}{}",
            w.name, arsenal.mag[i], arsenal.reserve[i], status
        );
    }
    let (rex_line, frac) = match rex.single() {
        Ok((r, rtf)) => (
            format!(
                "V-Rex: {}  hp {:.0}/{:.0}{}  dist {:.1} m  gait {}",
                r.label(),
                r.hp,
                r.max_hp,
                if settings.mortal { " (mortal F2)" } else { "" },
                rtf.translation.distance(ptf.translation),
                r.gait
            ),
            r.hp / r.max_hp,
        ),
        Err(_) => (String::new(), 0.0),
    };
    if let Ok(mut n) = bar.single_mut() {
        n.width = Val::Percent(frac.clamp(0.0, 1.0) * 100.0);
    }
    let arms_clip = arms.single().map(|r| r.current.clone()).unwrap_or_default();
    let rex_clip = rex_rp.single().map(|r| r.current.clone()).unwrap_or_default();
    if let Ok(mut t) = dbg.single_mut() {
        t.0 = format!(
            "Jack: {:?}{}{}  speed {:.2} m/s\n{}\narms clip: {}{}\nrex clip: {}\nshots {}  hits {}{}\n\
             WASD move  Shift run  C crouch  RMB aim  LMB fire  R reload  1-4/wheel weapons\n\
             F1 arm clips  F2 mortal V-Rex  F3 hit spheres  F5 hide HUD  F8 respawn all  E pick up  G drop spear  Esc free mouse  Enter get up",
            p.wounds.state,
            if p.crouch { " crouched" } else { "" },
            if p.aiming { " aiming" } else { "" },
            p.vel.length(),
            rex_line,
            arms_clip,
            if arsenal.debug_clip.is_some() { "  [F1 debug]" } else { "" },
            rex_clip,
            arsenal.shots_fired,
            arsenal.hits,
            arsenal
                .last_hit
                .map(|(b, d)| format!("  last hit {b} -{d:.0}"))
                .unwrap_or_default(),
        );
    }
    let grabbed = windows
        .single()
        .map(|w| w.grab_mode != bevy::window::CursorGrabMode::None)
        .unwrap_or(false);
    if let Ok(mut t) = center.single_mut() {
        t.0 = if !p.alive() {
            format!("Jack was {}.\n\nPress Enter to try again", p.death_cause)
        } else if let Ok((r, _)) = rex.single() {
            if matches!(r.state, RexState::Dead { .. }) {
                "The V-Rex is down.\n\nPress Enter to restart".to_string()
            } else if !grabbed {
                "Click to capture the mouse".to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };
    }
    if let Ok(mut bg) = overlay.single_mut() {
        // Injury vignette driven by the wound status (H01..H04): wounded pulses and fades with the hurt
        // timer (`this[0xb8f]`, length of the wound), recovering stays faintly tinted until Ann heals
        // (Jack never heals alone), dead fades in over the 3 s death sequence [alpha levels G].
        let a = match p.wounds.state {
            WoundState::Healthy => 0.0,
            WoundState::Wounded => {
                let left = (p.wounds.hurt_timer / p.wounds.wound_len.max(0.1)).clamp(0.0, 1.0);
                0.18 + 0.06 * (p.wounds.since_wound * 6.0).sin().abs() * left
            }
            WoundState::Recovering => 0.10,
            WoundState::Dead => (p.wounds.dead_time / DEATH_SEQUENCE_S).min(1.0) * 0.75,
        };
        bg.0 = Color::srgba(0.45, 0.0, 0.0, a);
    }
    let _ = rigs;
}

fn respawn(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut all: MessageReader<RespawnAll>,
    mut players: Query<(&mut Player, &mut Transform), Without<Rex>>,
    mut rex: Query<(&mut Rex, &mut Transform), Without<Player>>,
    settings: Res<RexSettings>,
    mut arsenal: ResMut<Arsenal>,
    arena: Res<crate::world::Arena>,
) {
    let everything = all.read().count() > 0;
    let Ok((mut p, mut ptf)) = players.single_mut() else { return };
    let pressed = keys.just_pressed(KeyCode::Enter)
        || gamepads.iter().any(|g| g.just_pressed(GamepadButton::Start));
    // H_ETAT_IA_mort: the checkpoint restart is requested once Jack has been in the death state for 4.0 s
    // (hard timeout 8.0 s); Enter / Start restarts earlier [C]. Only Jack gets up: the creatures stay as
    // they are until F8.
    let auto = p.wounds.restart_due();
    let jack_up = !p.alive() && ((pressed && p.wounds.dead_time > DEATH_SEQUENCE_S * 0.5) || auto);
    if !(everything || jack_up) {
        return;
    }
    // Stats_OnPlayerDeath counts the death (the wound timers shrink with it); the counter survives the respawn
    let mut w: Wounds = p.wounds;
    if !p.alive() {
        w.commit_death();
    }
    w.respawn();
    *p = Player::new();
    p.wounds = w;
    p.yaw = arena.player_yaw;
    ptf.translation = arena.settle(arena.player_spawn, 0.35);
    arsenal.refill();
    if !everything {
        return;
    }
    if let Ok((mut r, mut rtf)) = rex.single_mut() {
        *r = Rex::new(if settings.mortal { REX_HP_MORTAL } else { REX_HP });
        r.yaw = arena.rex_yaw;
        rtf.translation = arena.rex_spawn;
        rtf.rotation = Quat::from_rotation_y(arena.rex_yaw);
    }
}

/// F5 / test batches: hide every HUD element (the original game shows no HUD during play).
fn clean_hud(
    keys: Res<ButtonInput<KeyCode>>,
    mut clean: ResMut<crate::batch::CleanHud>,
    mut q: Query<&mut Visibility, (With<Node>, Without<Overlay3d>)>,
    overlay_children: Query<&Children, With<Overlay3d>>,
    mut scope: Query<&mut Visibility, (With<ScopeOverlay>, Without<crate::player::ViewModel>, Without<Node>)>,
) {
    let _ = (&overlay_children, &mut scope);
    if keys.just_pressed(KeyCode::F5) {
        clean.0 = !clean.0;
    }
    let v = if clean.0 { Visibility::Hidden } else { Visibility::Inherited };
    for mut vis in &mut q {
        if *vis != v {
            *vis = v;
        }
    }
}

/// Sniper: at (near) full zoom hide the viewmodel and show the scope picture.
fn scope_view(
    players: Query<&Player>,
    arsenal: Res<Arsenal>,
    cam: Query<&Projection, With<crate::player::MainCam>>,
    mut scope: Query<(&mut Visibility, &Children), With<ScopeOverlay>>,
    mut vis_all: Query<&mut Visibility, Without<ScopeOverlay>>,
    mut vm: Query<Entity, With<crate::player::ViewModel>>,
) {
    let Ok(p) = players.single() else { return };
    let zoomed = match cam.single() {
        Ok(Projection::Perspective(pp)) => {
            let h = 2.0 * ((pp.fov * 0.5).tan() / 0.75).atan();
            arsenal.current().id == WeaponId::SniperRifle && p.aiming && !arsenal.spear_held && h < FOV_SNIPER_AIM + 0.12
        }
        _ => false,
    };
    if let Ok((mut v, children)) = scope.single_mut() {
        let want = if zoomed { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
        for c in children.iter() {
            if let Ok(mut cv) = vis_all.get_mut(c) {
                *cv = Visibility::Inherited;
            }
        }
    }
    if let Ok(e) = vm.single_mut() {
        if let Ok(mut v) = vis_all.get_mut(e) {
            let want = if zoomed { Visibility::Hidden } else { Visibility::Inherited };
            if *v != want {
                *v = want;
            }
        }
    }
}
