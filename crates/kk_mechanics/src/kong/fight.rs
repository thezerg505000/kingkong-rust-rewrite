//! Two-body Kong-versus-V-Rex fight simulation (ledger B02, KC01-KC18, KF01-KF07).
//!
//! `Fight` glues the recovered pieces together: Kong's combat-phase machine and input latches
//! ([`super::combat`]), fury ([`super::fury`]) and the KT rex state machine, hit rules and the jaw-break
//! mash ([`super::vrex`]). It is deterministic (fixed 60 Hz internal tick, one seeded xorshift
//! for the rex's attack spacing) and engine-free: the presentation layer feeds a [`KongInput`]
//! (the pad abstraction a player would produce) and renders the [`FightEvent`]s that
//! [`Fight::step`] returns. The AI that produces a `KongInput` is [`super::ai::KongBrain`].
//!
//! What is recovered and what is ours (see `spec/evidence/B02.md`, "Fight simulation"):
//! * `[C]` the phase graph (`combat::next_phase`), blow table (`combat::blow`), invulnerable anims,
//!   dodge gates, fury start/extend/penalty/decay, pound window, rex states and transitions, rex
//!   hit rule (`hit_damage`), counters, KO timings, 20.0 impact, finisher proposal + mash, rex
//!   attack packets, ranges and cool-down-less facts of `KT_ETAT_fight_KONG`.
//! * `[G]` everything physical: metres, speeds, body radii, arena, animation lengths and hit
//!   windows (the animation bank is not in the exe), the rex's attack durations/cool-downs, the
//!   rex's and Kong's life maxima, which rex states offer a grab, grab move durations.
//!   Each `[G]` constant below says so.

use super::anims;
use super::combat::{self, BlowCtx, ComboCtx, Latched, Next, Phase, SpecialTarget};
use super::fury::{self, Fury, PoundResult, PoundWindow};
use super::movement;
use super::vrex::{
    self, AttackKind, FinishMash, FinishOutcome, GrabMsg, HitCounters, KtInput, KtLife, KtMachine, KtState, Target,
};

pub type V2 = (f32, f32);

// ---------------------------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------------------------

/// Internal fixed tick [G]: animation frame counters are taken as 60 Hz (KC01 gap).
pub const TICK: f32 = 1.0 / 60.0;
pub const FPS: f32 = 60.0;
/// Arena (circle, metres) [G].
pub const ARENA_RADIUS: f32 = 32.0;
/// Body radii [G]; Kong's sweep reach 3.0 is [C] (`HIT_SWEEP_REACH`).
pub const REX_RADIUS: f32 = 4.0;
pub const KONG_RADIUS: f32 = 1.0;
/// Kong full-speed run [G] (locomotion is animation driven, K01).
pub const KONG_RUN_SPEED: f32 = 8.0;
/// Rex walk speed / charge speed [G]. The KT clips' own root speeds (clip root track x kit speed byte
/// b2/64) are walk 0x01 4.72 m/s and charge 0x05 18.6 m/s [C data]; whether KT locomotion follows the
/// root track or a Dyn speed set by `KT_ETAT_charge` is not settled, and the demo brain is tuned on
/// these values, so the presentation scales the clips to the simulated speed instead.
pub const REX_WALK_SPEED: f32 = 4.0;
pub const REX_CHARGE_SPEED: f32 = 14.0;
/// Maximum charge length before the rex stops by itself [G].
pub const REX_CHARGE_MAX_LEN: f32 = 28.0;
/// Rex life of the Kong-level `PNJ_KTREX` model default: f_life_max / f_life_init / f_life_seuil_blesse
/// = 80 / 80 / 50 [C] (`models/PNJ_KTREX.json` offsets 0xf4/0xf8/0xfc, instances/tunables.json; 03E-less levels:
/// 07D `J_PNJ_KTREX`, 10B `PNJ_KTREX` use the default). The wound threshold is a life value, so the ratio
/// used by `fn@0x00770b70` (`cur/max <= ratio`) is 50/80 [L].
pub const REX_MAX_LIFE: f32 = 80.0;
pub const REX_START_LIFE: f32 = 80.0;
pub const REX_WOUND_LIFE: f32 = 50.0;
pub const REX_WOUND_RATIO: f32 = REX_WOUND_LIFE / REX_MAX_LIFE;
/// Kong life (`models/Kong.json` f_life @0x54) [C]; wound ratio and last-stand rules: the latter are [C]
/// (`fury::life_zero_response`), the ratio is [G].
pub const KONG_MAX_LIFE: f32 = 100.0;
pub const KONG_WOUND_RATIO: f32 = 0.35;
/// Life Kong is restored to when a last stand times out [G] (k_reflex uses the wound ratio [L]).
pub const KONG_RESTORE_RATIO: f32 = 0.3;
/// Reach of Kong's blows measured from the rex's surface: sweep 3.0 [C] + hand reach 1.5 [G];
/// the counter lunge uses the enlarged hand volume 4.0 instead of 3.0 [C].
pub const KONG_HAND_REACH: f32 = 1.5;
/// The blow must be roughly in front of Kong: cos 60 deg [G]; aim assist (cos 45, reach 8) is [C].
pub const KONG_HIT_CONE_COS: f32 = 0.5;
/// Distance (centre to centre) within which the rex offers a grab [G].
pub const GRAB_RANGE: f32 = 8.0;
/// A held rex that Kong does nothing with breaks free after this long: 1.0 s, 5.0 s if wounded [C B02: `Rex+0x...` reply 2].
pub const GRAB_HOLD_TIMEOUT: f32 = 1.0;
pub const GRAB_HOLD_TIMEOUT_WOUNDED: f32 = 5.0;
/// Throw release frame 60 and slam release frame 78 (`fn@0x00889e10` callers, kinds 0x17/0x18) [C].
pub const THROW_RELEASE_FRAME: f32 = 60.0;
pub const SLAM_RELEASE_FRAME: f32 = 78.0;
/// Throw flight: 20 m at 25 m/s [G]. Slam damage [G] (KC12: slam damage not recovered; uses the impact value).
pub const THROW_DISTANCE: f32 = 20.0;
pub const THROW_SPEED: f32 = 25.0;
pub const SLAM_DAMAGE: f32 = vrex::IMPACT_DAMAGE;
/// Jaw-break lock clip length in frames: `fn@0x00425e60` of rex anim 0x37 = 346 (its TRL in the
/// J_PNJ_KTREX_2 kit; Kong's 0xe7 has the same 346 frames) [C].
pub const FINISH_ANIM_LEN: f32 = 346.0;

// ---- animation ids ---------------------------------------------------------------------------
/// Recovered Kong ids live in `combat` (`ANIM_*`). Ids below are `[G]` placeholders because the
/// code only references them through tables not decoded here; the presentation maps them.
pub const ANIM_WALK: u32 = 0x01;
pub const ANIM_RUN: u32 = 0x02;
pub const ANIM_GRAB_START: u32 = 0xa0;
pub const ANIM_THROW: u32 = 0xa4;
pub const ANIM_SLAM: u32 = 0xa5;
/// Kong's jaw-break animations against the rex, `k_ETAT_finish@0x8e3ca0` (Kong state 0x352) [C]: lock 0xe6 (20 frames), mash 0xe7 (346 frames,
/// replayed every frame by `ANIM_Play` so it restarts when it ends), win 0xe8 (214 frames; the kill is sent at frame 100 = `local_120`),
/// shake-off 0x6b (47 + 39 frames, two items). The bat branch of the same state uses 0x136-0x139 (`fn@0x0043f8e0(target, KBatsCharognards)`).
pub const ANIM_FINISH_LOCK: u32 = 0xe6;
pub const ANIM_FINISH_MASH: u32 = 0xe7;
pub const ANIM_FINISH_WIN: u32 = 0xe8;
pub const ANIM_FINISH_ESCAPE: u32 = 0x6b;
/// `Kong_MashPullValue@0x602e40` in state 0x352 on 0xe7: `max(0, (frame - 102) / (length - 102))` [C]; 0xe6 -> 0, 0xe8 -> 1.
pub const FINISH_PULL_RAMP_START: f32 = 102.0;
/// Frame of 0xe8 at which Kong reports the kill (message 0x17 + `fn@0x00883540`) [C].
pub const FINISH_KILL_FRAME: f32 = 100.0;

/// `Kong_MashPullValue` as a function of the time since the finisher started [C ramp, see [`ANIM_FINISH_MASH`]].
pub fn finish_pull_value(t: f32) -> f32 {
    let lock_s = anim_info(ANIM_FINISH_LOCK).len / FPS;
    if t < lock_s {
        return 0.0;
    }
    let len = anim_info(ANIM_FINISH_MASH).len;
    let frame = ((t - lock_s) * FPS) % len;
    ((frame - FINISH_PULL_RAMP_START) / (len - FINISH_PULL_RAMP_START)).max(0.0)
}
/// Rex animation ids that the code names [C]: sweep 0xc/0xd, tail 0xe/0xf, bite 4/0xa/0xb, throw 0x7a,
/// slams 0x7b/0x8a/0x8d/0x9e/0xa1, jaw-break 0x37 (lock) / 0x38 (won) / 0x39 (shake-off).
pub const REX_ANIM_SWEEP: u32 = 0xc;
pub const REX_ANIM_TAIL: u32 = 0xe;
pub const REX_ANIM_BITE: u32 = 0x4;
pub const REX_ANIM_THROWN: u32 = 0x7a;
pub const REX_ANIM_SLAMMED: u32 = 0x8d;
pub const REX_ANIM_FINISH: u32 = 0x37;
pub const REX_ANIM_FINISH_WON: u32 = 0x38;
pub const REX_ANIM_FINISH_ESCAPE: u32 = 0x39;

/// Frame of a dodge from which a latched attack press may start the counter lunge [G] (the real cancel flag is animation data).
pub const DODGE_ATTACK_CANCEL_FRAME: f32 = 12.0;

/// Animation timing [G] (the animation bank is not available): length, hit window (frames), the frame
/// from which a latched press may cancel into the next action. Frames 0x12 (grab strike), 0x20 (downward
/// shake), 0x37 (side-step chain) and 0x82 (pound) are [C] and are honoured below.
#[derive(Clone, Copy, Debug)]
pub struct AnimInfo {
    pub len: f32,
    pub hit: Option<(f32, f32)>,
    pub cancel: f32,
}

pub fn anim_info(id: u32) -> AnimInfo {
    // [G] placeholders the code does not name (see the `ANIM_*` notes above): lengths are presentation choices.
    match id {
        ANIM_GRAB_START => return AnimInfo { len: 20.0, hit: None, cancel: 20.0 },
        // Kong's own throw/slam clips are 54 frames (0xa4/0xa5) but the release frames 60/78 are counted on the rex's side [L],
        // so the grab stages keep the longer lengths.
        ANIM_THROW => return AnimInfo { len: 90.0, hit: None, cancel: 90.0 },
        ANIM_SLAM => return AnimInfo { len: 100.0, hit: None, cancel: 100.0 },
        ANIM_WALK | ANIM_RUN => return AnimInfo { len: 1.0e9, hit: None, cancel: 1.0e9 },
        _ => {}
    }
    // Recovered table [C] (KC01): length = item frames at speed byte/64; the hit window opens at the first TRL event that sets
    // object flag 0x1 (`k_exec_hit_window_open@0x887490`) and stays open until the animation changes (`k_callback_after_blend@0x896bf0` clears it at 0x89801a, trigger [L]);
    // the cancel window is the first event that sets flag 0x100 (`fn@0x00884780`), else the end of the clip.
    if let Some(a) = anims::action(id) {
        if a.looping() {
            return AnimInfo { len: 1.0e9, hit: None, cancel: 1.0e9 };
        }
        let len = a.len60();
        let hit = a.hit_start60().map(|s| (s, len));
        let cancel = a.cancel60().unwrap_or(len).min(len);
        return AnimInfo { len, hit, cancel };
    }
    AnimInfo { len: 1.0e9, hit: None, cancel: 1.0e9 } // idle / locomotion loops never end by themselves
}

// ---------------------------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------------------------

/// Button slots, as in `combat::Latched`: slot 1 jump/roll(dodge), 2 special (fury/repel), 3 attack, 4 cancel/mash.
pub const SLOT_JUMP_ROLL: usize = 0;
pub const SLOT_SPECIAL: usize = 1;
pub const SLOT_ATTACK: usize = 2;
pub const SLOT_CANCEL: usize = 3;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Button {
    pub held: bool,
    /// rising edge this step (`Pad_ButtonPressed`)
    pub pressed: bool,
}

/// What a pad produces for Kong. `stick` is the camera-relative ground direction (x, y), magnitude 0..1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KongInput {
    pub stick: V2,
    pub buttons: [Button; 4],
}

impl KongInput {
    pub fn press(&mut self, slot: usize) {
        self.buttons[slot] = Button { held: true, pressed: true };
    }
    pub fn hold(&mut self, slot: usize) {
        self.buttons[slot].held = true;
    }
    pub fn pressed(&self, slot: usize) -> bool {
        self.buttons[slot].pressed
    }
    pub fn held(&self, slot: usize) -> bool {
        self.buttons[slot].held
    }
}

// ---------------------------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Actor {
    Kong,
    Rex,
}

/// The rex's attack moves (`KT_exec_zdf_zdc` modes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RexMove {
    /// mode 5, flags 1, damage 10
    Sweep,
    /// mode 7, flags 2, damage 10
    Tail,
    /// mode 1, flags 2, damage 10 (jaw lunge)
    Bite,
    /// mode 3, flags 0x42, damage 10
    Charge,
}

impl RexMove {
    pub fn packet(self) -> (u32, i32) {
        match self {
            RexMove::Sweep => vrex::ATTACK_SWEEP,
            RexMove::Tail => vrex::ATTACK_TAIL,
            RexMove::Bite => vrex::ATTACK_JAW,
            RexMove::Charge => vrex::ATTACK_CHARGE,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KongMode {
    /// `k_ETAT_main`: idle, locomotion, combat phases
    Main,
    /// `k_ETAT_paf`: hit-stun
    Paf,
    /// `k_ETAT_grab`: holding the rex
    Grab,
    /// `k_ETAT_finish`: jaw-break finisher
    Finisher,
    /// after the rex died
    Victory,
    Dead,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FightEvent {
    /// Animation requested (the game's anim ids); `speed` includes the fury 1.25.
    Anim { actor: Actor, id: u32, speed: f32 },
    /// Kong started a combat phase (`Kong+0x1adc`).
    KongPhase { phase: Phase },
    KongMode { mode: KongMode },
    RexState { from: KtState, to: KtState },
    RexAttack { kind: RexMove },
    /// A blow landed. `damage` is the packet value, `applied` the life actually removed (0 for the
    /// no-damage classes and for plain blows on a wounded rex).
    Hit { attacker: Actor, victim: Actor, class: u32, damage: i32, applied: f32, pos: [f32; 3], anim: u32 },
    /// A rex attack reached Kong while he was invulnerable (anims 6, 7, 0x16).
    HitAvoided { attack: RexMove, by_anim: u32 },
    /// `knockdown_capable`: the blow can knock the rex down (feeds Kong's hit-connected feedback).
    KnockdownFeedback { class: u32 },
    /// Kong flinches: anim and strength from `combat::hit_reaction`.
    KongStunned { anim: u32, strength: u32 },
    CameraShake { amplitude: f32, frequency: f32, duration: f32, decay: f32 },
    PoundStart,
    FuryShout { radius: f32 },
    FuryStart { timer: f32 },
    FuryExtend { timer: f32 },
    FuryEnd,
    RexRoar,
    KoStart { hold: f32 },
    KoEnd,
    GrabStart,
    GrabStrike,
    GrabBrokeFree,
    Throw { dir: V2 },
    ThrowImpact { damage: f32 },
    /// The rex's hit reaction (`fn@0x0055a020`): KT anim, sound slot (3 light / 4 heavy), `+0xae8` weight.
    RexPaf { anim: u32, sound: u32, recoil: f32 },
    /// A blow on the lying rex: ground-hit clip 0x20, it stays down.
    RexGroundHit,
    /// Kong's blow reached its hit frame (plane position / facing of Kong): breakables in reach shatter.
    KongSwing { anim: u32, pos: V2, facing: f32 },
    Slam { damage: f32 },
    FinisherStart,
    FinisherMash { progress: f32, pull: f32, resist: f32 },
    FinisherEscape,
    FinisherSuccess,
    LastStand { seconds: f32 },
    KongRevived { life: f32 },
    RexDied,
    KongDied,
    VictoryPound,
    VictoryRoar,
    FightOver { winner: Actor },
}

// ---------------------------------------------------------------------------------------------
// Small math helpers
// ---------------------------------------------------------------------------------------------

fn sub(a: V2, b: V2) -> V2 {
    (a.0 - b.0, a.1 - b.1)
}
fn len(a: V2) -> f32 {
    (a.0 * a.0 + a.1 * a.1).sqrt()
}
fn norm(a: V2) -> V2 {
    let l = len(a);
    if l < 1e-6 { (1.0, 0.0) } else { (a.0 / l, a.1 / l) }
}
fn dot(a: V2, b: V2) -> f32 {
    a.0 * b.0 + a.1 * b.1
}
fn dir_of(angle: f32) -> V2 {
    (angle.cos(), angle.sin())
}
fn angle_of(d: V2) -> f32 {
    d.1.atan2(d.0)
}
fn add_scaled(a: V2, d: V2, s: f32) -> V2 {
    (a.0 + d.0 * s, a.1 + d.1 * s)
}
fn turn_toward(cur: f32, target: f32, max_step: f32) -> f32 {
    let mut d = (target - cur) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    } else if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    cur + d.clamp(-max_step, max_step)
}
fn clamp_arena(p: V2, radius: f32) -> V2 {
    let l = len(p);
    let lim = ARENA_RADIUS - radius;
    if l > lim { (p.0 / l * lim, p.1 / l * lim) } else { p }
}

/// xorshift32 [G]: only used for the rex's attack spacing.
#[derive(Clone, Debug)]
pub struct Rng(pub u32);
impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng(seed.wrapping_mul(2_654_435_761).max(1))
    }
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    /// uniform in [0,1)
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

// ---------------------------------------------------------------------------------------------
// Bodies
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GrabStage {
    /// grab animation playing (`t` seconds)
    Reach(f32),
    /// holding; `idle` counts toward the break-free timeout
    Holding(f32),
    Strike { t: f32, hit_done: bool },
    Throw { t: f32, released: bool },
    Slam { t: f32, done: bool },
}

#[derive(Clone, Debug)]
pub struct KongState {
    pub pos: V2,
    /// facing angle (radians)
    pub facing: f32,
    pub vel: V2,
    pub life: f32,
    pub max_life: f32,
    pub fury: Fury,
    pub mode: KongMode,
    pub phase: Phase,
    pub anim: u32,
    /// seconds of animation time (already multiplied by the fury speed)
    pub anim_t: f32,
    pub latched: Latched,
    pub pound: PoundWindow,
    /// the single hit of the current phase has been delivered (`Kong+0xd34[]` list, one victim)
    pub hit_done: bool,
    /// the current blow's hit window has opened (`KongSwing` sent), whether or not it reached the rex
    pub swing_done: bool,
    pub shake_done: bool,
    /// time since the phase started (real seconds), drives the forward step
    pub phase_t: f32,
    pub step_speed: f32,
    pub dodge_dir: V2,
    pub dodge_speed: f32,
    pub dodge_left: f32,
    pub dodge_flip: bool,
    pub last_stand_timer: f32,
    pub last_stand_flag: bool,
    pub grab: GrabStage,
    pub finisher: Option<FinisherState>,
    pub victory_t: f32,
    /// victory pound waits for the jaw-break clip 0xe8 to finish (s into the victory)
    pub victory_pound_at: f32,
    /// the victory sequence (pound + roar) is over and Kong is back in `k_ETAT_main` [L: after the
    /// win the game hands Kong back to the player; the fight no longer drives anything]
    pub victory_done: bool,
    pub paf_len: f32,
    pub attacks_started: u32,
}

#[derive(Clone, Debug)]
pub struct FinisherState {
    pub mash: FinishMash,
    pub t: f32,
    /// ticks spent waiting for the rex to enter `Finish`
    pub wait: u32,
    /// seconds since the jaw broke (message 0x16: Kong 0xe8, rex 0x38), None while mashing
    pub won_t: Option<f32>,
}

impl KongState {
    fn new(pos: V2, facing: f32) -> Self {
        KongState {
            pos,
            facing,
            vel: (0.0, 0.0),
            life: KONG_MAX_LIFE,
            max_life: KONG_MAX_LIFE,
            fury: Fury::default(),
            mode: KongMode::Main,
            phase: Phase::None,
            anim: combat::ANIM_IDLE,
            anim_t: 0.0,
            latched: Latched::default(),
            pound: PoundWindow::default(),
            hit_done: false,
            swing_done: false,
            shake_done: false,
            phase_t: 0.0,
            step_speed: 0.0,
            dodge_dir: (0.0, 0.0),
            dodge_speed: 0.0,
            dodge_left: 0.0,
            dodge_flip: false,
            last_stand_timer: 0.0,
            last_stand_flag: false,
            grab: GrabStage::Reach(0.0),
            finisher: None,
            victory_t: 0.0,
            victory_pound_at: 0.0,
            victory_done: false,
            paf_len: 0.0,
            attacks_started: 0,
        }
    }

    pub fn frame(&self) -> f32 {
        self.anim_t * FPS
    }
    pub fn is_wounded(&self) -> bool {
        self.life / self.max_life <= KONG_WOUND_RATIO
    }
    pub fn forward(&self) -> V2 {
        dir_of(self.facing)
    }
    pub fn invulnerable(&self) -> bool {
        combat::is_invulnerable(self.anim, false, false)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RexAttack {
    pub kind: RexMove,
    pub dur: f32,
    pub hit_t: f32,
    pub t: f32,
    pub hit_done: bool,
}

#[derive(Clone, Copy, Debug)]
struct Fly {
    dir: V2,
    left: f32,
}

#[derive(Clone, Debug)]
pub struct RexBody {
    pub machine: KtMachine,
    pub pos: V2,
    pub facing: f32,
    pub counters: HitCounters,
    pub attack: Option<RexAttack>,
    pub attack_cd: f32,
    pub charge_cd: f32,
    /// post-throw / post-get-up stagger in which the rex neither attacks nor walks [G]
    pub recover: f32,
    pub charge_dir: V2,
    pub charge_speed: f32,
    pub charge_travel: f32,
    pub charge_hit_done: bool,
    pub charge_stopped: bool,
    pub charge_blocked: bool,
    state_len: f32,
    fly: Option<Fly>,
    // one-shot messages for the next machine step
    hit_pending: bool,
    hit_flags: u32,
    grab_msg: Option<GrabMsg>,
    grab_anim: u16,
    shout_pending: bool,
    finish_done: bool,
    finish_escaped: bool,
    pub last_move: Option<RexMove>,
    pub was_in_ko: bool,
    /// attacker -> rex direction of the last blow (`Rex+0x2ac`)
    pub hit_dir: V2,
    /// KT clip the rex is playing (last `ANIM_Play`), its clock and play length in seconds
    pub anim: u32,
    pub anim_t: f32,
    pub anim_len: f32,
    /// KO clip phase: 0 none, 1 fall, 2 lying loop 0x3c, 3 get-up 0x1e
    pub ko_phase: u8,
    /// `Rex+0x2cc`: the last reaction (0x33) knocks the rex over sideways (0x21 / 0x22)
    pub knocked: bool,
    /// remaining clip shove of the 0x33 reaction (m, along `hit_dir`)
    shove_left: f32,
    shove_speed: f32,
    /// hit knock-back (`Rex+0x5e8` velocity, `+0x2c8` blend toward the animation's own motion,
    /// `+0x5f4` yaw kick) from `KT_exec_check_paf` / `fn@0x006e4970`, integrated by `KT_TRACK_tagon`
    pub kb_vel: V2,
    pub kb_blend: f32,
    pub kb_spin: f32,
}

impl RexBody {
    fn new(pos: V2, facing: f32, profile: RexProfile) -> Self {
        RexBody {
            machine: KtMachine::new(KtLife::new(profile.max, profile.init, profile.wound_ratio())),
            pos,
            facing,
            counters: HitCounters::default(),
            attack: None,
            attack_cd: 0.0,
            // the rex may charge as soon as its entry delay is over (Kong starts 24 m away) [G]
            charge_cd: 0.0,
            recover: 0.0,
            charge_dir: (1.0, 0.0),
            charge_speed: 0.0,
            charge_travel: 0.0,
            charge_hit_done: false,
            charge_stopped: false,
            charge_blocked: false,
            state_len: 0.0,
            fly: None,
            hit_pending: false,
            hit_flags: 0,
            grab_msg: None,
            grab_anim: 0,
            shout_pending: false,
            finish_done: false,
            finish_escaped: false,
            last_move: None,
            was_in_ko: false,
            hit_dir: (1.0, 0.0),
            anim: 0,
            anim_t: 0.0,
            anim_len: 0.0,
            ko_phase: 0,
            knocked: false,
            shove_left: 0.0,
            shove_speed: 0.0,
            kb_vel: (0.0, 0.0),
            kb_blend: 1.0,
            kb_spin: 0.0,
        }
    }

    pub fn state(&self) -> KtState {
        self.machine.state
    }
    pub fn life(&self) -> f32 {
        self.machine.life.cur
    }
    pub fn wounded(&self) -> bool {
        self.machine.life.is_wounded()
    }
    pub fn forward(&self) -> V2 {
        dir_of(self.facing)
    }
    /// The rex is in `fight_KONG` but cannot attack yet (entry delay or cool-down): a gap to close in.
    pub fn is_hesitating(&self) -> bool {
        self.machine.state == KtState::FightKong
            && (self.machine.entry_delay > 0.0 || self.attack_cd > 0.0 || self.recover > 0.0)
    }
    /// Seconds until the pending rex attack connects, if one is winding up.
    pub fn attack_eta(&self) -> Option<f32> {
        match (self.machine.state, self.attack) {
            (KtState::Attaque, Some(a)) if !a.hit_done => Some((a.hit_t - a.t).max(0.0)),
            _ => None,
        }
    }
    /// True during the recovery tail of an attack (after the blow, before the animation ends).
    pub fn in_attack_recovery(&self) -> bool {
        matches!((self.machine.state, self.attack), (KtState::Attaque, Some(a)) if a.hit_done && a.t < a.dur)
    }
}

// ---------------------------------------------------------------------------------------------
// The fight
// ---------------------------------------------------------------------------------------------

/// The rex's life triple (`Rex+0xf4/+0xf8/+0xfc`): max, initial, wound threshold (a life value) [C values per instance].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RexProfile {
    pub max: f32,
    pub init: f32,
    pub wounded_at: f32,
}

impl RexProfile {
    /// `PNJ_KTREX` model default (07D `J_PNJ_KTREX`, 10B `PNJ_KTREX`): 80 / 80 / 50 [C].
    pub const KT_DEFAULT: RexProfile = RexProfile { max: 80.0, init: 80.0, wounded_at: 50.0 };
    /// 07D arena rex `J_PNJ_KTREX_2` instance override: 50 / 50 / 25 [C] (`07d_instances.json`).
    pub const ARENA_07D: RexProfile = RexProfile { max: 50.0, init: 50.0, wounded_at: 25.0 };
    /// 05C marsh rexes `PNJ_KTREX` [c101f503]/[c101f50a] instance overrides: 55 / 55 / 40 [C] (`05c_instances.json`).
    pub const MARSH_05C: RexProfile = RexProfile { max: 55.0, init: 55.0, wounded_at: 40.0 };

    pub fn wound_ratio(&self) -> f32 {
        self.wounded_at / self.max
    }
}

impl Default for RexProfile {
    fn default() -> Self {
        RexProfile::KT_DEFAULT
    }
}

/// Everything selectable about a fight's life numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FightConfig {
    pub rex: RexProfile,
    pub kong_life: f32,
}

impl Default for FightConfig {
    fn default() -> Self {
        FightConfig { rex: RexProfile::KT_DEFAULT, kong_life: KONG_MAX_LIFE }
    }
}

impl FightConfig {
    /// The 07D arena fight (`J_PNJ_KTREX_2`, rex 50/50/25).
    pub fn arena_07d() -> Self {
        FightConfig { rex: RexProfile::ARENA_07D, kong_life: KONG_MAX_LIFE }
    }
}

#[derive(Clone, Debug)]
pub struct Fight {
    pub time: f32,
    pub kong: KongState,
    pub rex: RexBody,
    pub over: Option<Actor>,
    /// number of Kong blows delivered to the rex so far (for the brain / HUD)
    pub landed: u32,
    /// animation id of the last Kong blow that reached the rex (0 = none yet)
    pub last_landed_anim: u32,
    rng: Rng,
    acc: f32,
    pending_pressed: [bool; 4],
    events: Vec<FightEvent>,
    fury_was_active: bool,
}

/// What the rex offers to the attack button (`KT_exec_propose_grab`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proposal {
    /// kind 8: jaw-break finish (needs KO + wounded + first KO + Kong within 22) [C]
    Finish,
    /// kind 1: grab [G conditions]
    Grab,
}

impl Fight {
    /// Kong at (-8, 0) facing +x, rex at (16, 0) facing -x (24 m apart) [G]; placed so the rex's opening
    /// charge (28 m) and skid end inside the 32 m arena instead of in the wall behind Kong.
    pub fn new(seed: u32) -> Self {
        Self::with_config(seed, FightConfig::default())
    }

    /// Same as `new` with an explicit rex life profile (KT default 80/80/50 or the 07D arena rex 50/50/25).
    pub fn with_profile(seed: u32, rex: RexProfile) -> Self {
        Self::with_config(seed, FightConfig { rex, ..FightConfig::default() })
    }

    pub fn with_config(seed: u32, cfg: FightConfig) -> Self {
        let mut kong = KongState::new((-8.0, 0.0), 0.0);
        kong.life = cfg.kong_life;
        kong.max_life = cfg.kong_life;
        Fight {
            time: 0.0,
            kong,
            rex: RexBody::new((16.0, 0.0), std::f32::consts::PI, cfg.rex),
            over: None,
            landed: 0,
            last_landed_anim: 0,
            rng: Rng::new(seed),
            acc: 0.0,
            pending_pressed: [false; 4],
            events: Vec::new(),
            fury_was_active: false,
        }
    }

    pub fn dist(&self) -> f32 {
        len(sub(self.rex.pos, self.kong.pos))
    }

    /// Distance from Kong to the rex's surface.
    pub fn surface_dist(&self) -> f32 {
        self.dist() - REX_RADIUS
    }

    /// Unit vector from Kong to the rex.
    pub fn to_rex(&self) -> V2 {
        norm(sub(self.rex.pos, self.kong.pos))
    }

    /// The proposal the rex publishes to Kong right now.
    pub fn proposal(&self) -> Option<Proposal> {
        let r = &self.rex;
        if self.kong.mode != KongMode::Main && self.kong.mode != KongMode::Paf {
            return None;
        }
        if vrex::finish_accepts(r.state(), 0.0, r.wounded(), !r.machine.knocked_down_before, self.dist()) {
            return Some(Proposal::Finish);
        }
        let grabbable = match r.state() {
            KtState::Derap | KtState::Cri | KtState::KoAuSol => true,
            KtState::Attaque => r.in_attack_recovery(),
            _ => false,
        };
        if grabbable && self.dist() <= GRAB_RANGE {
            Some(Proposal::Grab)
        } else {
            None
        }
    }

    /// Advance the simulation by `dt` seconds (any size; it is cut into 1/60 s ticks) with Kong's pad.
    /// Button edges (`pressed`) are applied on the first tick they can be.
    pub fn step(&mut self, dt: f32, input: &KongInput) -> Vec<FightEvent> {
        for i in 0..4 {
            if input.buttons[i].pressed {
                self.pending_pressed[i] = true;
            }
        }
        self.acc += dt;
        while self.acc >= TICK - 1e-6 {
            self.acc -= TICK;
            if self.acc < 0.0 {
                self.acc = 0.0;
            }
            if self.over == Some(Actor::Rex) || (self.over.is_some() && self.kong.mode != KongMode::Victory && !self.kong.victory_done) {
                break;
            }
            let pressed = self.pending_pressed;
            self.pending_pressed = [false; 4];
            let held = [input.buttons[0].held, input.buttons[1].held, input.buttons[2].held, input.buttons[3].held];
            self.tick(held, pressed, input.stick);
        }
        std::mem::take(&mut self.events)
    }

    fn emit(&mut self, e: FightEvent) {
        self.events.push(e);
    }

    fn play(&mut self, id: u32) {
        let speed = self.kong.fury.anim_speed_mul();
        self.kong.anim = id;
        self.kong.anim_t = 0.0;
        self.emit(FightEvent::Anim { actor: Actor::Kong, id, speed });
    }

    fn shake(&mut self, amplitude: f32, decay: f32) {
        self.emit(FightEvent::CameraShake { amplitude, frequency: 35.0, duration: 0.15, decay });
    }

    // ------------------------------------------------------------------------------------
    // tick
    // ------------------------------------------------------------------------------------
    fn tick(&mut self, held: [bool; 4], pressed: [bool; 4], stick: V2) {
        self.time += TICK;
        // Kong first (his blows reach the rex's input this very tick), then the rex.
        match self.kong.mode {
            KongMode::Main => self.kong_main(held, pressed, stick),
            KongMode::Paf => self.kong_paf(pressed),
            KongMode::Grab => self.kong_grab(pressed),
            KongMode::Finisher => self.kong_finisher(pressed),
            KongMode::Victory => self.kong_victory(),
            KongMode::Dead => {}
        }
        self.kong.pos = add_scaled(self.kong.pos, self.kong.vel, TICK);
        self.kong.vel = (self.kong.vel.0 * 0.9, self.kong.vel.1 * 0.9);
        self.rex_update();
        self.separate();
        self.kong_life_rules();
        self.fury_tick();
        self.check_end();
    }

    fn separate(&mut self) {
        // Kong cannot walk into the rex
        if matches!(self.kong.mode, KongMode::Grab | KongMode::Finisher) {
            return;
        }
        let d = sub(self.kong.pos, self.rex.pos);
        let l = len(d);
        let min = REX_RADIUS + KONG_RADIUS;
        if l < min {
            let n = if l < 1e-4 { (-1.0, 0.0) } else { (d.0 / l, d.1 / l) };
            self.kong.pos = add_scaled(self.rex.pos, n, min);
        }
        self.kong.pos = clamp_arena(self.kong.pos, KONG_RADIUS);
    }

    fn fury_tick(&mut self) {
        let paused = matches!(self.kong.mode, KongMode::Finisher | KongMode::Grab)
            && fury::timer_paused_in_action(if self.kong.mode == KongMode::Finisher { 0x160 } else { 0x15e }, false);
        self.kong.fury.tick(TICK, paused);
        let active = self.kong.fury.is_active();
        if self.fury_was_active && !active {
            self.emit(FightEvent::FuryEnd);
        }
        self.fury_was_active = active;
    }

    fn check_end(&mut self) {
        if self.over.is_none() && self.kong.mode == KongMode::Dead {
            self.over = Some(Actor::Rex);
            self.emit(FightEvent::FightOver { winner: Actor::Rex });
        }
    }

    // ------------------------------------------------------------------------------------
    // Kong: life rules, hits received
    // ------------------------------------------------------------------------------------
    fn kong_life_rules(&mut self) {
        let k = &mut self.kong;
        if matches!(k.mode, KongMode::Dead | KongMode::Victory) || k.life > 0.0 {
            return;
        }
        match fury::life_zero_response(k.fury.is_active(), k.last_stand_timer, k.last_stand_flag, TICK) {
            fury::LifeZero::Revive(v) => {
                k.life = v;
                k.last_stand_timer = 0.0;
                self.emit(FightEvent::KongRevived { life: v });
            }
            fury::LifeZero::StartLastStand(t) => {
                k.last_stand_timer = t;
                k.last_stand_flag = true;
                self.emit(FightEvent::LastStand { seconds: t });
            }
            fury::LifeZero::LastStandTick(t) => {
                k.last_stand_timer = t;
                if t <= 0.0 {
                    k.life = KONG_RESTORE_RATIO * k.max_life;
                    k.last_stand_flag = false;
                }
            }
            fury::LifeZero::Restore => {
                k.life = KONG_RESTORE_RATIO * k.max_life;
                k.last_stand_flag = false;
            }
        }
    }

    /// A rex packet reaches Kong. Returns whether it connected.
    fn kong_receive(&mut self, mv: RexMove, flags: u32, damage: i32) -> bool {
        if matches!(self.kong.mode, KongMode::Dead | KongMode::Victory | KongMode::Grab | KongMode::Finisher) {
            return false;
        }
        if self.kong.invulnerable() {
            let by = self.kong.anim;
            self.emit(FightEvent::HitAvoided { attack: mv, by_anim: by });
            return false;
        }
        let pos3 = [self.kong.pos.0, self.kong.pos.1, 2.0];
        let wounded = self.kong.is_wounded();
        // last stand: a hit while the flag is set kills when < 5 s remain, else cuts it to 5 s [C]
        if self.kong.last_stand_flag {
            if fury::last_stand_hit_kills(self.kong.last_stand_timer) {
                self.kong.life = 0.0;
                self.kong.mode = KongMode::Dead;
                self.emit(FightEvent::KongMode { mode: KongMode::Dead });
                self.emit(FightEvent::Hit { attacker: Actor::Rex, victim: Actor::Kong, class: flags, damage, applied: 0.0, pos: pos3, anim: 0 });
                self.emit(FightEvent::KongDied);
                return true;
            }
            self.kong.last_stand_timer = self.kong.last_stand_timer.min(fury::HIT_PENALTY_HARD);
        }
        let before = self.kong.life;
        self.kong.life = (self.kong.life - damage as f32).max(0.0);
        let applied = before - self.kong.life;
        self.emit(FightEvent::Hit { attacker: Actor::Rex, victim: Actor::Kong, class: flags, damage, applied, pos: pos3, anim: 0 });
        if let Some(a) = combat::hit_shake_amplitude(flags) {
            self.shake(a, 1.1);
        }
        self.kong.fury.on_kong_hit(flags, wounded);
        // hit-stun
        let from_rex = norm(sub(self.kong.pos, self.rex.pos));
        let fwd = self.kong.forward();
        let side = (-fwd.1, fwd.0);
        let (anim, strength) = combat::hit_reaction(flags, dot(from_rex, fwd), dot(from_rex, side));
        self.kong.mode = KongMode::Paf;
        self.kong.phase = Phase::None;
        self.kong.latched = Latched::default();
        let heavy = flags & 2 != 0;
        self.kong.paf_len = if heavy { 54.0 } else { 36.0 } / FPS;
        self.kong.vel = (from_rex.0 * strength.min(42) as f32 * 0.3, from_rex.1 * strength.min(42) as f32 * 0.3);
        self.kong.dodge_left = 0.0;
        self.emit(FightEvent::KongMode { mode: KongMode::Paf });
        self.play(anim);
        self.emit(FightEvent::KongStunned { anim, strength });
        true
    }

    fn kong_paf(&mut self, pressed: [bool; 4]) {
        let sp = self.kong.fury.anim_speed_mul();
        self.kong.anim_t += TICK * sp;
        // light hit-stun anims 0x28 / 0x2b: an attack press after frame 0xf restarts the punch chain [C]
        if pressed[SLOT_ATTACK]
            && matches!(self.kong.anim, 0x28 | 0x2b)
            && self.kong.frame() > combat::HIT_STUN_PUNCH_CANCEL_FRAME as f32
        {
            self.back_to_main();
            self.kong.latched.attack = true;
            self.start_phase(Phase::Punch1);
            return;
        }
        if self.kong.anim_t >= self.kong.paf_len {
            self.back_to_main();
        }
    }

    fn back_to_main(&mut self) {
        self.kong.mode = KongMode::Main;
        self.kong.phase = Phase::None;
        self.kong.latched = Latched::default();
        self.kong.anim = combat::ANIM_IDLE;
        self.kong.anim_t = 0.0;
        self.emit(FightEvent::KongMode { mode: KongMode::Main });
        self.emit(FightEvent::Anim { actor: Actor::Kong, id: combat::ANIM_IDLE, speed: 1.0 });
    }

    // ------------------------------------------------------------------------------------
    // Kong: main state
    // ------------------------------------------------------------------------------------
    fn kong_main(&mut self, held: [bool; 4], pressed: [bool; 4], stick: V2) {
        {
            let l = &mut self.kong.latched;
            l.jump_roll |= pressed[SLOT_JUMP_ROLL];
            l.special |= pressed[SLOT_SPECIAL];
            l.attack |= pressed[SLOT_ATTACK];
            l.cancel |= pressed[SLOT_CANCEL];
        }
        let sp = self.kong.fury.anim_speed_mul();
        self.kong.anim_t += TICK * sp;
        self.kong.phase_t += TICK;
        let phase = self.kong.phase;
        let info = anim_info(self.kong.anim);
        let frame = self.kong.frame();
        let ended = frame >= info.len;
        match phase {
            Phase::None => {
                self.locomotion(stick, sp);
                if self.kong.latched.any() {
                    let n = self.next_for(Phase::None);
                    self.resolve(n, stick);
                }
            }
            Phase::ChestPound => self.tick_pound(held, frame, info, ended),
            Phase::Recover => {
                // recovery animation: any latched press may start the next action after a short wait
                if ended {
                    self.kong.phase = Phase::None;
                    self.kong.anim = combat::ANIM_IDLE;
                    self.emit(FightEvent::Anim { actor: Actor::Kong, id: combat::ANIM_IDLE, speed: 1.0 });
                } else if frame >= info.cancel && self.kong.latched.any() {
                    let n = self.next_for(Phase::None);
                    self.resolve(n, stick);
                }
            }
            Phase::DodgeRoll | Phase::DodgeSide => {
                if self.kong.dodge_left > 0.0 {
                    let step = self.kong.dodge_speed * sp;
                    self.kong.pos = add_scaled(self.kong.pos, self.kong.dodge_dir, step * TICK);
                    self.kong.dodge_left -= TICK;
                }
                let can_chain = phase == Phase::DodgeSide && frame >= combat::DODGE_CHAIN_FRAME as f32;
                let ready = ended || (can_chain && self.kong.latched.any()) || (self.kong.latched.attack && frame >= DODGE_ATTACK_CANCEL_FRAME);
                if ready {
                    let n = self.next_for(phase);
                    self.resolve(n, stick);
                }
            }
            _ => self.tick_attack_phase(phase, info, frame, ended, stick, sp),
        }
        self.kong.pos = clamp_arena(self.kong.pos, KONG_RADIUS);
    }

    fn locomotion(&mut self, stick: V2, sp: f32) {
        let raw = len(stick);
        let mag = movement::stick_magnitude(raw);
        if mag > 0.0 {
            let d = norm(stick);
            self.kong.facing = angle_of(d);
            self.kong.pos = add_scaled(self.kong.pos, d, KONG_RUN_SPEED * mag * sp * TICK);
            let want = if mag >= 0.99 { ANIM_RUN } else { ANIM_WALK };
            if self.kong.anim != want {
                self.play(want);
            }
        } else if self.kong.anim == ANIM_WALK || self.kong.anim == ANIM_RUN {
            self.play(combat::ANIM_IDLE);
        }
    }

    fn next_for(&self, cur: Phase) -> Next {
        let ctx = ComboCtx {
            grab_offered: self.proposal().is_some(),
            carrying: false,
            special_target: SpecialTarget::None,
            test_grab_state: false,
        };
        combat::next_phase(cur, &self.kong.latched, &ctx)
    }

    fn resolve(&mut self, n: Next, stick: V2) {
        match n {
            Next::Stay => {}
            Next::GrabState => match self.proposal() {
                Some(Proposal::Finish) => self.start_finisher(),
                Some(Proposal::Grab) => self.start_grab(),
                None => self.end_phase(),
            },
            Next::TestGrabState => self.end_phase(),
            Next::Phase(p) => self.start_phase(p),
            Next::ObstacleResponse => self.start_dodge(stick),
            Next::End => self.end_phase(),
        }
    }

    fn end_phase(&mut self) {
        let ended = self.kong.phase;
        self.kong.latched = Latched::default();
        match combat::recovery_anim(ended, self.kong.anim) {
            Some(a) if a != combat::ANIM_IDLE => {
                self.kong.phase = Phase::Recover;
                self.emit(FightEvent::KongPhase { phase: Phase::Recover });
                self.play(a);
            }
            _ => {
                self.kong.phase = Phase::None;
                if self.kong.anim != combat::ANIM_IDLE {
                    self.play(combat::ANIM_IDLE);
                }
            }
        }
    }

    /// Aim assist (`fn@0x008848e0`): snap the facing to the rex if he is within the cone and reach.
    fn aim_assist(&mut self, cone_cos: f32, reach: f32) {
        let to = self.to_rex();
        if self.surface_dist() <= reach && dot(self.kong.forward(), to) >= cone_cos {
            self.kong.facing = angle_of(to);
        }
    }

    fn start_phase(&mut self, p: Phase) {
        self.kong.phase = p;
        self.kong.latched = Latched::default();
        self.kong.hit_done = false;
        self.kong.swing_done = false;
        self.kong.shake_done = false;
        self.kong.phase_t = 0.0;
        self.kong.attacks_started += 1;
        self.kong.fury.begin_action();
        let params = combat::phase_params(p);
        self.kong.step_speed = params.step_speed;
        let anim = match p {
            Phase::Punch1 => {
                // the stick is not consulted by the brain's usual approach: facing the rex gives the straight punch
                let (a, prm) = combat::punch_variant(0.0, -1.0, false);
                self.kong.step_speed = prm.step_speed;
                a
            }
            other => other.start_anim().unwrap_or(combat::ANIM_IDLE),
        };
        match p {
            Phase::Punch1 | Phase::Punch2 | Phase::Repel | Phase::Downward => self.aim_assist(combat::AIM_ASSIST_COS, combat::AIM_ASSIST_REACH),
            Phase::CounterLunge => self.aim_assist(0.866, 20.0),
            _ => {}
        }
        if p == Phase::ChestPound {
            self.kong.pound = PoundWindow::default();
            self.kong.step_speed = 0.0;
            self.emit(FightEvent::PoundStart);
        }
        self.emit(FightEvent::KongPhase { phase: p });
        self.play(anim);
    }

    fn start_dodge(&mut self, stick: V2) {
        self.kong.latched = Latched::default();
        self.kong.phase_t = 0.0;
        self.kong.hit_done = false;
        let to = self.to_rex();
        let mag = len(stick);
        let sd = if mag > 1e-3 { norm(stick) } else { (0.0, 0.0) };
        let gate = self.dist() * self.dist() <= combat::SIDESTEP_MAX_DIST_SQ && dot(sd, to) >= combat::SIDESTEP_MIN_DOT;
        if gate && self.rex.state() != KtState::Mort {
            // side-step: anim 6 / 7 by the sign of the side dot; invulnerable while it plays [C]
            let perp = (-to.1, to.0);
            let side = dot(sd, perp);
            let sign = if side.abs() > 0.2 {
                side.signum()
            } else {
                self.kong.dodge_flip = !self.kong.dodge_flip;
                if self.kong.dodge_flip { 1.0 } else { -1.0 }
            };
            self.kong.dodge_dir = (perp.0 * sign, perp.1 * sign);
            self.kong.dodge_speed = 6.0;
            self.kong.dodge_left = 0.83;
            self.kong.phase = Phase::DodgeSide;
            self.emit(FightEvent::KongPhase { phase: Phase::DodgeSide });
            self.play(if sign > 0.0 { combat::ANIM_SIDESTEP_R } else { combat::ANIM_SIDESTEP_L });
        } else {
            let d = if mag > 1e-3 { sd } else { self.kong.forward() };
            self.kong.dodge_dir = d;
            self.kong.dodge_speed = 8.0;
            self.kong.dodge_left = 0.6;
            self.kong.facing = angle_of(d);
            self.kong.phase = Phase::DodgeRoll;
            self.emit(FightEvent::KongPhase { phase: Phase::DodgeRoll });
            self.play(combat::ANIM_ROLL);
        }
        self.kong.fury.begin_action();
    }

    fn tick_pound(&mut self, held: [bool; 4], frame: f32, info: AnimInfo, ended: bool) {
        let l = self.kong.latched;
        let res = self.kong.pound.step(
            TICK,
            held[SLOT_SPECIAL],
            frame as i32,
            frame >= info.cancel,
            ended,
            [l.jump_roll, l.attack, l.cancel],
        );
        match res {
            PoundResult::Continue => {}
            PoundResult::Exit => self.end_phase(),
            PoundResult::OtherButton(_) => {
                let n = self.next_for(Phase::None);
                self.resolve(n, (0.0, 0.0));
            }
            PoundResult::FireFury => {
                self.kong.fury.start();
                let radius = self.kong.pound.shout_radius;
                self.emit(FightEvent::FuryShout { radius });
                if self.dist() <= fury::REACT_RADIUS_KT {
                    self.rex.shout_pending = true;
                }
                let t = self.kong.fury.timer;
                self.emit(FightEvent::FuryStart { timer: t });
                self.kong.phase = Phase::Recover;
                self.kong.latched = Latched::default();
                self.emit(FightEvent::KongPhase { phase: Phase::Recover });
                self.play(fury::ANIM_FURY_ROAR);
                self.fury_was_active = true;
            }
        }
    }

    /// Is the rex inside the blow's reach and in front of Kong?
    fn rex_in_reach(&self, counter: bool) -> bool {
        let vol = if counter { combat::COUNTER_VOLUME_SCALE.0 } else { combat::HIT_SWEEP_REACH };
        self.surface_dist() <= vol + KONG_HAND_REACH && dot(self.kong.forward(), self.to_rex()) >= KONG_HIT_CONE_COS
    }

    fn tick_attack_phase(&mut self, phase: Phase, info: AnimInfo, frame: f32, ended: bool, stick: V2, sp: f32) {
        // forward step ("lunge"): `Kong+0x1ae8` metres per second for the first 0.3 s, stopping at the rex [C speed, G time]
        let counter = phase == Phase::CounterLunge;
        let (speed, until) = if counter { (12.0, 0.25) } else { (self.kong.step_speed, 0.3) };
        // the rex slides 8/3 m under each blow (knock-back, KT_TRACK_tagon): the step keeps closing in
        // up to the blow's first hit frame so chained blows stay in reach [G]
        let closing = info.hit.map_or(false, |(a, _)| frame < a && !self.kong.hit_done);
        if speed > 0.0 && (self.kong.phase_t < until || closing) && self.surface_dist() > 2.0 {
            self.kong.pos = add_scaled(self.kong.pos, self.kong.forward(), speed * sp * TICK);
        }
        // the blow
        if let Some((a, b)) = info.hit {
            if !self.kong.swing_done && frame >= a {
                // the hit window opened: anything breakable in front of Kong is struck (the level decides)
                self.kong.swing_done = true;
                let (anim, pos, facing) = (self.kong.anim, self.kong.pos, self.kong.facing);
                self.emit(FightEvent::KongSwing { anim, pos, facing });
            }
            if !self.kong.hit_done && frame >= a && frame <= b && self.rex_in_reach(counter) {
                self.kong.hit_done = true;
                self.kong_blow();
            }
        }
        // downward strike shake, once past frame 0x20 [C]
        if phase == Phase::Downward && !self.kong.shake_done && frame > combat::DOWNWARD_SHAKE_FRAME as f32 {
            self.kong.shake_done = true;
            self.shake(0.05, 0.99);
        }
        let ready = combat::ready_for_input(ended, frame >= info.cancel, &self.kong.latched);
        if ready {
            let n = self.next_for(phase);
            self.resolve(n, stick);
        }
    }

    /// Kong's blow lands on the rex: damage and class from `combat::blow`, rex rules from `vrex`.
    fn kong_blow(&mut self) {
        let b = combat::blow(&BlowCtx { action: combat::action::MAIN, anim: self.kong.anim, fury: self.kong.fury.is_active(), ..Default::default() });
        let anim = self.kong.anim;
        self.deliver_to_rex(b.class, b.damage, anim, true);
    }

    /// Apply a Kong packet to the rex. `flinch`: normal hit path (rex goes to `paf`); grab moves
    /// bypass it (hits are ignored while a grab message is active, B02).
    fn deliver_to_rex(&mut self, class: u32, damage: i32, anim: u32, flinch: bool) {
        let st = self.rex.state();
        if matches!(st, KtState::Mort | KtState::Finish) {
            return;
        }
        let life_before = self.rex.life();
        let wounded = self.rex.wounded();
        let applied = vrex::hit_damage(class, class, wounded, damage as f32);
        self.rex.machine.life.apply_damage(applied);
        self.landed += 1;
        self.last_landed_anim = anim;
        let pos3 = [self.rex.pos.0, self.rex.pos.1, 3.0];
        self.emit(FightEvent::Hit { attacker: Actor::Kong, victim: Actor::Rex, class, damage, applied, pos: pos3, anim });
        if vrex::knockdown_capable(class, life_before, self.kong.fury.is_active(), st) {
            self.emit(FightEvent::KnockdownFeedback { class });
        }
        if let Some(a) = combat::hit_shake_amplitude(class) {
            self.shake(a, 1.1);
        }
        // fury charge / extend: once per attack action, victim must have life [C fn@0x00883050]
        if life_before > 0.0 {
            let before = self.kong.fury.timer;
            let was = self.kong.fury.is_active();
            let in47 = matches!(self.kong.phase, Phase::ChestPound | Phase::DodgeRoll);
            let kw = self.kong.is_wounded();
            self.kong.fury.on_hit_landed(kw, wounded, class, in47);
            if was && self.kong.fury.timer != before {
                let t = self.kong.fury.timer;
                self.emit(FightEvent::FuryExtend { timer: t });
            }
        }
        if flinch && !matches!(st, KtState::Grabbed) {
            self.rex.hit_dir = norm(sub(self.rex.pos, self.kong.pos));
            // knock-back impulse: 8.0 m/s along the blow, none for the no-damage class 0x10 [C]
            let h = self.rex.hit_dir;
            let impulse = if class & combat::HIT_NO_DAMAGE != 0 { 0.0 } else { vrex::KNOCKBACK_SPEED };
            self.rex.kb_vel = (h.0 * impulse, h.1 * impulse);
            self.rex.kb_blend = 0.0;
            // yaw kick: blows that land off the hips (Kong strikes the head/neck, in front of the
            // pivot) turn the rex away from the blow's side; |kick| <= 1 rad [C formula, L hit point]
            let fwd = self.rex.forward();
            let left = (-fwd.1, fwd.0);
            let lever = if dot(sub(self.kong.pos, self.rex.pos), fwd) >= 0.0 { 1.0 } else { -1.0 };
            self.rex.kb_spin = -dot(left, h) * lever;
            self.rex.counters.register(class);
            self.rex.hit_pending = true;
            self.rex.hit_flags = class;
            if class & combat::HIT_REPEL != 0 {
                // shove: the hit vector * 5.0 [C]; displacement in metres is [G]
                let away = norm(sub(self.rex.pos, self.kong.pos));
                self.rex.pos = clamp_arena(add_scaled(self.rex.pos, away, 4.0), REX_RADIUS);
            }
        }
    }

    // ------------------------------------------------------------------------------------
    // Kong: grab
    // ------------------------------------------------------------------------------------
    fn start_grab(&mut self) {
        self.kong.mode = KongMode::Grab;
        self.kong.phase = Phase::None;
        self.kong.latched = Latched::default();
        self.kong.grab = GrabStage::Reach(0.0);
        let to = self.to_rex();
        self.kong.facing = angle_of(to);
        self.rex.grab_msg = Some(GrabMsg::Grab);
        self.emit(FightEvent::KongMode { mode: KongMode::Grab });
        self.emit(FightEvent::GrabStart);
        self.play(ANIM_GRAB_START);
    }

    fn kong_grab(&mut self, pressed: [bool; 4]) {
        let sp = self.kong.fury.anim_speed_mul();
        self.kong.anim_t += TICK * sp;
        // keep the rex in Kong's hands
        if self.rex.state() == KtState::Grabbed {
            let f = self.kong.forward();
            self.rex.pos = add_scaled(self.kong.pos, f, REX_RADIUS + 1.5);
            self.rex.facing = angle_of((-f.0, -f.1));
        }
        let frame = self.kong.frame();
        match self.kong.grab {
            GrabStage::Reach(t) => {
                let t = t + TICK;
                if self.rex.state() != KtState::Grabbed && t > 3.0 * TICK {
                    // the rex did not accept the grab (state changed): abort
                    self.back_to_main();
                    return;
                }
                if frame >= anim_info(ANIM_GRAB_START).len {
                    self.kong.grab = GrabStage::Holding(0.0);
                    self.kong.anim = combat::ANIM_IDLE;
                } else {
                    self.kong.grab = GrabStage::Reach(t);
                }
            }
            GrabStage::Holding(idle) => {
                let idle = idle + TICK;
                let limit = if self.rex.wounded() { GRAB_HOLD_TIMEOUT_WOUNDED } else { GRAB_HOLD_TIMEOUT };
                self.kong.grab = GrabStage::Holding(idle);
                if pressed[SLOT_ATTACK] {
                    self.kong.fury.begin_action();
                    self.kong.grab = GrabStage::Strike { t: 0.0, hit_done: false };
                    self.play(combat::GRAB_STRIKE_ANIM);
                    self.emit(FightEvent::GrabStrike);
                } else if pressed[SLOT_SPECIAL] {
                    self.kong.grab = GrabStage::Throw { t: 0.0, released: false };
                    self.play(ANIM_THROW);
                } else if pressed[SLOT_JUMP_ROLL] {
                    self.kong.grab = GrabStage::Slam { t: 0.0, done: false };
                    self.play(ANIM_SLAM);
                } else if pressed[SLOT_CANCEL] || idle > limit {
                    // release: the rex goes on (life 0 -> KO via anim 0x7a)
                    self.release_rex(0x7a);
                    if !pressed[SLOT_CANCEL] {
                        self.emit(FightEvent::GrabBrokeFree);
                    }
                    self.back_to_main();
                }
            }
            GrabStage::Strike { t, hit_done } => {
                let t = t + TICK;
                let mut hd = hit_done;
                if !hd && frame > combat::GRAB_STRIKE_FRAME as f32 {
                    hd = true;
                    let b = combat::GRAB_STRIKE;
                    self.deliver_to_rex(b.class, b.damage, combat::GRAB_STRIKE_ANIM, false);
                }
                if frame >= anim_info(combat::GRAB_STRIKE_ANIM).len {
                    self.kong.grab = GrabStage::Holding(0.0);
                    self.kong.anim = combat::ANIM_IDLE;
                } else {
                    self.kong.grab = GrabStage::Strike { t, hit_done: hd };
                }
            }
            GrabStage::Throw { t, released } => {
                let t = t + TICK;
                let mut rel = released;
                if !rel && frame >= THROW_RELEASE_FRAME {
                    rel = true;
                    let d = self.kong.forward();
                    self.rex.fly = Some(Fly { dir: d, left: THROW_DISTANCE });
                    self.rex.machine.prev = KtState::Grabbed;
                    self.rex.machine.state = KtState::Projectile;
                    self.rex.machine.timer = 0.0;
                    self.rex.state_len = 4.0;
                    self.emit(FightEvent::RexState { from: KtState::Grabbed, to: KtState::Projectile });
                    self.emit(FightEvent::Throw { dir: d });
                    self.rex_play(REX_ANIM_THROWN, 1.0);
                }
                if frame >= anim_info(ANIM_THROW).len {
                    self.back_to_main();
                } else {
                    self.kong.grab = GrabStage::Throw { t, released: rel };
                }
            }
            GrabStage::Slam { t, done } => {
                let t = t + TICK;
                let mut dn = done;
                if !dn && frame >= SLAM_RELEASE_FRAME {
                    dn = true;
                    self.rex.machine.life.apply_damage(SLAM_DAMAGE);
                    self.emit(FightEvent::Slam { damage: SLAM_DAMAGE });
                    self.shake(0.075, 1.1);
                    self.release_rex(REX_ANIM_SLAMMED as u16);
                }
                if frame >= anim_info(ANIM_SLAM).len {
                    self.back_to_main();
                } else {
                    self.kong.grab = GrabStage::Slam { t, done: dn };
                }
            }
        }
    }

    fn release_rex(&mut self, anim: u16) {
        self.rex.grab_anim = anim;
    }

    // ------------------------------------------------------------------------------------
    // Kong: jaw-break finisher
    // ------------------------------------------------------------------------------------
    fn start_finisher(&mut self) {
        self.kong.mode = KongMode::Finisher;
        self.kong.phase = Phase::None;
        self.kong.latched = Latched::default();
        self.kong.finisher = Some(FinisherState { mash: FinishMash::new(), t: 0.0, wait: 0, won_t: None });
        let to = self.to_rex();
        self.kong.facing = angle_of(to);
        self.rex.grab_msg = Some(GrabMsg::Finish);
        self.emit(FightEvent::KongMode { mode: KongMode::Finisher });
        self.emit(FightEvent::FinisherStart);
        self.play(ANIM_FINISH_LOCK);
    }

    fn kong_finisher(&mut self, pressed: [bool; 4]) {
        // message 0x16 received: Kong plays 0xe8 on the rex; at frame 100 (or its end) he sends 0x17,
        // the rex goes to `mort` (k_ETAT_finish) [C]
        if let Some(f) = self.kong.finisher.as_mut() {
            if let Some(w) = f.won_t {
                let w = w + TICK;
                f.won_t = Some(w);
                self.kong.anim_t += TICK;
                self.snap_onto_rex(1.0);
                if (w - TICK) * FPS <= FINISH_KILL_FRAME && w * FPS > FINISH_KILL_FRAME {
                    self.rex.finish_done = true;
                }
                return;
            }
        }
        if self.rex.state() != KtState::Finish {
            // waiting for the rex to enter `Finish` (one tick); give up if it never does
            let give_up = match self.kong.finisher.as_mut() {
                Some(f) => {
                    f.wait += 1;
                    f.wait > 6
                }
                None => true,
            };
            if give_up {
                self.kong.finisher = None;
                self.back_to_main();
            }
            return;
        }
        let mut f = match self.kong.finisher.take() {
            Some(f) => f,
            None => return,
        };
        f.t += TICK;
        // message 0x14: Kong's root slides onto the rex's root and turns to the rex's axis at 6*dt; from
        // the mash clip on (0x15) he stays snapped there (the paired clips are authored on one root) [C]
        let lock_done = f.t >= anim_info(ANIM_FINISH_LOCK).len / FPS;
        self.snap_onto_rex(if lock_done { 1.0 } else { (6.0 * TICK).min(1.0) });
        let pressing = pressed[SLOT_ATTACK] || pressed[SLOT_SPECIAL] || pressed[SLOT_CANCEL];
        // Kong's pull value (`Kong_MashPullValue@0x602e40`) [C]: 0 during the lock clip 0xe6 (20 frames), then the ramp of the 346 frame
        // mash clip 0xe7, which `ANIM_Play` restarts whenever it has ended
        let lock_s = anim_info(ANIM_FINISH_LOCK).len / FPS;
        let value = finish_pull_value(f.t);
        let anim_frame = (f.t * FPS).min(FINISH_ANIM_LEN);
        let before_lock = f.mash.elapsed < vrex::FINISH_LOCK_SECONDS;
        let out = f.mash.step(TICK, pressing, value, self.kong.fury.is_active(), anim_frame, FINISH_ANIM_LEN);
        if f.t >= lock_s && self.kong.anim == ANIM_FINISH_LOCK {
            self.play(ANIM_FINISH_MASH);
        }
        if pressing && !before_lock {
            let (p, pu, re) = (f.mash.progress, f.mash.pull, f.mash.resist);
            self.emit(FightEvent::FinisherMash { progress: p, pull: pu, resist: re });
        }
        match out {
            FinishOutcome::Running => {
                self.kong.finisher = Some(f);
            }
            FinishOutcome::Won => {
                // the cursor passed the clip end: the rex sends 0x16, plays 0x38; Kong plays 0xe8 [C]
                self.emit(FightEvent::FinisherSuccess);
                self.play(ANIM_FINISH_WIN);
                self.rex_play(REX_ANIM_FINISH_WON, 1.0);
                f.won_t = Some(0.0);
                self.kong.finisher = Some(f);
                // Kong waits in the finisher mode until the rex's `mort` triggers the victory
            }
            FinishOutcome::Escaped => {
                self.rex.finish_escaped = true;
                self.emit(FightEvent::FinisherEscape);
                self.emit(FightEvent::Anim { actor: Actor::Rex, id: REX_ANIM_FINISH_ESCAPE, speed: 1.0 });
                self.play(ANIM_FINISH_ESCAPE);
                self.kong.finisher = None;
                // shaken off: Kong is thrown back [G]. His root sat on the rex's root; put him back where
                // his body was (in front of the rex) before pushing him away.
                let away = self.rex.forward();
                self.kong.pos = add_scaled(self.rex.pos, away, REX_RADIUS + KONG_RADIUS);
                self.kong.facing = angle_of((-away.0, -away.1));
                self.kong.vel = (away.0 * 20.0, away.1 * 20.0);
                self.back_to_main();
            }
        }
    }

    // ------------------------------------------------------------------------------------
    // Victory
    // ------------------------------------------------------------------------------------
    fn begin_victory(&mut self) {
        if self.kong.victory_done || self.kong.mode == KongMode::Victory || self.kong.mode == KongMode::Dead {
            return;
        }
        // after the jaw-break, the pound waits for the end of 0xe8 [C: 0xe8 plays to its end]
        let wait = match self.kong.finisher.as_ref().and_then(|f| f.won_t) {
            Some(w) => (anim_info(ANIM_FINISH_WIN).len / FPS - w).max(0.0),
            None => 0.0,
        };
        self.kong.mode = KongMode::Victory;
        self.kong.finisher = None;
        self.kong.victory_t = 0.0;
        self.kong.victory_pound_at = wait;
        self.kong.phase = Phase::None;
        self.over = Some(Actor::Kong);
        self.emit(FightEvent::KongMode { mode: KongMode::Victory });
        if wait <= 0.0 {
            self.victory_pound();
        }
    }

    fn victory_pound(&mut self) {
        self.emit(FightEvent::VictoryPound);
        self.play(combat::ANIM_POUND);
        self.shake(0.075, 1.1);
    }

    /// Kong's root onto the rex's root and his axis onto the rex's axis (`k`: blend factor this tick).
    fn snap_onto_rex(&mut self, k: f32) {
        let r = self.rex.pos;
        let p = self.kong.pos;
        self.kong.pos = (p.0 + (r.0 - p.0) * k, p.1 + (r.1 - p.1) * k);
        let mut d = (self.rex.facing - self.kong.facing) % std::f32::consts::TAU;
        if d > std::f32::consts::PI {
            d -= std::f32::consts::TAU;
        } else if d < -std::f32::consts::PI {
            d += std::f32::consts::TAU;
        }
        self.kong.facing += d * k;
    }

    fn kong_victory(&mut self) {
        let before = self.kong.victory_t;
        self.kong.victory_t += TICK;
        self.kong.anim_t += TICK;
        let p0 = self.kong.victory_pound_at;
        if p0 > 0.0 && before < p0 && self.kong.victory_t >= p0 {
            self.victory_pound();
        }
        let roar_at = p0 + 2.5;
        let end_at = p0 + 5.0;
        if before < roar_at && self.kong.victory_t >= roar_at {
            self.emit(FightEvent::VictoryRoar);
            self.play(combat::ANIM_ROAR);
        }
        if before < end_at && self.kong.victory_t >= end_at {
            self.emit(FightEvent::FightOver { winner: Actor::Kong });
            // back to free control: walk, run, swing (the dead rex no longer reacts)
            self.kong.victory_done = true;
            self.kong.mode = KongMode::Main;
            self.kong.phase = Phase::None;
            self.emit(FightEvent::KongMode { mode: KongMode::Main });
            self.play(combat::ANIM_IDLE);
        }
    }

    pub fn is_finished(&self) -> bool {
        match self.over {
            Some(Actor::Rex) => true,
            Some(Actor::Kong) => self.kong.victory_done,
            None => false,
        }
    }

    // ------------------------------------------------------------------------------------
    // Rex
    // ------------------------------------------------------------------------------------
    fn rex_update(&mut self) {
        let r_state_before = self.rex.state();
        if r_state_before == KtState::Mort {
            self.begin_victory();
            return;
        }
        let dt = TICK;
        {
            let r = &mut self.rex;
            r.attack_cd = (r.attack_cd - dt).max(0.0);
            r.charge_cd = (r.charge_cd - dt).max(0.0);
            r.recover = (r.recover - dt).max(0.0);
            r.counters.tick(dt);
        }
        let kong_pos = self.kong.pos;
        let to_kong = norm(sub(kong_pos, self.rex.pos));
        let dist = self.dist();
        let facing_cos = dot(self.rex.forward(), to_kong);
        let kong_faces_rex = dot(self.kong.forward(), sub(self.rex.pos, kong_pos)) > 0.0;
        // counter due: while in paf past anim frame 19 [C]
        let mut counter_due = false;
        if self.rex.state() == KtState::Paf && self.rex.machine.timer > 19.0 / FPS && self.rex.machine.timer < self.rex.state_len {
            counter_due = self.rex.counters.take_counter();
        }
        let timer = self.rex.machine.timer;
        let in_ko = self.rex.state() == KtState::KoAuSol;
        let ko_getting_up = in_ko && self.rex.ko_phase == 3;
        let anim_done = if in_ko {
            ko_getting_up && self.rex.anim_t >= self.rex.anim_len
        } else {
            timer >= self.rex.state_len && self.rex.state_len > 0.0
        };
        let in_pound = self.kong.mode == KongMode::Main && self.kong.phase == Phase::ChestPound;
        let inp = KtInput {
            dt,
            hit: self.rex.hit_pending,
            grab_msg: self.rex.grab_msg,
            target: Target::Kong,
            dist,
            facing_cos,
            anim_done,
            fury_shout: self.rex.shout_pending,
            attack_ready: self.rex.attack_cd <= 0.0 && self.rex.recover <= 0.0,
            line_clear: true,
            charge_ready: self.rex.charge_cd <= 0.0 && self.rex.recover <= 0.0,
            target_in_finish: in_pound,
            kong_fury: self.kong.fury.is_active(),
            kong_mashing: false,
            kong_faces_rex,
            counter_due,
            hit_by_ann: false,
            charge_blocked: self.rex.charge_blocked,
            charge_stopped: self.rex.charge_stopped,
            grab_anim: self.rex.grab_anim,
            finish_done: self.rex.finish_done,
            finish_escaped: self.rex.finish_escaped,
            ko_getting_up,
            ..KtInput::default()
        };
        self.rex.hit_pending = false;
        self.rex.shout_pending = false;
        self.rex.grab_msg = None;
        self.rex.grab_anim = 0;
        self.rex.charge_blocked = false;
        self.rex.charge_stopped = false;
        self.rex.finish_done = false;
        self.rex.finish_escaped = false;
        let ns = self.rex.machine.step(&inp);
        if ns != r_state_before {
            self.on_rex_enter(r_state_before, ns);
        }
        if ns == KtState::Paf && (inp.hit || ns != r_state_before) {
            self.rex_paf_reaction();
        } else if ns == KtState::KoAuSol && r_state_before == KtState::KoAuSol && inp.hit {
            // ground hit 0x20, then back to the lying loop (the hold clock stops meanwhile) [C]
            self.rex_play(0x20, 1.0);
            self.rex.ko_phase = 1;
            self.emit(FightEvent::RexGroundHit);
        }
        self.rex_anim_tick(ns);
        self.rex_behave(ns, to_kong);
        if self.rex.state() == KtState::Mort {
            self.begin_victory();
        }
    }

    /// `ANIM_Play` on the rex: remembers the clip and its play length, tells the presentation.
    fn rex_play(&mut self, id: u32, speed: f32) {
        self.rex.anim = id;
        self.rex.anim_t = 0.0;
        self.rex.anim_len = vrex::kt_anim_frames(id) / FPS / speed.max(0.05);
        self.emit(FightEvent::Anim { actor: Actor::Rex, id, speed });
    }

    /// `fn@0x0055a020` for the current blow (or the default blow when `paf` is entered without one).
    fn rex_paf_reaction(&mut self) {
        let fwd = self.rex.forward();
        let back = (-fwd.0, -fwd.1);
        let left = (-fwd.1, fwd.0);
        let h = self.rex.hit_dir;
        let flags = if self.rex.hit_flags == 0 { combat::HIT_HEAVY } else { self.rex.hit_flags };
        let alive = self.rex.life() > 0.0;
        let Some(r) = vrex::paf_reaction(flags, alive, dot(back, h), dot(left, h), true) else { return };
        let r01 = self.rng.range(0.0, 1.0);
        let speed = vrex::paf_speed(r01);
        if r.face_attacker {
            // 0x33 turns the rex to face the attacker; its first clip carries a 5.0 m shove [C root track]
            self.rex.facing = angle_of((-h.0, -h.1));
            self.rex.shove_left = 5.0;
            self.rex.shove_speed = 5.0 / (47.0 / FPS / speed);
        }
        self.rex_play(r.anim, speed);
        self.rex.state_len = self.rex.anim_len;
        self.rex.machine.timer = 0.0;
        self.rex.hit_flags = 0;
        self.emit(FightEvent::RexPaf { anim: r.anim, sound: r.sound, recoil: r.recoil });
    }

    /// Rex clip clock: KO clip sequencing (fall -> lying loop 0x3c while the hold runs -> get-up 0x1e)
    /// and the 0x33 shove.
    fn rex_anim_tick(&mut self, st: KtState) {
        let dt = TICK;
        self.rex.anim_t += dt;
        // knock-back (KT_TRACK_tagon): v = lerp(impulse, 0, blend), yaw += (1-blend)*kick*3*dt,
        // blend -> 1 at rate 3/s; total slide 8/3 m [C]
        if self.rex.kb_blend < 0.9 && !matches!(st, KtState::Grabbed | KtState::Finish | KtState::Projectile | KtState::Mort) {
            let k = 1.0 - self.rex.kb_blend;
            let v = self.rex.kb_vel;
            self.rex.pos = clamp_arena(add_scaled(self.rex.pos, v, k * dt), REX_RADIUS);
            self.rex.facing += k * self.rex.kb_spin * 3.0 * dt;
            let a = (3.0 * dt).clamp(0.0, 1.0);
            self.rex.kb_blend = (1.0 - a) * self.rex.kb_blend + a;
        } else if self.rex.kb_blend < 1.0 {
            self.rex.kb_blend = 1.0;
        }
        if self.rex.shove_left > 0.0 {
            let step = (self.rex.shove_speed * dt).min(self.rex.shove_left);
            self.rex.shove_left -= step;
            let h = self.rex.hit_dir;
            self.rex.pos = clamp_arena(add_scaled(self.rex.pos, h, step), REX_RADIUS);
        }
        if st != KtState::KoAuSol {
            self.rex.ko_phase = 0;
            return;
        }
        match self.rex.ko_phase {
            1 => {
                // the hold clock (`Rex+0x2d8`) only runs during the lying loop [C]
                self.rex.machine.ko_left += dt;
                if self.rex.anim_t >= self.rex.anim_len {
                    self.rex_play(vrex::KO_LIE_ANIM, 1.0);
                    self.rex.ko_phase = 2;
                }
            }
            2 => {
                if self.rex.machine.ko_left <= 0.0 {
                    self.rex_play(vrex::KO_GETUP_ANIM, 1.0);
                    self.rex.ko_phase = 3;
                }
            }
            3 => {
                // when the get-up ends the rex leaves KO whatever the hold says (case 0x1e) [C]
                self.rex.machine.ko_left = self.rex.machine.ko_left.min(0.0);
            }
            _ => {}
        }
    }

    fn on_rex_enter(&mut self, from: KtState, to: KtState) {
        self.emit(FightEvent::RexState { from, to });
        if from == KtState::KoAuSol {
            self.emit(FightEvent::KoEnd);
            self.rex.recover = self.rex.recover.max(0.8);
        }
        let mut len_s = 0.0;
        match to {
            KtState::Attaque => {
                let kind = match self.rex.machine.last_attack {
                    Some(AttackKind::Tail) => RexMove::Tail,
                    Some(AttackKind::Bite) => RexMove::Bite,
                    _ => RexMove::Sweep,
                };
                // [G] timings: (duration, blow moment)
                let (dur, hit_t) = match kind {
                    RexMove::Sweep => (1.4, 0.55),
                    RexMove::Tail => (1.4, 0.6),
                    RexMove::Bite => (1.8, 0.7),
                    RexMove::Charge => (1.0, 0.5),
                };
                self.rex.attack = Some(RexAttack { kind, dur, hit_t, t: 0.0, hit_done: false });
                self.rex.last_move = Some(kind);
                self.rex.attack_cd = dur + self.rng.range(1.2, 2.6);
                len_s = dur;
                let anim = match kind {
                    RexMove::Sweep => REX_ANIM_SWEEP,
                    RexMove::Tail => REX_ANIM_TAIL,
                    _ => REX_ANIM_BITE,
                };
                self.emit(FightEvent::RexAttack { kind });
                self.rex_play(anim, 1.0);
            }
            KtState::Charge => {
                self.rex.charge_dir = norm(sub(self.kong.pos, self.rex.pos));
                self.rex.charge_speed = REX_CHARGE_SPEED;
                self.rex.charge_travel = 0.0;
                self.rex.charge_hit_done = false;
                self.rex.charge_cd = 6.0 + self.rng.range(0.0, 3.0);
                self.rex.last_move = Some(RexMove::Charge);
                len_s = 4.0;
                self.emit(FightEvent::RexAttack { kind: RexMove::Charge });
            }
            KtState::Derap => {
                if from == KtState::Projectile {
                    self.rex.charge_speed = 0.0;
                    self.rex.recover = self.rex.recover.max(1.0);
                }
                len_s = 1.6;
            }
            KtState::Cri => {
                // roar clip (`fn@0x00556040`): 0x24 at Kong, 0x6e when wounded [C ids, L branch choice]
                let id = if self.rex.wounded() { 0x6e } else { 0x24 };
                self.rex_play(id, 1.0);
                len_s = self.rex.anim_len;
                self.emit(FightEvent::RexRoar);
            }
            KtState::Paf => {
                len_s = 0.7;
                self.rex.attack = None;
            }
            KtState::KoAuSol => {
                let hold = self.rex.machine.ko_left;
                len_s = hold + 0.8;
                self.rex.attack = None;
                let left = (-self.rex.forward().1, self.rex.forward().0);
                let fall = vrex::ko_fall_anim(from, self.rex.anim, self.rex.knocked, dot(left, self.rex.hit_dir));
                self.rex.knocked = false;
                self.rex_play(fall, 1.0);
                self.rex.ko_phase = match fall {
                    vrex::KO_LIE_ANIM => 2,
                    vrex::KO_GETUP_ANIM => 3,
                    _ => 1,
                };
                self.emit(FightEvent::KoStart { hold });
            }
            KtState::Grabbed => {
                self.rex.attack = None;
            }
            KtState::Projectile => {
                len_s = 4.0;
            }
            KtState::Finish => {
                self.rex_play(REX_ANIM_FINISH, 1.0);
            }
            KtState::Mort => {
                // after the jaw-break the rex is already in 0x38; any other death plays 0x15 -> 0x1c [C]
                if from != KtState::Finish {
                    self.rex_play(vrex::MORT_ANIMS[0], 1.0);
                }
                self.emit(FightEvent::RexDied);
            }
            KtState::FightKong => {
                self.rex.attack = None;
                if from == KtState::Grabbed {
                    self.rex.recover = self.rex.recover.max(0.8);
                }
            }
            _ => {}
        }
        self.rex.state_len = len_s;
        // leaving a grab move normally: slam damage etc. already applied
        if from == KtState::Grabbed && to == KtState::FightKong {
            self.emit(FightEvent::GrabBrokeFree);
        }
    }

    fn rex_behave(&mut self, st: KtState, to_kong: V2) {
        let dt = TICK;
        match st {
            KtState::FightKong | KtState::Cri | KtState::Attente => {
                self.rex.facing = turn_toward(self.rex.facing, angle_of(to_kong), 2.5 * dt);
                if st == KtState::FightKong && self.rex.recover <= 0.0 && self.dist() > 7.0 {
                    let f = self.rex.forward();
                    self.rex.pos = add_scaled(self.rex.pos, f, REX_WALK_SPEED * dt);
                }
            }
            KtState::Attaque => self.rex_attack_tick(to_kong),
            KtState::Charge => self.rex_charge_tick(),
            KtState::Derap => {
                // skid: decelerate along the charge direction [G]
                self.rex.charge_speed = (self.rex.charge_speed - 10.0 * dt).max(0.0);
                let d = self.rex.charge_dir;
                self.rex.pos = add_scaled(self.rex.pos, d, self.rex.charge_speed * dt);
                if len(self.rex.pos) >= ARENA_RADIUS - REX_RADIUS && self.rex.charge_speed > 0.5 {
                    self.rex.charge_blocked = true;
                }
            }
            KtState::Paf => {}
            KtState::Projectile => {
                if let Some(mut f) = self.rex.fly {
                    let step = THROW_SPEED * dt;
                    self.rex.pos = add_scaled(self.rex.pos, f.dir, step);
                    f.left -= step;
                    let wall = len(self.rex.pos) >= ARENA_RADIUS - REX_RADIUS;
                    if f.left <= 0.0 || wall {
                        self.rex.pos = clamp_arena(self.rex.pos, REX_RADIUS);
                        self.rex.fly = None;
                        self.rex.charge_blocked = true;
                        self.rex.charge_dir = f.dir;
                        let d = vrex::IMPACT_DAMAGE;
                        self.emit(FightEvent::ThrowImpact { damage: d });
                        self.shake(0.075, 1.1);
                    } else {
                        self.rex.fly = Some(f);
                    }
                }
            }
            _ => {}
        }
        self.rex.pos = clamp_arena(self.rex.pos, REX_RADIUS);
    }

    fn rex_attack_tick(&mut self, to_kong: V2) {
        let mut a = match self.rex.attack {
            Some(a) => a,
            None => return,
        };
        a.t += TICK;
        // turn toward Kong during the wind-up (first 0.35 s) [G]
        if a.t < 0.35 && a.kind != RexMove::Tail {
            self.rex.facing = turn_toward(self.rex.facing, angle_of(to_kong), 4.0 * TICK);
        }
        // jaw lunge: moves forward [G]
        if a.kind == RexMove::Bite && a.t > 0.25 && a.t < 0.65 && self.dist() > 6.0 {
            let f = self.rex.forward();
            self.rex.pos = add_scaled(self.rex.pos, f, 9.0 * TICK);
        }
        if !a.hit_done && a.t >= a.hit_t {
            a.hit_done = true;
            let dist = self.dist();
            let facing = dot(self.rex.forward(), to_kong);
            let reach_ok = match a.kind {
                RexMove::Sweep => dist <= 9.0 && facing > 0.5,
                RexMove::Tail => dist <= 9.0,
                RexMove::Bite => dist <= 8.0 && facing > 0.5,
                RexMove::Charge => false,
            };
            if reach_ok {
                let (fl, dmg) = a.kind.packet();
                self.kong_receive(a.kind, fl, dmg);
            }
        }
        self.rex.attack = Some(a);
    }

    fn rex_charge_tick(&mut self) {
        let d = self.rex.charge_dir;
        let step = self.rex.charge_speed * TICK;
        self.rex.pos = add_scaled(self.rex.pos, d, step);
        self.rex.charge_travel += step;
        self.rex.facing = angle_of(d);
        let to_k = sub(self.kong.pos, self.rex.pos);
        let dist = len(to_k);
        // contact: the charge packet reaches Kong once [C packet, G radius]
        if !self.rex.charge_hit_done && dist <= 5.5 {
            self.rex.charge_hit_done = true;
            let (fl, dmg) = RexMove::Charge.packet();
            self.kong_receive(RexMove::Charge, fl, dmg);
        }
        let passed = dot(d, to_k) < 0.0;
        if len(self.rex.pos) >= ARENA_RADIUS - REX_RADIUS - 0.2 {
            self.rex.charge_blocked = true; // into the wall: 20 damage [C]
        } else if (passed && dist > 9.0) || self.rex.charge_travel > REX_CHARGE_MAX_LEN {
            self.rex.charge_stopped = true;
        }
    }
}

// ---------------------------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn run(f: &mut Fight, secs: f32, inp: &KongInput) -> Vec<FightEvent> {
        let mut out = Vec::new();
        let n = (secs / TICK) as usize;
        let mut first = true;
        for _ in 0..n {
            let mut i = *inp;
            if !first {
                for b in i.buttons.iter_mut() {
                    b.pressed = false;
                }
            }
            first = false;
            out.extend(f.step(TICK, &i));
        }
        out
    }

    fn near(f: &mut Fight) {
        f.kong.pos = (0.0, 0.0);
        f.rex.pos = (7.0, 0.0);
        f.rex.facing = std::f32::consts::PI;
        f.rex.attack_cd = 100.0;
        f.rex.charge_cd = 100.0;
    }

    #[test]
    fn punch_chain_hits_for_10_and_wounded_rex_ignores_light_blows() {
        let mut f = Fight::new(1);
        near(&mut f);
        let mut i = KongInput::default();
        i.press(SLOT_ATTACK);
        let ev = run(&mut f, 1.0, &i);
        let hit = ev.iter().find_map(|e| match e {
            FightEvent::Hit { attacker: Actor::Kong, class, damage, applied, .. } => Some((*class, *damage, *applied)),
            _ => None,
        });
        assert_eq!(hit, Some((1, 10, 10.0)));
        assert_eq!(f.rex.life(), REX_MAX_LIFE - 10.0);
        // wounded: class-1 blows do no damage
        let mut g = Fight::new(1);
        near(&mut g);
        g.rex.machine.life.cur = 20.0;
        let ev = run(&mut g, 1.0, &i);
        assert!(ev.iter().any(|e| matches!(e, FightEvent::Hit { applied, .. } if *applied == 0.0)));
        assert_eq!(g.rex.life(), 20.0);
    }

    #[test]
    fn side_step_is_invulnerable_to_a_sweep() {
        let mut f = Fight::new(2);
        near(&mut f);
        f.rex.attack_cd = 0.0;
        f.rex.machine.entry_delay = 0.0;
        let mut ev = run(&mut f, 0.12, &KongInput::default());
        let mut i = KongInput::default();
        i.press(SLOT_JUMP_ROLL);
        ev.extend(run(&mut f, 1.6, &i));
        assert!(ev.iter().any(|e| matches!(e, FightEvent::RexAttack { .. })));
        assert!(ev.iter().any(|e| matches!(e, FightEvent::HitAvoided { by_anim, .. } if *by_anim == 6 || *by_anim == 7)));
        assert_eq!(f.kong.life, KONG_MAX_LIFE);
    }

    fn calm(f: &mut Fight) {
        f.rex.attack_cd = 1e9;
        f.rex.charge_cd = 1e9;
    }

    #[test]
    fn fury_lasts_18s_then_ends_and_blows_extend_it() {
        let mut f = Fight::new(3);
        near(&mut f);
        calm(&mut f);
        f.rex.pos = (25.0, 0.0);
        f.kong.fury.start();
        f.fury_was_active = true;
        // no blows: runs down at 1 s per second and ends at 18 s
        let ev = run(&mut f, 17.9, &KongInput::default());
        assert!(f.kong.fury.is_active() && !ev.iter().any(|e| matches!(e, FightEvent::FuryEnd)));
        let ev = run(&mut f, 0.3, &KongInput::default());
        assert!(ev.iter().any(|e| matches!(e, FightEvent::FuryEnd)));
        assert!(!f.kong.fury.is_active());
        // a landed blow brings 18.0 down to the 15.0 cap (+1.5 then clamp)
        let mut g = Fight::new(3);
        near(&mut g);
        calm(&mut g);
        g.kong.fury.start();
        g.fury_was_active = true;
        let mut i = KongInput::default();
        i.press(SLOT_ATTACK);
        let ev = run(&mut g, 1.0, &i);
        assert!(ev.iter().any(|e| matches!(e, FightEvent::FuryExtend { timer } if *timer <= 15.0)));
        // fury blows: 20 damage, class 2, 1.25x animation speed
        assert!(ev.iter().any(|e| matches!(e, FightEvent::Hit { attacker: Actor::Kong, class: 2, damage: 20, .. })));
        assert!(ev.iter().any(|e| matches!(e, FightEvent::Anim { actor: Actor::Kong, speed, .. } if (*speed - 1.25).abs() < 1e-6)));
    }

    #[test]
    fn taking_a_hit_in_fury_cuts_the_timer_and_fury_prevents_dying() {
        let mut f = Fight::new(4);
        near(&mut f);
        calm(&mut f);
        f.kong.fury.start();
        f.kong.fury.timer = 12.0;
        f.kong_receive(RexMove::Sweep, 1, 10); // flags & 3 != 0: -5.0 s (only flags without bits 1/2 cost 2.5)
        assert!((f.kong.fury.timer - 7.0).abs() < 1e-4);
        f.kong.mode = KongMode::Main;
        f.kong.latched = Latched::default();
        f.kong.anim = combat::ANIM_IDLE;
        f.kong_receive(RexMove::Bite, 2, 10); // -5.0 s again (timer 7.0 > 5.0)
        assert!((f.kong.fury.timer - 2.0).abs() < 1e-4);
        // life 0 in fury -> 10.0, not dead
        f.kong.fury.timer = 10.0;
        f.kong.life = 0.0;
        f.kong.mode = KongMode::Main;
        run(&mut f, 0.05, &KongInput::default());
        assert_eq!(f.kong.life, 10.0);
        assert_ne!(f.kong.mode, KongMode::Dead);
    }

    #[test]
    fn last_stand_without_fury_kills_on_a_hit_in_the_last_5_seconds() {
        let mut f = Fight::new(5);
        near(&mut f);
        calm(&mut f);
        f.kong.life = 0.0;
        run(&mut f, 0.05, &KongInput::default());
        assert!(f.kong.last_stand_flag && f.kong.last_stand_timer > 9.0);
        // early in the last stand a hit only cuts it to 5.0
        f.kong.mode = KongMode::Main;
        f.kong_receive(RexMove::Sweep, 1, 10);
        assert!(f.kong.last_stand_timer <= 5.0 + 1e-3 && f.kong.mode != KongMode::Dead);
        // below 5 s the next hit kills
        f.kong.mode = KongMode::Main;
        f.kong.anim = combat::ANIM_IDLE;
        f.kong.last_stand_timer = 3.0;
        let ev_before = f.events.len();
        f.kong_receive(RexMove::Sweep, 1, 10);
        assert_eq!(f.kong.mode, KongMode::Dead);
        assert!(f.events[ev_before..].iter().any(|e| matches!(e, FightEvent::KongDied)));
    }

    fn knocked_out(f: &mut Fight) {
        near(f);
        calm(f);
        f.rex.machine.life.cur = 0.0;
        run(f, 0.1, &KongInput::default());
        assert_eq!(f.rex.state(), KtState::KoAuSol);
    }

    #[test]
    fn knock_down_holds_4_75_s_and_gets_up_with_20_life() {
        let mut f = Fight::new(6);
        near(&mut f);
        calm(&mut f);
        f.rex.machine.life.cur = 0.0;
        let ev = run(&mut f, 0.1, &KongInput::default());
        let hold = ev.iter().find_map(|e| match e { FightEvent::KoStart { hold } => Some(*hold), _ => None }).unwrap();
        assert!((hold - 4.75).abs() < 1e-4);
        // fall 0x16 (129 f) + lying 0x3c for the hold + get-up 0x1e (93 f): still down after 6 s
        let ev = run(&mut f, 6.0, &KongInput::default());
        assert!(!ev.iter().any(|e| matches!(e, FightEvent::KoEnd)));
        assert!(ev.iter().any(|e| matches!(e, FightEvent::Anim { actor: Actor::Rex, id: 0x3c, .. })));
        let ev = run(&mut f, 3.0, &KongInput::default());
        assert!(ev.iter().any(|e| matches!(e, FightEvent::KoEnd)));
        assert_eq!(f.rex.life(), 20.0);
        assert_eq!(f.rex.state(), KtState::FightKong);
    }

    #[test]
    fn blow_plays_the_paf_reaction_and_a_new_blow_restarts_it() {
        let mut f = Fight::new(3);
        near(&mut f);
        calm(&mut f);
        // rex faces Kong: the blow travels against the rex's forward -> "front" clip
        f.rex.facing = angle_of(sub(f.kong.pos, f.rex.pos));
        f.deliver_to_rex(combat::HIT_HEAVY, 5, 0x10, true);
        let ev = run(&mut f, TICK * 2.0, &KongInput::default());
        assert_eq!(f.rex.state(), KtState::Paf);
        assert!(ev.iter().any(|e| matches!(e, FightEvent::RexPaf { anim: 0x67, sound: 4, .. })), "{ev:?}");
        run(&mut f, 0.2, &KongInput::default());
        let t_before = f.rex.machine.timer;
        f.deliver_to_rex(combat::HIT_LIGHT, 5, 0x10, true);
        let ev = run(&mut f, TICK * 2.0, &KongInput::default());
        assert!(ev.iter().any(|e| matches!(e, FightEvent::RexPaf { anim: 0x6b, sound: 3, .. })), "{ev:?}");
        assert!(f.rex.machine.timer < t_before);
    }

    #[test]
    fn jaw_break_snaps_kong_onto_the_rex_and_kills_at_frame_100() {
        let mut f = Fight::new(9);
        knocked_out(&mut f);
        f.rex.facing = 0.7;
        f.start_finisher();
        let mut i = KongInput::default();
        i.buttons[SLOT_ATTACK].pressed = true;
        i.buttons[SLOT_ATTACK].held = true;
        let mut won = false;
        let mut died_at = None;
        for n in 0..(20.0 / TICK) as usize {
            let mut j = i;
            j.buttons[SLOT_ATTACK].pressed = n % 2 == 0;
            let ev = f.step(TICK, &j);
            if f.rex.state() == KtState::Finish && n > 40 {
                assert!(len(sub(f.kong.pos, f.rex.pos)) < 1e-3);
                assert!((f.kong.facing - f.rex.facing).abs() < 1e-3);
            }
            if ev.iter().any(|e| matches!(e, FightEvent::FinisherSuccess)) {
                won = true;
                assert!(ev.iter().any(|e| matches!(e, FightEvent::Anim { actor: Actor::Rex, id: 0x38, .. })));
            }
            if died_at.is_none() && ev.iter().any(|e| matches!(e, FightEvent::RexDied)) {
                died_at = Some(n);
            }
            if ev.iter().any(|e| matches!(e, FightEvent::VictoryPound)) {
                assert!(died_at.is_some());
                break;
            }
        }
        assert!(won && died_at.is_some());
    }

    #[test]
    fn after_the_victory_kong_is_free_to_move_again() {
        let mut f = Fight::new(9);
        knocked_out(&mut f);
        f.start_finisher();
        let mut n = 0usize;
        while !f.is_finished() && n < (40.0 / TICK) as usize {
            let mut j = KongInput::default();
            j.buttons[SLOT_ATTACK].held = true;
            j.buttons[SLOT_ATTACK].pressed = n % 2 == 0;
            f.step(TICK, &j);
            n += 1;
        }
        assert!(f.is_finished() && f.over == Some(Actor::Kong));
        assert_eq!(f.kong.mode, KongMode::Main);
        // let a swing latched during the mash play out, then the stick walks him away from the corpse
        for _ in 0..(2.0 / TICK) as usize {
            f.step(TICK, &KongInput::default());
        }
        let p0 = f.kong.pos;
        let away = norm(sub(f.kong.pos, f.rex.pos));
        let mut walk = KongInput::default();
        walk.stick = away;
        for _ in 0..(1.0 / TICK) as usize {
            f.step(TICK, &walk);
        }
        assert!(len(sub(f.kong.pos, p0)) > 2.0, "Kong stayed in the victory pose: {:?} -> {:?} phase {:?}", p0, f.kong.pos, f.kong.phase);
        assert!(f.is_finished());
    }

    #[test]
    fn finisher_needs_ko_wounded_first_knockdown_and_22_m() {
        let attack = {
            let mut i = KongInput::default();
            i.press(SLOT_ATTACK);
            i
        };
        // rex standing: no finisher
        let mut f = Fight::new(7);
        near(&mut f);
        calm(&mut f);
        let ev = run(&mut f, 0.3, &attack);
        assert!(!ev.iter().any(|e| matches!(e, FightEvent::FinisherStart)));
        // KO but Kong 25 m away: the 22 radius refuses
        let mut f = Fight::new(7);
        knocked_out(&mut f);
        f.kong.pos = (-23.0, 0.0);
        f.rex.pos = (2.0, 0.0);
        assert_eq!(f.proposal(), None);
        // within 22: accepted
        f.kong.pos = (-18.0, 0.0);
        assert_eq!(f.proposal(), Some(Proposal::Finish));
        let ev = run(&mut f, 0.3, &attack);
        assert!(ev.iter().any(|e| matches!(e, FightEvent::FinisherStart)));
        assert_eq!(f.rex.state(), KtState::Finish);
        // second knock-down (the window flag is gone): only a plain grab is offered
        let mut g = Fight::new(7);
        knocked_out(&mut g);
        g.rex.machine.knocked_down_before = true;
        assert_eq!(g.proposal(), Some(Proposal::Grab));
    }

    #[test]
    fn finisher_mash_wins_and_idling_loses() {
        // mashing: success -> rex dies, Kong celebrates
        let mut f = Fight::new(8);
        knocked_out(&mut f);
        let mut a = KongInput::default();
        a.press(SLOT_ATTACK);
        run(&mut f, 0.1, &a);
        let mut ev = Vec::new();
        for n in 0..(14.0 / TICK) as usize {
            let mut i = KongInput::default();
            if n % 6 == 0 {
                i.press(if (n / 6) % 2 == 0 { SLOT_ATTACK } else { SLOT_SPECIAL });
            }
            ev.extend(f.step(TICK, &i));
            if f.is_finished() {
                break;
            }
        }
        assert!(ev.iter().any(|e| matches!(e, FightEvent::FinisherSuccess)));
        assert!(ev.iter().any(|e| matches!(e, FightEvent::RexDied)));
        assert!(ev.iter().any(|e| matches!(e, FightEvent::VictoryRoar)));
        assert!(matches!(f.over, Some(Actor::Kong)));
        // idle: the rex shakes the jaws free after the 2.74 s lock, Kong is thrown back, life floor 20
        let mut g = Fight::new(8);
        knocked_out(&mut g);
        run(&mut g, 0.1, &a);
        let ev = run(&mut g, 8.0, &KongInput::default());
        assert!(ev.iter().any(|e| matches!(e, FightEvent::FinisherEscape)));
        assert!(!ev.iter().any(|e| matches!(e, FightEvent::FinisherSuccess)));
        assert_ne!(g.rex.state(), KtState::Mort);
        assert!(g.rex.life() >= vrex::KO_EXIT_LIFE_FLOOR);
    }

    #[test]
    fn grab_strike_then_throw_impact_20_and_hold_timeout() {
        let mut f = Fight::new(9);
        near(&mut f);
        calm(&mut f);
        f.rex.machine.state = KtState::Derap; // skidding rex: grab offered
        f.rex.state_len = 100.0;
        f.rex.charge_speed = 0.0;
        assert_eq!(f.proposal(), Some(Proposal::Grab));
        let mut a = KongInput::default();
        a.press(SLOT_ATTACK);
        let mut ev = run(&mut f, 0.5, &a);
        assert_eq!(f.kong.mode, KongMode::Grab);
        assert_eq!(f.rex.state(), KtState::Grabbed);
        ev.extend(run(&mut f, TICK, &a)); // strike press while holding
        ev.extend(run(&mut f, 1.0, &KongInput::default()));
        let life_after_strike = f.rex.life();
        assert_eq!(life_after_strike, REX_MAX_LIFE - 10.0);
        assert!(ev.iter().any(|e| matches!(e, FightEvent::Hit { class: 9, damage: 10, anim: 0xa2, .. })));
        let mut t = KongInput::default();
        t.press(SLOT_SPECIAL);
        let mut ev = run(&mut f, 0.02, &t);
        ev.extend(run(&mut f, 2.5, &KongInput::default()));
        assert!(ev.iter().any(|e| matches!(e, FightEvent::Throw { .. })));
        assert!(ev.iter().any(|e| matches!(e, FightEvent::ThrowImpact { damage } if *damage == 20.0)));
        assert_eq!(f.rex.life(), life_after_strike - 20.0);
        // a hold nobody uses breaks free after 1.0 s (5.0 s when wounded)
        let mut g = Fight::new(9);
        near(&mut g);
        calm(&mut g);
        g.rex.machine.state = KtState::Cri;
        g.rex.state_len = 100.0;
        run(&mut g, 0.5, &a);
        assert_eq!(g.kong.mode, KongMode::Grab);
        let ev = run(&mut g, 1.6, &KongInput::default());
        assert!(ev.iter().any(|e| matches!(e, FightEvent::GrabBrokeFree)));
        assert_eq!(g.kong.mode, KongMode::Main);
    }

    #[test]
    fn charge_into_the_wall_costs_the_rex_20_and_a_dodged_charge_misses() {
        let mut f = Fight::new(10);
        f.kong.pos = (31.0, 0.0);
        f.rex.pos = (0.0, 0.0);
        f.rex.facing = 0.0;
        f.rex.machine.state = KtState::Charge;
        f.rex.charge_dir = (1.0, 0.0);
        f.rex.charge_speed = REX_CHARGE_SPEED;
        f.rex.charge_cd = 1e9;
        f.rex.state_len = 4.0;
        let ev = run(&mut f, 3.0, &KongInput::default());
        assert_eq!(f.rex.life(), REX_MAX_LIFE - 20.0);
        assert!(ev.iter().any(|e| matches!(e, FightEvent::RexState { from: KtState::Charge, to: KtState::Paf })));
        // dodge: Kong 14 m in front, side-step as it arrives -> avoided
        let mut g = Fight::new(10);
        g.kong.pos = (14.0, 0.0);
        g.kong.facing = std::f32::consts::PI;
        g.rex.pos = (0.0, 0.0);
        g.rex.facing = 0.0;
        g.rex.machine.state = KtState::Charge;
        g.rex.charge_dir = (1.0, 0.0);
        g.rex.charge_speed = REX_CHARGE_SPEED;
        g.rex.charge_cd = 1e9;
        g.rex.state_len = 4.0;
        let mut d = KongInput::default();
        d.stick = (-1.0, 0.0);
        d.press(SLOT_JUMP_ROLL);
        let ev = run(&mut g, 2.0, &d);
        assert!(ev.iter().any(|e| matches!(e, FightEvent::HitAvoided { attack: RexMove::Charge, .. })));
        assert_eq!(g.kong.life, KONG_MAX_LIFE);
        // standing still takes the 0x42 / 10 packet
        let mut h = Fight::new(10);
        h.kong.pos = (14.0, 0.0);
        h.rex.pos = (0.0, 0.0);
        h.rex.machine.state = KtState::Charge;
        h.rex.charge_dir = (1.0, 0.0);
        h.rex.charge_speed = REX_CHARGE_SPEED;
        h.rex.charge_cd = 1e9;
        h.rex.state_len = 4.0;
        let ev = run(&mut h, 2.0, &KongInput::default());
        assert!(ev.iter().any(|e| matches!(e, FightEvent::Hit { attacker: Actor::Rex, victim: Actor::Kong, class: 0x42, damage: 10, .. })));
    }


    #[test]
    fn fury_pattern_special_special_hold() {
        let mut f = Fight::new(3);
        near(&mut f);
        f.rex.pos = (25.0, 0.0);
        let mut i = KongInput::default();
        i.press(SLOT_SPECIAL);
        let mut ev = run(&mut f, 0.45, &i); // repel running
        let mut j = KongInput::default();
        j.press(SLOT_SPECIAL);
        ev.extend(run(&mut f, 0.05, &j));
        let mut h = KongInput::default();
        h.hold(SLOT_SPECIAL);
        ev.extend(run(&mut f, 3.5, &h));
        assert!(ev.iter().any(|e| matches!(e, FightEvent::PoundStart)));
        assert!(ev.iter().any(|e| matches!(e, FightEvent::FuryStart { timer } if *timer == 18.0)));
        assert!(f.kong.fury.is_active());
    }

    #[test]
    fn finisher_pull_value_is_the_recovered_clip_ramp() {
        assert_eq!(anim_info(ANIM_FINISH_LOCK).len, 20.0);
        assert_eq!(anim_info(ANIM_FINISH_MASH).len, 346.0);
        assert_eq!(anim_info(ANIM_FINISH_WIN).len, 214.0);
        assert_eq!(finish_pull_value(0.1), 0.0);
        // 20 frames of lock + 102 frames of nothing, then the ramp up to 1 at frame 346
        assert_eq!(finish_pull_value((20.0 + 101.0) / FPS), 0.0);
        assert!((finish_pull_value((20.0 + 224.0) / FPS) - 0.5).abs() < 1e-3);
        assert!(finish_pull_value((20.0 + 345.9) / FPS) > 0.99);
        // the mash clip restarts after it ends
        assert_eq!(finish_pull_value((20.0 + 346.0 + 50.0) / FPS), 0.0);
    }
}
