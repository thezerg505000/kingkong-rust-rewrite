//! Progression: per-chapter stats, scoring, unlocks, chapter loadouts, checkpoint rules.
//! Ledger P01-P15, I11, G07, W07. Tags: [C] code/data, [L] inferred, [G] guess.

/// Per-chapter stat record: 20 floats at G+0x1df4 + chapterIdx*0x50 [C] Stats_OnPlayerDeath@0x52f240.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChapterStats {
    pub time: f32,          // +0x00, capped 3600 [C] fn@0x0052f540
    pub paf: f32,           // +0x04, cap 999
    pub deaths: f32,        // +0x08, cap 99
    pub shots: f32,         // +0x0c
    pub spears: f32,        // +0x10, cap 99
    pub death_variant: f32, // +0x14
    pub kills_player: f32,  // +0x18
    pub kills_other: f32,   // +0x1c
    pub jack_a: f32,        // +0x20
    pub jack_b: f32,        // +0x24
    pub jack_c: f32,        // +0x28
    pub kong_a: f32,        // +0x2c
    pub kong_b: f32,        // +0x30
    pub kong_attacks: f32,  // +0x34
    pub jack_wounds: f32,   // +0x38 cap 99
    pub companion_wounds: f32, // +0x3c cap 99
}

/// Kill class of a Jack kill. Kill_Credit@0x52f950 [C].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JackClass { A, B, C }
pub fn jack_kill_class(kill_type: u32) -> Option<JackClass> {
    match kill_type {
        2 | 6 | 9 | 0xf => Some(JackClass::A),
        3 | 0xb | 0xc => Some(JackClass::B),
        4 | 5 | 7 | 0xd | 0xe => Some(JackClass::C),
        _ => None,
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KongClass { A, B }
pub fn kong_kill_class(kill_type: u32) -> Option<KongClass> {
    match kill_type {
        1 | 10 | 0xf => Some(KongClass::A),
        3 | 7 | 8 | 9 | 0xb | 0xc | 0x10 => Some(KongClass::B),
        _ => None,
    }
}

/// Score line weights, IntMIG_Page_ScoreNew@0x61a2c0 / fn@0x004e1330 [C].
pub const JACK_WEIGHTS: [f32; 8] = [5000.0, 1000.0, 500.0, -100.0, -20.0, -5000.0, -1000.0, -1000.0];
pub const KONG_WEIGHTS: [f32; 4] = [5000.0, 500.0, -50.0, -20.0];

/// Per-chapter scoring row, fn@0x004dfdf0@0x4dfdf0 [C].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScoreRow { pub id: u32, pub base: f32, pub bonus_mult: f32, pub penalty_mult: f32 }
pub const DEFAULT_TOP5: [i32; 5] = [20000, 19000, 17000, 14000, 10000];
const fn r(id: u32, base: f32, b: f32, p: f32) -> ScoreRow { ScoreRow { id, base, bonus_mult: b, penalty_mult: p } }
pub const SCORE_TABLE: [ScoreRow; 33] = [
    r(101, 10000.0, 1.0, 1.0), r(201, 15000.0, 0.5, 1.0), r(203, 10000.0, 1.0, 1.0),
    r(204, 10000.0, 1.0, 1.0), r(301, 10000.0, 1.0, 1.0), r(302, 10000.0, 0.5, 1.0),
    r(303, 10000.0, 0.5, 0.5), r(304, 10000.0, 0.5, 0.5), r(305, 20000.0, 1.0, 0.5),
    r(306, 5000.0, 1.0, 0.5), r(401, 10000.0, 0.5, 0.5), r(402, 15000.0, 0.5, 0.5),
    r(403, 15000.0, 0.2, 0.5), r(404, 15000.0, 0.5, 1.0), r(501, 10000.0, 1.0, 0.5),
    r(502, 20000.0, 1.0, 0.5), r(504, 15000.0, 1.0, 2.0), r(702, 15000.0, 0.5, 1.0),
    r(705, 15000.0, 0.5, 1.0), r(1002, 10000.0, 0.5, 2.0), r(1101, 25000.0, 1.0, 2.0),
    r(1201, 15000.0, 0.2, 0.5), r(703, 15000.0, 0.5, 1.0), r(1401, 20000.0, 1.0, 2.0),
    r(1402, 10000.0, 0.5, 0.5), r(1501, 15000.0, 0.2, 1.0), r(1701, 15000.0, 0.5, 1.0),
    r(1702, 5000.0, 1.0, 1.0), r(1703, 10000.0, 1.0, 2.0), r(1803, 15000.0, 1.0, 1.0),
    r(1804, 10000.0, 1.0, 1.0), r(1901, 10000.0, 0.5, 0.5), r(2001, 15000.0, 1.0, 1.0),
];
pub fn score_row(id: u32) -> Option<&'static ScoreRow> { SCORE_TABLE.iter().find(|x| x.id == id) }

/// Kong chapters, fn@0x004e0dc0@0x4e0dc0 [C].
pub const KONG_CHAPTERS: [u32; 13] = [304, 504, 601, 705, 1002, 1402, 1703, 1901, 2001, 2002, 2003, 2004, 2100];
pub fn is_kong_chapter(id: u32) -> bool { KONG_CHAPTERS.contains(&id) }

fn line(w: f32, count: f32, bonus: f32, penalty: f32) -> i32 {
    let m = if w < 0.0 { -(w.abs() * penalty) } else { w * bonus };
    (m * count) as i32
}

/// Total chapter score (header base + weighted lines, clamped >= 0). fn@0x004e1330 [C]; the
/// truncation of each line (`_ftol`) is [C]; the order of multiply is [L].
pub fn chapter_score(id: u32, s: &ChapterStats) -> i32 {
    let Some(row) = score_row(id) else { return 0 };
    let mut total = row.base as i32;
    let (b, p) = (row.bonus_mult, row.penalty_mult);
    if is_kong_chapter(id) {
        let counts = [s.kong_a, s.kong_b, s.kong_attacks, s.time];
        for (w, c) in KONG_WEIGHTS.iter().zip(counts) { total += line(*w, c, b, p); }
    } else {
        let counts = [s.jack_a, s.jack_b, s.jack_c, s.shots, s.spears, s.deaths, s.jack_wounds, s.companion_wounds];
        for (w, c) in JACK_WEIGHTS.iter().zip(counts) { total += line(*w, c, b, p); }
    }
    total.max(0)
}

/// Unlockable extras, fn@0x004dc1b0@0x4dc1b0 [C]. 20 slots.
pub const EXTRA_SCORE_THR: [i32; 20] = [75000, -1, 250000, -1, 0, -1, 20000, 50000, -1, -1, 100000, 150000, -1, -100, -100, -1, -100, -100, -1, -1];
/// Progress threshold (fraction of campaign) or >9.0 = event flag; -1 = none.
pub const EXTRA_PROGRESS_THR: [f32; 20] = [-1.0, 0.15, 1.0, 20.0, 0.0, 0.75, -1.0, -1.0, 1.0, 0.5, -1.0, -1.0, 0.05, 0.0, 0.0, 10.0, 0.0, 0.0, -1.0, -1.0];
/// slot 4 is never initialised in the dump (zero memory => threshold 0, always unlocked) [L].
pub fn unlock_mask(total_best: i32, progress_pct: i32, event_flags: &[bool; 20]) -> u32 {
    let mut mask = 0u32;
    for i in 0..20 {
        let thr = EXTRA_SCORE_THR[i];
        let f = EXTRA_PROGRESS_THR[i];
        if thr == -100 { continue; }
        let unlocked = if f <= 9.0 { f <= progress_pct as f32 / 100.0 && total_best >= thr } else { event_flags[i] };
        if unlocked { mask |= 1 << i; }
    }
    mask
}
/// Campaign progress percent. fn@0x004dc1b0 [C].
pub fn progress_pct(chapter_idx: i32, chapter_count: i32) -> i32 {
    if chapter_idx >= chapter_count - 1 { 100 } else { (chapter_idx as f32 / (chapter_count - 1) as f32 * 100.0) as i32 }
}

/// Campaign order (42 entries), uni_init_all.txt / G+0x5010 [C].
pub const CAMPAIGN: [u32; 42] = [100, 101, 201, 203, 204, 301, 302, 303, 305, 306, 304, 401, 402, 403, 404, 501, 502, 504, 702, 705, 901, 1002, 1101, 1200, 1201, 703, 1401, 1402, 1501, 1701, 1702, 1703, 1801, 1803, 1804, 1805, 1901, 1902, 2001, 2003, 2004, 7];

/// Chapter starting loadout (Jack weapon, ammo, Hayes weapon, ammo); 1 Colt 2 Tommy 3 Shotgun 4 Sniper. [C]
pub fn chapter_loadout(id: u32) -> Option<(u8, u16, u8, u16)> {
    Some(match id {
        101 => (0, 0, 1, 50), 201 => (3, 5, 0, 0), 203 => (1, 20, 4, 25), 204 => (4, 20, 4, 65),
        303 => (1, 5, 2, 100), 305 => (2, 30, 0, 0), 306 => (2, 10, 0, 0), 401 => (2, 5, 0, 0),
        402 => (4, 5, 3, 16), 403 => (4, 1, 3, 10), 404 => (3, 5, 0, 0), 501 => (4, 45, 1, 150),
        502 => (1, 50, 4, 50), 702 => (1, 10, 1, 10), 1401 => (4, 5, 4, 5), 1501 => (4, 5, 4, 5),
        _ => return None,
    })
}
/// Magazine size by weapon id. fn@0x0052ef90@0x52ef90 [C].
pub fn mag_size(w: u8) -> u16 { match w { 1 => 8, 2 => 50, 3 => 5, 4 => 5, _ => 0 } }
/// Split ammo into (magazine, reserve). fn@0x0052ef90 [C].
pub fn split_ammo(w: u8, ammo: u16) -> (u16, u16) {
    let m = mag_size(w);
    if ammo > m { (m, ammo - m) } else { (ammo, 0) }
}
/// Chapter 301 strips Jack's weapons on entry. ES_EnterMap@0x6f18d0 [C] (id 0x12d).
pub const STRIP_WEAPONS_CHAPTER: u32 = 301;

/// Low-ammo call-out. H_exec_text@0x58c3f0 [C].
pub fn low_ammo_threshold(w: u8) -> u16 { match w { 1 => 2, 2 => 10, 3 => 1, 4 => 1, _ => 0 } }
/// Speech id and min interval (s) for a low-ammo call-out; `alt` = this[1099] != 0.
pub fn low_ammo_line(reserve: u16, alt: bool) -> (u32, u32) {
    match (alt, reserve == 0) {
        (false, true) => (0xd0005ee7, 1), (false, false) => (0xd0005eec, 60),
        (true, true) => (0xd0005f98, 1), (true, false) => (0xd0005f9d, 60),
    }
}

/// Music phase selection (SM_Scan_Phase_Kit@0x51e2b0) [C structure, persistence 0x10 ticks].
pub const MUSIC_PERSIST_TICKS: u32 = 0x10;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kill_classes() {
        assert_eq!(jack_kill_class(0xf), Some(JackClass::A));
        assert_eq!(jack_kill_class(0xb), Some(JackClass::B));
        assert_eq!(jack_kill_class(7), Some(JackClass::C));
        assert_eq!(kong_kill_class(10), Some(KongClass::A));
        assert_eq!(kong_kill_class(8), Some(KongClass::B));
        assert_eq!(jack_kill_class(0), None);
    }
    #[test]
    fn jack_score() {
        // 101: base 10000, mult 1/1; 2 A kills, 10 shots, 1 death
        let s = ChapterStats { jack_a: 2.0, shots: 10.0, deaths: 1.0, ..Default::default() };
        assert_eq!(chapter_score(101, &s), 10000 + 10000 - 1000 - 5000);
        // half penalty chapter 303
        let s = ChapterStats { deaths: 1.0, ..Default::default() };
        assert_eq!(chapter_score(303, &s), 10000 - 2500);
        assert_eq!(chapter_score(303, &ChapterStats { deaths: 99.0, ..Default::default() }), 0);
    }
    #[test]
    fn kong_score() {
        let s = ChapterStats { kong_a: 1.0, kong_b: 2.0, kong_attacks: 4.0, time: 30.0, ..Default::default() };
        // 304: base 10000, bonus .5, penalty .5
        assert_eq!(chapter_score(304, &s), 10000 + 2500 + 500 - 100 - 300);
        assert!(is_kong_chapter(2003) && !is_kong_chapter(305));
    }
    #[test]
    fn unlocks() {
        let none = [false; 20];
        assert_eq!(unlock_mask(0, 0, &none), (1 << 4) | (1 << 18) | (1 << 19));
        let m = unlock_mask(250000, 100, &none);
        assert!(m & (1 << 2) != 0 && m & (1 << 0) != 0);
        assert_eq!(unlock_mask(249999, 100, &none) & (1 << 2), 0);
        assert_eq!(progress_pct(41, 42), 100);
        assert_eq!(progress_pct(20, 42), 48);
    }
    #[test]
    fn loadouts() {
        assert_eq!(split_ammo(4, 45), (5, 40));
        assert_eq!(split_ammo(1, 5), (5, 0));
        assert_eq!(chapter_loadout(501), Some((4, 45, 1, 150)));
        assert_eq!(CAMPAIGN.len(), 42);
    }
    #[test]
    fn callouts() {
        assert_eq!(low_ammo_threshold(2), 10);
        assert_eq!(low_ammo_line(0, false), (0xd0005ee7, 1));
        assert_eq!(low_ammo_line(5, false), (0xd0005eec, 60));
    }
}
