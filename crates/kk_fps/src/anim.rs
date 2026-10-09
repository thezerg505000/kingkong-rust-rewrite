//! glTF loading and named-clip animation graphs for the recovered Jade rigs.

use bevy::prelude::*;
use std::collections::HashMap;
use std::time::Duration;

pub const ARMS_GLB: &str = "jack_fps_arms.glb";
/// Root-motion-stripped copy of trex.glb (JadeActor travel removed, pelvis made in-place).
pub const REX_GLB: &str = "trex_inplace.glb";
/// The Kong-level V-Rex action kit (07D J_PNJ_KTREX_2, clips `kt_0xNN` by KT anim id) on the same
/// skeleton, exported by kk_extract (tools2/export_rex_kt.py). Optional: merged into the rex rig.
pub const REX_KT_GLB: &str = "trex_kt.glb";
/// Level 03E rebuilt from the key map: every placed instance with its real Jade materials.
pub const LEVEL_GLB: &str = "level03e/level03e_v2.glb";
pub const LEVEL_COLLISION: &str = "level03e/level03e_v2_collision.json";

/// A loaded glTF with an animation graph built from all of its named clips.
#[derive(Default)]
pub struct Rig {
    pub gltf: Handle<Gltf>,
    pub graph: Handle<AnimationGraph>,
    /// full clip name -> graph node
    pub nodes: HashMap<String, AnimationNodeIndex>,
    /// full clip name -> duration (s)
    pub durations: HashMap<String, f32>,
    /// sorted list of clip names (debug cycling)
    pub names: Vec<String>,
}

impl Rig {
    /// Resolve a clip by exact name, or by label prefix ("idle_c07" -> "idle_c07__arms_009").
    pub fn find(&self, label: &str) -> Option<&str> {
        if let Some((k, _)) = self.nodes.get_key_value(label) {
            return Some(k.as_str());
        }
        let prefix = format!("{label}__");
        self.names
            .iter()
            .find(|n| n.starts_with(&prefix))
            .map(|s| s.as_str())
    }
    pub fn duration(&self, label: &str) -> f32 {
        self.find(label)
            .and_then(|n| self.durations.get(n))
            .copied()
            .unwrap_or(1.0)
    }
}

#[derive(Resource, Default)]
pub struct Rigs {
    pub arms: Rig,
    pub rex: Rig,
    /// trex_kt.glb when present (KT clips merged into `rex`)
    pub rex_kt: Option<Handle<Gltf>>,
    pub arms_scene: Handle<Scene>,
    pub rex_scene: Handle<Scene>,
    pub weapon_scenes: Vec<Handle<Scene>>,
    pub weapon_gltfs: Vec<Handle<Gltf>>,
    /// original level 03E, when exported (research/pc/game_assets/level03e/level03e.glb)
    pub level: Option<Handle<Gltf>>,
    /// King Kong's rig (kong/kong.glb), loaded only when `kong::kong_enabled()`
    pub kong_gltf: Option<Handle<Gltf>>,
    pub ready: bool,
}

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    #[default]
    Loading,
    Playing,
}

pub struct AnimPlugin;

impl Plugin for AnimPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>()
            .init_resource::<Rigs>()
            .add_systems(Startup, start_loading)
            .add_systems(Update, finish_loading.run_if(in_state(GameState::Loading)));
    }
}

fn start_loading(mut rigs: ResMut<Rigs>, assets: Res<AssetServer>) {
    rigs.arms.gltf = assets.load(ARMS_GLB);
    rigs.rex.gltf = assets.load(REX_GLB);
    if crate::asset_dir().join(REX_KT_GLB).exists() {
        rigs.rex_kt = Some(assets.load(REX_KT_GLB));
    }
    let lvl = crate::asset_dir().join(crate::scene::level_glb());
    if lvl.exists() && std::env::var("KK_STAND_IN").is_err() {
        rigs.level = Some(assets.load(crate::scene::level_glb()));
    }
    if crate::kong::kong_enabled() {
        rigs.kong_gltf = Some(assets.load(crate::kong::KONG_GLB));
    }
    rigs.weapon_gltfs = crate::spec::WEAPONS
        .iter()
        .map(|w| assets.load(w.glb))
        .collect();
}

fn build_rig(rig: &mut Rig, gltf: &Gltf, clips: &Assets<AnimationClip>, graphs: &mut Assets<AnimationGraph>) {
    build_rig_multi(rig, &[gltf], clips, graphs);
}

/// One graph from the named clips of several glTFs sharing a skeleton (same node names, so the
/// animation targets match); the first file wins on a name clash.
fn build_rig_multi(rig: &mut Rig, gltfs: &[&Gltf], clips: &Assets<AnimationClip>, graphs: &mut Assets<AnimationGraph>) {
    let mut by_name: HashMap<String, Handle<AnimationClip>> = HashMap::new();
    for g in gltfs {
        for (k, h) in g.named_animations.iter() {
            by_name.entry(k.to_string()).or_insert_with(|| h.clone());
        }
    }
    let mut names: Vec<String> = by_name.keys().cloned().collect();
    names.sort();
    let handles: Vec<Handle<AnimationClip>> = names.iter().map(|n| by_name[n].clone()).collect();
    let (graph, idx) = AnimationGraph::from_clips(handles.iter().cloned());
    rig.graph = graphs.add(graph);
    for ((n, i), h) in names.iter().zip(idx).zip(handles.iter()) {
        rig.nodes.insert(n.clone(), i);
        let d = clips.get(h).map(|c| c.duration()).unwrap_or(1.0);
        rig.durations.insert(n.clone(), d);
    }
    rig.names = names;
}

fn finish_loading(
    mut rigs: ResMut<Rigs>,
    gltfs: Res<Assets<Gltf>>,
    clips: Res<Assets<AnimationClip>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    assets: Res<AssetServer>,
    mut next: ResMut<NextState<GameState>>,
    mut warned: Local<f32>,
    time: Res<Time>,
) {
    let all = std::iter::once(rigs.arms.gltf.id())
        .chain(std::iter::once(rigs.rex.gltf.id()))
        .chain(rigs.rex_kt.iter().map(|h| h.id()))
        .chain(rigs.weapon_gltfs.iter().map(|h| h.id()))
        .chain(rigs.level.iter().map(|h| h.id()))
        .chain(rigs.kong_gltf.iter().map(|h| h.id()))
        .collect::<Vec<_>>();
    if !all.iter().all(|id| assets.is_loaded_with_dependencies(*id)) {
        *warned += time.delta_secs();
        if *warned > 10.0 {
            *warned = -1000.0;
            warn!(
                "assets still loading after 10 s — check that the asset folder contains {ARMS_GLB}, {REX_GLB} and the weapon glbs (set KK_ASSETS to override)"
            );
        }
        return;
    }
    let rigs = &mut *rigs;
    let arms = gltfs.get(&rigs.arms.gltf).expect("arms gltf");
    build_rig(&mut rigs.arms, arms, &clips, &mut graphs);
    let rex = gltfs.get(&rigs.rex.gltf).expect("rex gltf");
    match rigs.rex_kt.as_ref().and_then(|h| gltfs.get(h)) {
        Some(kt) => build_rig_multi(&mut rigs.rex, &[rex, kt], &clips, &mut graphs),
        None => build_rig(&mut rigs.rex, rex, &clips, &mut graphs),
    }
    rigs.arms_scene = arms.scenes[0].clone();
    rigs.rex_scene = rex.scenes[0].clone();
    rigs.weapon_scenes = rigs
        .weapon_gltfs
        .iter()
        .map(|h| gltfs.get(h).expect("weapon gltf").scenes[0].clone())
        .collect();
    info!(
        "rigs ready: arms {} clips, rex {} clips",
        rigs.arms.names.len(),
        rigs.rex.names.len()
    );
    rigs.ready = true;
    next.set(GameState::Playing);
}

/// Animation player + transitions handle attached to a spawned rig.
#[derive(Component)]
pub struct RigPlayer {
    pub player: Entity,
    pub current: String,
}

/// Find the descendant entity carrying the AnimationPlayer, attach the graph, and return it.
pub fn attach_graph(
    commands: &mut Commands,
    root: Entity,
    children: &Query<&Children>,
    players: &Query<Entity, With<AnimationPlayer>>,
    graph: &Handle<AnimationGraph>,
) -> Option<Entity> {
    let e = std::iter::once(root)
        .chain(children.iter_descendants(root))
        .find(|e| players.contains(*e))?;
    commands
        .entity(e)
        .insert((AnimationGraphHandle(graph.clone()), AnimationTransitions::new()));
    Some(e)
}

/// Cross-fade to a clip. `repeat` loops it; `restart` replays it even if already current.
pub fn play(
    rig: &Rig,
    rp: &mut RigPlayer,
    player: &mut AnimationPlayer,
    transitions: &mut AnimationTransitions,
    label: &str,
    fade_s: f32,
    repeat: bool,
    restart: bool,
) -> bool {
    let Some(name) = rig.find(label) else {
        return false;
    };
    if !restart && rp.current == name {
        return true;
    }
    let node = rig.nodes[name];
    let active = transitions.play(player, node, Duration::from_secs_f32(fade_s));
    if repeat {
        active.repeat();
    }
    if restart {
        active.replay();
    }
    rp.current = name.to_string();
    true
}
