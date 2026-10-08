//! Jack: movement and camera constants (ledger J01–J08, C01–C06).
//!
//! From `H_exec_select_action@0x587bd0`, `CM_Cam@0x45b1b0`, `sub_43cb00` (stick normalise).
//! The movement/camera *logic* still lives in `kk_fps::player`; porting the full functions
//! here is the J-group task in the ledger.

/// Target speeds, m/s [C] (`H_exec_select_action`: crouch 1.5, run 4.5, walk 2.5).
pub const WALK_SPEED: f32 = 2.5;
pub const RUN_SPEED: f32 = 4.5;
pub const CROUCH_SPEED: f32 = 1.5;
/// Speed cap while aiming (+0xb94) [C].
pub const AIM_SPEED_CAP: f32 = 0.5;
/// speed = lerp(target, prev, 4*dt): accel and decel share one ~0.25 s time constant [C].
pub const SPEED_SMOOTH_RATE: f32 = 4.0;
/// Eye heights, m [C] (`CM_Cam`: 1.6 standing, 0.8 crouched; also 1.45 / 1.4 flag variants).
pub const EYE_STAND: f32 = 1.6;
pub const EYE_CROUCH: f32 = 0.8;
pub const EYE_SMOOTH_RATE: f32 = 5.0;
/// Forward vector z clamped to ±0.95 → ±71.8° pitch [C].
pub const PITCH_LIMIT_SIN: f32 = 0.95;
/// Stick deadzone (`sub_43cb00`, mode 3): (|v| − 0.15) / 0.85 [C].
pub const STICK_DEADZONE: f32 = 0.15;
/// Look factors (yaw, pitch) [C]; the ×10 rad/s scale is [L].
pub const LOOK_NORMAL: (f32, f32) = (2.0, 1.75);
pub const LOOK_RUN: (f32, f32) = (3.0, 1.75);
pub const LOOK_AIM: (f32, f32) = (0.6, 0.35);
pub const LOOK_LOCKON: (f32, f32) = (1.5, 1.31);
/// FOV: default 1.2, aim 0.6, sniper 0.3 [L units].
pub const FOV_DEFAULT: f32 = 1.2;
pub const FOV_AIM: f32 = 0.6;
pub const FOV_SNIPER: f32 = 0.3;

/// Stick normalisation `sub_43cb00` mode 3: clamp to [−1, 1], apply the 0.15 deadzone and
/// rescale so full deflection is still 1.0 [C].
pub fn normalize_stick(v: f32) -> f32 {
    let v = v.clamp(-1.0, 1.0);
    let a = v.abs();
    if a <= STICK_DEADZONE {
        0.0
    } else {
        ((a - STICK_DEADZONE) / (1.0 - STICK_DEADZONE)).copysign(v)
    }
}

/// One frame of the speed smoothing in `H_exec_select_action` [C].
pub fn smooth_speed(prev: f32, target: f32, dt: f32) -> f32 {
    let k = (SPEED_SMOOTH_RATE * dt).min(1.0);
    prev + (target - prev) * k
}

// ---------------------------------------------------------------------------------------------
// Movement: H_exec_read_joy@0x579f30 (flags), GG_Exec_Joy@0x695030 (stick -> world direction),
// H_exec_select_action@0x587bd0 (target speed, smoothing, velocity).
// ---------------------------------------------------------------------------------------------

/// Radial threshold under which GG_Exec_Joy reports "no movement" (|v| < 0.25) [C GG_Exec_Joy@0x695030].
pub const MOVE_STICK_MIN: f32 = 0.25;
/// GG_Exec_Joy rescales |v| as clamp((|v| - 0.25) / 0.675, 0, 1) into the joy-norm variable [C].
pub const MOVE_STICK_SPAN: f32 = 0.675;
/// Wading: at water depth >= 0.6 the normal speed table is replaced; >= 1.1 is "deep" [C select_action].
pub const WADE_DEPTH_SHALLOW: f32 = 0.6;
pub const WADE_DEPTH_DEEP: f32 = 1.1;
/// Run multiplier on the stick magnitude [C].
pub const SPEED_RUN_MUL: f32 = 4.5;
/// Weapon-ready walk multiplier [C].
pub const SPEED_READY_MUL: f32 = 2.5;
/// Crouch multiplier [C].
pub const SPEED_CROUCH_MUL: f32 = 1.5;
/// Caps applied after the base speed: script mode 4 -> 2.0, mode 2 -> 3.5, pad-flag -> 3.0 [C].
pub const CAP_MODE4: f32 = 2.0;
pub const CAP_MODE2: f32 = 3.5;
pub const CAP_FLAG40: f32 = 3.0;
/// Target speed while the aim flag (+0xb94) is set (assignment, not a cap) [C].
pub const SPEED_AIM: f32 = 0.5;
/// Target speed while the slow flag (+0x1a30) is set; overrides the aim value [C].
pub const SPEED_SLOW_FLAG: f32 = 1.5;
/// After leaving "run" for weapon-ready, a 0.1 s ramp holds a stick-independent speed
/// `t*2/0.1 + 2.5` (4.5 -> 2.5 m/s) [C read_joy@0x579f30 line 637, select_action line 1124].
pub const READY_RAMP_TIME: f32 = 0.1;
/// Grace time (s) set on weapon swap (+0xc0c): while > 0 the run speed is used [C values 0.5 / 0.6,
/// semantic L].
pub const PANIC_RUN_TIME_A: f32 = 0.5;
pub const PANIC_RUN_TIME_B: f32 = 0.6;
/// Magnitude of the deadzoned stick vector: sqrt(x^2 + y^2) (z is 0) [C fn@0x00405ad0 on fn@0x0043cb00].
/// Note it is NOT clamped to 1: a full diagonal is sqrt(2).
pub fn stick_magnitude(x: f32, y: f32) -> f32 {
    let (x, y) = (normalize_stick(x), normalize_stick(y));
    (x * x + y * y).sqrt()
}

/// Result of `GG_Exec_Joy@0x695030`: world direction (horizontal, unit) and the joy-norm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveDir {
    /// Unit vector in the world XY plane (Z is the up axis).
    pub dir: [f32; 2],
    /// clamp((|v|-0.25)/0.675, 0, 1), snapped to 1.0 above 0.99.
    pub joy_norm: f32,
}

/// Stick -> world direction as done by GG_Exec_Joy [C structure, L axis conventions].
///
/// `a0`,`a1` are the two components returned by the engine pad layer (fn@0x00a6be80) after
/// `fn@0x0043cb00` (per-axis 0.15 deadzone). The code negates them (v = -s), rotates the
/// vector by the camera object's matrix (`fn@0x0041ccc0`), zeroes Z, and normalises.
/// `cam_x` / `cam_y` are the horizontal parts of the camera's first two local axes
/// (X is the camera forward in Jade, fallback (1,0,0)) [L].
pub fn move_direction(a0: f32, a1: f32, cam_x: [f32; 2], cam_y: [f32; 2]) -> Option<MoveDir> {
    let (n0, n1) = (normalize_stick(a0), normalize_stick(a1));
    let len = (n0 * n0 + n1 * n1).sqrt();
    if len < MOVE_STICK_MIN {
        return None;
    }
    let (v0, v1) = (-n0, -n1);
    let wx = v0 * cam_x[0] + v1 * cam_y[0];
    let wy = v0 * cam_x[1] + v1 * cam_y[1];
    let l = (wx * wx + wy * wy).sqrt();
    if l < 1e-6 {
        return None;
    }
    let mut norm = ((len - MOVE_STICK_MIN).clamp(0.0, MOVE_STICK_SPAN)) / MOVE_STICK_SPAN;
    if norm > 0.99 {
        norm = 1.0;
    }
    Some(MoveDir { dir: [wx / l, wy / l], joy_norm: norm })
}

/// `fn@0x00689ff0`: script modes that use the wading speed table [C].
pub fn is_wade_mode(mode: i32) -> bool {
    matches!(mode, 3 | 9 | 0xb | 10 | 0xc)
}

/// Per-frame inputs to the Jack speed selection (all offsets are in Jack's AI struct).
#[derive(Clone, Copy, Debug)]
pub struct MoveInput {
    /// |stick| after deadzone (see [`stick_magnitude`]); `local_48` in select_action.
    pub stick_mag: f32,
    /// Direction from [`move_direction`]; `None` = stick inactive (speed reset, velocity 0).
    pub dir: Option<MoveDir>,
    /// +0xba8 i_flag_run: set every frame by read_joy, cleared while a weapon is held "ready".
    pub run: bool,
    /// +0xb60 crouch intent (toggle / hold, see [`crouch_intent`]).
    pub crouch: bool,
    /// +0xb94 aim flag (hold aim button).
    pub aim: bool,
    /// +0xc0c (float): weapon-swap grace timer; > 0 forces run speed.
    pub panic_timer: f32,
    /// +0xb1c i_mode_script.
    pub mode: i32,
    /// +0x1004 water surface height above the feet.
    pub water_depth: f32,
    /// fn@0x0042c0d0(actor,0x40) & 0x80000000 flag -> cap 3.0.
    pub flag40: bool,
    /// +0x1a30 slow flag -> target 1.5.
    pub slow: bool,
    /// Current vertical velocity (preserved by select_action; z of fn@0x00433890).
    pub vz: f32,
}

impl Default for MoveInput {
    fn default() -> Self {
        MoveInput {
            stick_mag: 0.0,
            dir: None,
            run: true,
            crouch: false,
            aim: false,
            panic_timer: 0.0,
            mode: 0,
            water_depth: 0.0,
            flag40: false,
            slow: false,
            vz: 0.0,
        }
    }
}

/// Output of one movement step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveOutput {
    /// Velocity set through fn@0x00433790 (x, y, z); z is copied from the previous velocity.
    pub velocity: [f32; 3],
    pub speed: f32,
    pub target: f32,
}

/// Persistent movement state: smoothed speed (+0x1ab0) and the weapon-ready ramp
/// (+0xc08 / +0xbac, driven from `H_exec_read_joy`).
#[derive(Clone, Copy, Debug)]
pub struct MoveState {
    pub input: MoveInput,
    /// +0x1ab0 smoothed speed.
    pub speed: f32,
    /// +0xc08 ready-ramp timer.
    pub ready_timer: f32,
    /// +0xbac: ramp finished.
    pub ready_done: bool,
    prev_run: bool,
}

impl Default for MoveState {
    fn default() -> Self {
        MoveState { input: MoveInput::default(), speed: 0.0, ready_timer: 0.0, ready_done: true, prev_run: true }
    }
}

impl MoveState {
    /// The `i_flag_run` / ready-ramp bookkeeping at the end of `H_exec_read_joy@0x579f30`.
    fn update_ready_ramp(&mut self, dt: f32) {
        let run = self.input.run;
        if self.prev_run && !run {
            // just stopped running (weapon raised): start the 0.1 s ramp
            self.ready_timer = if self.input.panic_timer <= 0.0 { READY_RAMP_TIME } else { 1.0e14 };
            self.ready_done = false;
        } else if !self.prev_run && run {
            self.ready_done = false;
        }
        if !run && !self.ready_done {
            self.ready_timer -= dt;
            if self.ready_timer < 0.0 {
                self.ready_done = true;
            }
        }
        self.prev_run = run;
    }

    /// Target speed selection of `H_exec_select_action@0x587bd0` (player branch, moving).
    pub fn target_speed(&self) -> f32 {
        let i = &self.input;
        let l = i.stick_mag;
        let wading = is_wade_mode(i.mode) && i.water_depth >= WADE_DEPTH_SHALLOW;
        let mut t = if !wading {
            if !i.crouch {
                if i.run || i.panic_timer > 0.0 {
                    l * SPEED_RUN_MUL
                } else if !self.ready_done {
                    // stick-independent ramp 4.5 -> 2.5 over 0.1 s
                    (self.ready_timer * 2.0) / READY_RAMP_TIME + 2.5
                } else {
                    l * SPEED_READY_MUL
                }
            } else {
                l * SPEED_CROUCH_MUL
            }
        } else if i.water_depth < WADE_DEPTH_DEEP {
            if i.run { l * 1.5 } else { l * 1.0 }
        } else {
            l * 1.0
        };
        if i.mode == 4 && t > CAP_MODE4 {
            t = CAP_MODE4;
        }
        if i.mode == 2 && t > CAP_MODE2 {
            t = CAP_MODE2;
        }
        if i.flag40 && t > CAP_FLAG40 {
            t = CAP_FLAG40;
        }
        if i.aim {
            t = SPEED_AIM;
        }
        if i.slow {
            t = SPEED_SLOW_FLAG;
        }
        t
    }

    /// One frame: speed = lerp(speed, target, min(4*dt,1)); velocity = speed * dir, z preserved.
    /// With no direction (stick below 0.25) the speed is reset and the horizontal velocity is 0 [C].
    pub fn step(&mut self, dt: f32) -> MoveOutput {
        self.update_ready_ramp(dt);
        let vz = self.input.vz;
        match self.input.dir {
            None => {
                self.speed = 0.0;
                MoveOutput { velocity: [0.0, 0.0, vz], speed: 0.0, target: 0.0 }
            }
            Some(d) => {
                let target = self.target_speed();
                let k = (SPEED_SMOOTH_RATE * dt).clamp(0.0, 1.0);
                self.speed = (1.0 - k) * self.speed + target * k;
                MoveOutput {
                    velocity: [self.speed * d.dir[0], self.speed * d.dir[1], vz],
                    speed: self.speed,
                    target,
                }
            }
        }
    }
}

/// Crouch intent (+0xb60) as maintained by `H_exec_read_joy@0x579f30` [C structure].
/// * toggle scheme (fn@0x004401e0()==2): button 2 pressed flips it;
/// * hold scheme: crouch only while button 0xb is held;
/// * both gated by `grab_timer < 0.5` (+0x2d3);
/// * cleared when wading deeper than 0.6; forced on when the stand-up ray (0.8 m from
///   pos+0.8 up) hits a ceiling while already crouched in posture (+0x2da).
pub fn crouch_intent(
    prev: bool,
    toggle_scheme: bool,
    toggle_pressed: bool,
    hold_pressed: bool,
    grab_timer: f32,
    water_depth: f32,
    posture_crouched: bool,
    ceiling_blocked: bool,
) -> bool {
    let mut c = prev;
    if toggle_scheme {
        if toggle_pressed && grab_timer < 0.5 {
            c = !c;
        }
    } else {
        c = hold_pressed && grab_timer < 0.5;
    }
    if water_depth >= WADE_DEPTH_SHALLOW {
        c = false;
    }
    if posture_crouched && !c && ceiling_blocked {
        c = true;
    }
    c
}

/// Height of the stand-up ceiling probe origin above the feet and its length, m [C read_joy].
pub const CROUCH_PROBE_START: f32 = 0.8;
pub const CROUCH_PROBE_LEN: f32 = 0.8;

// ---------------------------------------------------------------------------------------------
// Camera: CM_Cam@0x45b1b0 (disassembled; Ghidra fails on this 26 KB function).
// ---------------------------------------------------------------------------------------------

/// Look stick scale: angle = -stick * 10 * dt * factor [C CM_Cam 0x45f0c8..0x45f18c].
pub const LOOK_RATE_SCALE: f32 = 10.0;
/// Values below this (after x2*sens) are snapped to 0 [C 0x45f04e].
pub const LOOK_SNAP: f32 = 0.01;
/// |stick x| above this starts the yaw acceleration ramp [C 0x45ede9].
pub const LOOK_RAMP_THRESHOLD: f32 = 0.95;
pub const LOOK_SPECIAL: (f32, f32) = (9.0, 5.25);
/// Ramp caps (seconds of build-up) per look mode [C].
pub const RAMP_NORMAL: f32 = 1.2;
pub const RAMP_RUN: f32 = 1.2;
pub const RAMP_AIM: f32 = 1.0;
pub const RAMP_LOCKON: f32 = 1.0;
pub const RAMP_SPECIAL: f32 = 3.6;
/// Unarmed (weapon id 0) aiming multiplier on both factors [C 0x45ec27].
pub const LOOK_AIM_UNARMED_MUL: f32 = 1.5;
/// sqrt(1 - 0.95^2): xy scale at the pitch limit [C 0x45f2b3].
pub const PITCH_LIMIT_XY: f32 = 0.31225;

/// Which look-factor set CM_Cam selects, in priority order [C 0x45ebb2..0x45ecf9].
#[derive(Clone, Copy, Debug, Default)]
pub struct LookMode {
    /// G+0x337c != 0
    pub special: bool,
    /// Jack +0xb94
    pub aim: bool,
    /// G+0x3348 == 0 (no weapon): aim factors x1.5
    pub unarmed: bool,
    /// Jack +0xba8 && +0x3b48 == 0
    pub run: bool,
    /// lock-on actor present (Jack +0x2d00 validated)
    pub lock_on: bool,
}

/// (yaw factor, pitch factor, ramp cap) for a look mode.
pub fn look_factors(m: LookMode) -> (f32, f32, f32) {
    if m.special {
        (LOOK_SPECIAL.0, LOOK_SPECIAL.1, RAMP_SPECIAL)
    } else if m.aim {
        let (mut a, mut b) = LOOK_AIM;
        if m.unarmed {
            a *= LOOK_AIM_UNARMED_MUL;
            b *= LOOK_AIM_UNARMED_MUL;
        }
        (a, b, RAMP_AIM)
    } else if m.run {
        (LOOK_RUN.0, LOOK_RUN.1, RAMP_RUN)
    } else if m.lock_on {
        (LOOK_LOCKON.0, LOOK_LOCKON.1, RAMP_LOCKON)
    } else {
        (LOOK_NORMAL.0, LOOK_NORMAL.1, RAMP_NORMAL)
    }
}

/// Persistent look state: the yaw ramp timer (camera struct +0xc).
#[derive(Clone, Copy, Debug, Default)]
pub struct LookState {
    pub ramp: f32,
}

/// Inputs to [`look_step`].
#[derive(Clone, Copy, Debug)]
pub struct LookInput {
    /// Raw right stick (x, y) from the pad layer (fn@0x0043cda0), no deadzone function.
    pub stick: (f32, f32),
    /// Options: CAM sensitivity (G+0x4dbc) and invert-Y (G+0x4db8). Default value unknown [gap].
    pub sensitivity: f32,
    pub invert_y: bool,
    pub mode: LookMode,
    /// A lock-on target is active and the camera is not detached (look is skipped).
    pub locked_chase: bool,
}

/// One frame of CM_Cam free look. Returns (yaw_delta, pitch_delta) in radians as passed to
/// `fn@0x00414520` (rotate about Z) and `fn@0x00413750` (rotate about the pitch axis) [C structure,
/// L radians]. Positive stick x gives negative yaw.
pub fn look_step(st: &mut LookState, inp: &LookInput, dt: f32) -> (f32, f32) {
    let (yaw_f, pitch_f, ramp_cap) = look_factors(inp.mode);
    let x = inp.stick.0.clamp(-1.0, 1.0);
    let mut y = inp.stick.1.clamp(-1.0, 1.0);
    // yaw acceleration: only while |x| > 0.95 and not aiming; factor += t^3
    let mut a = yaw_f;
    if x.abs() > LOOK_RAMP_THRESHOLD {
        if !inp.mode.aim {
            st.ramp = (st.ramp + dt).min(ramp_cap);
        } else {
            st.ramp = 0.0;
        }
        a += st.ramp * st.ramp * st.ramp;
    } else {
        st.ramp = 0.0;
    }
    if inp.locked_chase {
        return (0.0, 0.0);
    }
    if inp.invert_y {
        y = -y;
    }
    let mut xs = x * 2.0 * inp.sensitivity;
    let mut ys = y * 2.0 * inp.sensitivity;
    if xs > -LOOK_SNAP && xs < LOOK_SNAP {
        xs = 0.0;
    }
    if ys > -LOOK_SNAP && ys < LOOK_SNAP {
        ys = 0.0;
    }
    (-xs * LOOK_RATE_SCALE * dt * a, -ys * LOOK_RATE_SCALE * dt * pitch_f)
}

/// Pitch limit on the forward vector (CM_Cam 0x45f281..0x45f351): if |fz| > 0.95 the vector is
/// replaced by (xy normalised * 0.31225, +-0.95) [C].
pub fn clamp_pitch(fwd: [f32; 3]) -> [f32; 3] {
    let limit = |sign: f32| {
        let l = (fwd[0] * fwd[0] + fwd[1] * fwd[1]).sqrt();
        let (nx, ny) = if l > 0.0 { (fwd[0] / l, fwd[1] / l) } else { (1.0, 0.0) };
        [nx * PITCH_LIMIT_XY, ny * PITCH_LIMIT_XY, sign * PITCH_LIMIT_SIN]
    };
    if fwd[2] > PITCH_LIMIT_SIN {
        limit(1.0)
    } else if fwd[2] < -PITCH_LIMIT_SIN {
        limit(-1.0)
    } else {
        fwd
    }
}

/// Eye height heights, m [C CM_Cam 0x45dbe6..0x45dc3e].
pub const EYE_FLAG_C1C: f32 = 1.4;
pub const EYE_FLAG_34A0: f32 = 1.45;
/// Eye offset forward/back (y) applied with the height: (0, -0.02, h + bob) [C 0x45dd09].
pub const EYE_OFFSET_Y: f32 = -0.02;

/// Eye height target: override (cam +4) > flag +0xc1c (1.4) > crouch (0.8) > flag +0x34a0 (1.45) > 1.6.
pub fn eye_target(override_h: f32, flag_c1c: bool, crouch: bool, flag_34a0: bool) -> f32 {
    if override_h != 0.0 {
        override_h
    } else if flag_c1c {
        EYE_FLAG_C1C
    } else if crouch {
        EYE_CROUCH
    } else if flag_34a0 {
        EYE_FLAG_34A0
    } else {
        EYE_STAND
    }
}

/// h = target*k + (1-k)*h with k = clamp(5*dt, 0, 1) [C 0x45dc68].
pub fn smooth_eye(h: f32, target: f32, dt: f32) -> f32 {
    let k = (EYE_SMOOTH_RATE * dt).clamp(0.0, 1.0);
    target * k + (1.0 - k) * h
}

/// FOV smoothing rate (`5*dt`, same blend as the eye) [C CM_Cam 0x45fc6d].
pub const FOV_SMOOTH_RATE: f32 = 5.0;
/// Weapon id (G+0x3348) that selects the 0.3 zoom [C 0x45f91b]; sniper rifle [L].
pub const ZOOM_WEAPON_ID: i32 = 4;

/// Target FOV (CM_Cam 0x45f8c8..0x45fc43). `scope_blocked` = Jack +0x110c or +0x1118 set
/// (no zoom, keeps 1.2); `lock_fov` = FOV computed toward the lock-on target when not aiming.
pub fn fov_target(aim: bool, scope_blocked: bool, weapon_id: i32, lock_fov: Option<f32>) -> f32 {
    if aim {
        if scope_blocked {
            FOV_DEFAULT
        } else if weapon_id == ZOOM_WEAPON_ID {
            FOV_SNIPER
        } else {
            FOV_AIM
        }
    } else {
        lock_fov.unwrap_or(FOV_DEFAULT)
    }
}

pub fn smooth_fov(fov: f32, target: f32, dt: f32) -> f32 {
    let k = (FOV_SMOOTH_RATE * dt).clamp(0.0, 1.0);
    target * k + (1.0 - k) * fov
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deadzone_rescales_to_full_range() {
        assert_eq!(normalize_stick(0.1), 0.0);
        assert!((normalize_stick(1.0) - 1.0).abs() < 1e-6);
        assert!((normalize_stick(-1.0) + 1.0).abs() < 1e-6);
        assert!((normalize_stick(0.575) - 0.5).abs() < 1e-6);
    }
    #[test]
    fn speed_time_constant_is_quarter_second() {
        let mut s = 0.0;
        for _ in 0..15 {
            s = smooth_speed(s, RUN_SPEED, 1.0 / 60.0);
        }
        // after 0.25 s the exponential reaches ~63 %
        assert!((s / RUN_SPEED - (1.0 - (-1.0f32).exp())).abs() < 0.03);
    }

    #[test]
    fn target_speed_table() {
        let mut m = MoveState::default();
        m.input.stick_mag = 1.0;
        m.input.run = true;
        assert_eq!(m.target_speed(), 4.5);
        m.input.stick_mag = 1.2;
        assert!((m.target_speed() - 5.4).abs() < 1e-5);
        m.input.stick_mag = 1.0;
        m.input.crouch = true;
        assert_eq!(m.target_speed(), 1.5);
        m.input.crouch = false;
        m.input.run = false;
        m.ready_done = true;
        m.input.stick_mag = 1.0;
        assert_eq!(m.target_speed(), 2.5);
        m.input.aim = true;
        assert_eq!(m.target_speed(), 0.5);
        m.input.slow = true;
        assert_eq!(m.target_speed(), 1.5);
    }

    #[test]
    fn caps_and_wading() {
        let mut m = MoveState::default();
        m.input.stick_mag = 1.0;
        m.input.mode = 4;
        assert_eq!(m.target_speed(), 2.0);
        m.input.mode = 2;
        assert_eq!(m.target_speed(), 3.5);
        m.input.mode = 3;
        m.input.water_depth = 0.7;
        assert_eq!(m.target_speed(), 1.5); // run in shallow water = x1.5
        m.input.run = false;
        assert_eq!(m.target_speed(), 1.0 * m.input.stick_mag);
        m.input.water_depth = 1.2;
        assert_eq!(m.target_speed(), 1.0);
        m.input.mode = 0;
        m.input.run = true;
        m.input.water_depth = 5.0; // non-wade mode ignores depth
        m.input.flag40 = true;
        assert_eq!(m.target_speed(), 3.0);
    }

    #[test]
    fn ready_ramp_runs_4_5_to_2_5_in_a_tenth_of_a_second() {
        let mut m = MoveState::default();
        m.input.dir = Some(MoveDir { dir: [1.0, 0.0], joy_norm: 1.0 });
        m.input.stick_mag = 0.3; // ramp ignores stick magnitude
        m.step(0.001);
        m.input.run = false; // weapon raised
        m.step(0.0);
        assert!((m.target_speed() - 4.5).abs() < 1e-4);
        m.step(0.05);
        assert!((m.target_speed() - 3.5).abs() < 0.05);
        m.step(0.06);
        assert!(m.ready_done);
        assert!((m.target_speed() - 2.5 * 0.3).abs() < 1e-5);
    }

    #[test]
    fn step_smooths_with_four_per_second_and_keeps_vz() {
        let mut m = MoveState::default();
        m.input.stick_mag = 1.0;
        m.input.dir = Some(MoveDir { dir: [0.0, 1.0], joy_norm: 1.0 });
        m.input.vz = -3.0;
        let o = m.step(0.1); // k = 0.4
        assert!((o.speed - 1.8).abs() < 1e-5);
        assert!((o.velocity[1] - 1.8).abs() < 1e-5 && o.velocity[0] == 0.0 && o.velocity[2] == -3.0);
        m.input.dir = None; // stick released: instant stop, speed reset
        let o = m.step(0.1);
        assert_eq!(o.velocity, [0.0, 0.0, -3.0]);
        assert_eq!(m.speed, 0.0);
    }

    #[test]
    fn move_direction_thresholds() {
        let fwd = [1.0, 0.0];
        let left = [0.0, 1.0];
        assert!(move_direction(0.3, 0.0, fwd, left).is_none());
        // a0 = -1 (stick pushed the "negative" way) walks along camera forward; full speed
        let d = move_direction(-1.0, 0.0, fwd, left).unwrap();
        assert_eq!(d.dir, [1.0, 0.0]);
        assert_eq!(d.joy_norm, 1.0);
        // |v| = 0.5 after deadzone -> (0.5-0.25)/0.675
        let raw = 0.15 + 0.5 * 0.85;
        let d = move_direction(0.0, raw, fwd, left).unwrap();
        assert!((d.joy_norm - 0.25 / 0.675).abs() < 1e-5);
        assert_eq!(d.dir, [0.0, -1.0]);
    }

    #[test]
    fn crouch_toggle_and_ceiling() {
        assert!(crouch_intent(false, true, true, false, 0.0, 0.0, false, false));
        assert!(!crouch_intent(true, true, true, false, 0.0, 0.0, true, false));
        // ceiling forces crouch while crouched posture
        assert!(crouch_intent(true, true, true, false, 0.0, 0.0, true, true));
        // hold scheme
        assert!(crouch_intent(false, false, false, true, 0.0, 0.0, false, false));
        assert!(!crouch_intent(true, false, false, false, 0.0, 0.0, false, false));
        // deep water clears
        assert!(!crouch_intent(true, true, false, false, 0.0, 0.6, false, false));
    }

    #[test]
    fn look_factor_table() {
        let n = LookMode::default();
        assert_eq!(look_factors(n), (2.0, 1.75, 1.2));
        assert_eq!(look_factors(LookMode { run: true, ..n }), (3.0, 1.75, 1.2));
        assert_eq!(look_factors(LookMode { lock_on: true, ..n }), (1.5, 1.31, 1.0));
        let aim = look_factors(LookMode { aim: true, ..n });
        assert_eq!(aim, (0.6, 0.35, 1.0));
        let aim0 = look_factors(LookMode { aim: true, unarmed: true, ..n });
        assert!((aim0.0 - 0.9).abs() < 1e-6 && (aim0.1 - 0.525).abs() < 1e-6);
        assert_eq!(look_factors(LookMode { special: true, aim: true, ..n }), (9.0, 5.25, 3.6));
    }

    #[test]
    fn look_rate_and_ramp() {
        let mut st = LookState::default();
        let inp = LookInput {
            stick: (0.5, 0.0),
            sensitivity: 0.5,
            invert_y: false,
            mode: LookMode::default(),
            locked_chase: false,
        };
        let (yaw, pitch) = look_step(&mut st, &inp, 1.0 / 60.0);
        // x' = 0.5*2*0.5 = 0.5; yaw = -0.5*10/60*2.0
        assert!((yaw + 0.5 * 10.0 / 60.0 * 2.0).abs() < 1e-6);
        assert_eq!(pitch, 0.0);
        // full deflection builds the cubic ramp up to 1.2 s -> +1.728 on the factor
        let full = LookInput { stick: (1.0, 0.0), ..inp };
        let mut st = LookState::default();
        let mut last = 0.0;
        for _ in 0..120 {
            last = look_step(&mut st, &full, 1.0 / 60.0).0;
        }
        assert!((st.ramp - 1.2).abs() < 1e-5);
        assert!((last + 1.0 * 10.0 / 60.0 * (2.0 + 1.2f32.powi(3))).abs() < 1e-4);
        // aiming never ramps; deadzone snap
        let aim = LookInput { mode: LookMode { aim: true, ..LookMode::default() }, ..full };
        let mut st = LookState::default();
        look_step(&mut st, &aim, 0.5);
        assert_eq!(st.ramp, 0.0);
        let tiny = LookInput { stick: (0.009, 0.009), sensitivity: 0.5, ..inp };
        assert_eq!(look_step(&mut LookState::default(), &tiny, 0.1), (0.0, 0.0));
        // invert flips pitch sign
        let up = LookInput { stick: (0.0, 1.0), ..inp };
        let inv = LookInput { invert_y: true, ..up };
        let p0 = look_step(&mut LookState::default(), &up, 0.1).1;
        let p1 = look_step(&mut LookState::default(), &inv, 0.1).1;
        assert!(p0 < 0.0 && (p0 + p1).abs() < 1e-6);
    }

    #[test]
    fn pitch_clamp_and_eye_fov() {
        let v = clamp_pitch([0.0, 0.1, 0.99]);
        assert!((v[2] - 0.95).abs() < 1e-6 && (v[1] - 0.31225).abs() < 1e-6);
        let v = clamp_pitch([0.2, 0.0, -0.99]);
        assert!((v[2] + 0.95).abs() < 1e-6 && (v[0] - 0.31225).abs() < 1e-6);
        assert_eq!(clamp_pitch([0.6, 0.0, 0.8]), [0.6, 0.0, 0.8]);
        assert_eq!(eye_target(0.0, false, false, false), 1.6);
        assert_eq!(eye_target(0.0, false, true, true), 0.8);
        assert_eq!(eye_target(0.0, false, false, true), 1.45);
        assert_eq!(eye_target(0.0, true, true, true), 1.4);
        assert_eq!(eye_target(2.0, true, true, true), 2.0);
        let h = smooth_eye(1.6, 0.8, 0.1); // k = 0.5
        assert!((h - 1.2).abs() < 1e-6);
        assert_eq!(fov_target(false, false, 1, None), 1.2);
        assert_eq!(fov_target(true, false, 1, None), 0.6);
        assert_eq!(fov_target(true, false, 4, None), 0.3);
        assert_eq!(fov_target(true, true, 4, None), 1.2);
        assert!((smooth_fov(1.2, 0.6, 0.1) - 0.9).abs() < 1e-6);
    }
}
