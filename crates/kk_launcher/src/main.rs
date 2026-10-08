//! KingKongRecompiled launcher.
//!
//! 1. find the user's King Kong (2005) PC install (`--game`, `game_path.txt`, `KKGAME`, common paths, prompt)
//! 2. verify KKMaps.bf / KKTextures.bf / Sound_Common.bf
//! 3. build `assets/` next to the exe with `kk_extract` (skipped when complete, or in developer override)
//! 4. start `bin/kk-fps(.exe)` with `KK_ASSETS` set, forwarding arguments.
//!
//! Design choice: the game is a separate child process (not linked as a library). Reasons: the launcher stays
//! tiny and builds in seconds, a crash or GPU failure in the game cannot take the extraction state with it,
//! and `kk_fps` keeps its own `main` (Bevy owns the main thread) unchanged.
//!
//! Developer override: env `KK_ASSETS`, or an `assets_dev/` folder next to the exe, is used as the asset root
//! and extraction is skipped. Never ship `assets_dev/`.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

pub const REQUIRED_BF: [&str; 3] = ["KKMaps.bf", "KKTextures.bf", "Sound_Common.bf"];
pub const GAME_PATH_FILE: &str = "game_path.txt";

#[derive(Debug, Default, PartialEq)]
pub struct Cli {
    pub game: Option<String>,
    pub batch: Option<String>,
    pub scene: Option<String>,
    pub out: Option<String>,
    pub rebuild: bool,
    pub extract_only: bool,
    pub help: bool,
    /// everything else, forwarded to the game
    pub rest: Vec<String>,
}

pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Cli, String> {
    let mut c = Cli::default();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let mut val = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "--game" => c.game = Some(val("--game")?),
            "--batch" => c.batch = Some(val("--batch")?),
            "--scene" => c.scene = Some(val("--scene")?),
            "--out" => c.out = Some(val("--out")?),
            "--rebuild-assets" => c.rebuild = true,
            "--extract-only" => c.extract_only = true,
            "-h" | "--help" => c.help = true,
            _ => {
                if let Some(v) = a.strip_prefix("--game=") {
                    c.game = Some(v.into())
                } else if let Some(v) = a.strip_prefix("--batch=") {
                    c.batch = Some(v.into())
                } else if let Some(v) = a.strip_prefix("--scene=") {
                    c.scene = Some(v.into())
                } else if let Some(v) = a.strip_prefix("--out=") {
                    c.out = Some(v.into())
                } else {
                    c.rest.push(a)
                }
            }
        }
    }
    Ok(c)
}

/// Environment variables for the game process derived from the command line.
pub fn game_env(c: &Cli) -> Vec<(&'static str, String)> {
    let mut v = vec![];
    if let Some(b) = &c.batch {
        v.push(("KK_BATCH", b.clone()));
    }
    if let Some(s) = &c.scene {
        v.push(("KK_SCENE", s.clone()));
    }
    if let Some(o) = &c.out {
        v.push(("KK_BATCH_OUT", o.clone()));
    }
    v
}

/// True when `dir` contains the three archives (case-insensitive, directly inside).
pub fn is_game_dir(dir: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else { return false };
    let names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect();
    REQUIRED_BF.iter().all(|r| names.contains(&r.to_lowercase()))
}

/// Accept the install folder, or a parent that contains it one level down (or the .exe path itself).
pub fn resolve_game_dir(p: &Path) -> Option<PathBuf> {
    let p = if p.is_file() { p.parent()?.to_path_buf() } else { p.to_path_buf() };
    if is_game_dir(&p) {
        return Some(p);
    }
    for e in std::fs::read_dir(&p).ok()?.flatten() {
        if e.path().is_dir() && is_game_dir(&e.path()) {
            return Some(e.path());
        }
    }
    None
}

/// Clean a pasted/dragged path: trim, strip quotes.
pub fn clean_path(s: &str) -> String {
    s.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_string()
}

pub fn read_game_path_file(exe_dir: &Path) -> Option<PathBuf> {
    let s = std::fs::read_to_string(exe_dir.join(GAME_PATH_FILE)).ok()?;
    let l = s.lines().map(clean_path).find(|l| !l.is_empty())?;
    Some(PathBuf::from(l))
}

/// Candidate folders to look in, in order, after the explicit sources.
pub fn common_candidates(exe_dir: &Path, program_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut v = vec![];
    for root in program_roots {
        for vendor in ["Ubisoft", "Ubisoft Entertainment"] {
            let base = root.join(vendor);
            if let Ok(rd) = std::fs::read_dir(&base) {
                for e in rd.flatten() {
                    if e.file_name().to_string_lossy().to_lowercase().contains("king kong") {
                        v.push(e.path());
                    }
                }
            }
        }
    }
    v.push(exe_dir.to_path_buf());
    if let Ok(rd) = std::fs::read_dir(exe_dir) {
        for e in rd.flatten() {
            if e.path().is_dir() {
                v.push(e.path());
            }
        }
    }
    v
}

/// Pure discovery: explicit arg, game_path.txt, KKGAME, then common paths. `None` means "ask the user".
pub fn find_game(arg: Option<&str>, exe_dir: &Path, kkgame: Option<&str>, program_roots: &[PathBuf]) -> Option<PathBuf> {
    let mut explicit: Vec<PathBuf> = vec![];
    if let Some(a) = arg {
        explicit.push(PathBuf::from(clean_path(a)));
    }
    if let Some(p) = read_game_path_file(exe_dir) {
        explicit.push(p);
    }
    if let Some(k) = kkgame {
        explicit.push(PathBuf::from(clean_path(k)));
    }
    for p in explicit.iter().chain(common_candidates(exe_dir, program_roots).iter()) {
        if let Some(d) = resolve_game_dir(p) {
            return Some(d);
        }
    }
    None
}

fn program_roots() -> Vec<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"].iter().filter_map(|k| std::env::var_os(k)).map(PathBuf::from).collect()
}

pub fn game_binary(exe_dir: &Path) -> PathBuf {
    let name = if cfg!(windows) { "kk-fps.exe" } else { "kk-fps" };
    let p = exe_dir.join("bin").join(name);
    if p.is_file() {
        return p;
    }
    exe_dir.join(name) // dev layouts: next to the launcher (cargo target dir)
}

/// Where assets come from: (dir, extraction_needed)
pub fn asset_root(exe_dir: &Path, env_assets: Option<&str>) -> (PathBuf, bool) {
    if let Some(a) = env_assets.filter(|a| !a.is_empty()) {
        return (PathBuf::from(a), false);
    }
    let dev = exe_dir.join("assets_dev");
    if dev.is_dir() {
        return (dev, false);
    }
    (exe_dir.join("assets"), true)
}

fn ask_game_path(exe_dir: &Path) -> Result<PathBuf, String> {
    let stdin = std::io::stdin();
    loop {
        println!("Paste or drag your King Kong (2005) PC install folder here and press Enter");
        println!("(the folder that contains KKMaps.bf, KKTextures.bf and Sound_Common.bf):");
        print!("> ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            return Err("no game folder given (stdin closed). Use --game <folder> or create game_path.txt".into());
        }
        let p = clean_path(&line);
        if p.is_empty() {
            continue;
        }
        match resolve_game_dir(Path::new(&p)) {
            Some(d) => {
                let _ = std::fs::write(exe_dir.join(GAME_PATH_FILE), format!("{}\n", d.display()));
                println!("Saved to {GAME_PATH_FILE}.");
                return Ok(d);
            }
            None => println!("Could not find the three .bf files in '{p}'. Try again.\n"),
        }
    }
}

fn usage() {
    println!(
        "KingKongRecompiled [options] [-- game args]\n\n\
  --game <dir>       King Kong (2005) PC install folder (saved to game_path.txt)\n\
  --batch <name>     run a scripted test batch (KK_BATCH), e.g. b7_kong_fight\n\
  --scene <name>     choose the scene (KK_SCENE), e.g. testarea\n\
  --out <dir>        output folder for batch results (KK_BATCH_OUT)\n\
  --rebuild-assets   force the asset build again\n\
  --extract-only     build assets, do not start the game\n\n\
Developer override: env KK_ASSETS or an assets_dev/ folder next to the exe is used as-is, no extraction."
    );
}

fn run() -> Result<i32, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_dir = exe.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let cli = parse_args(std::env::args().skip(1))?;
    if cli.help {
        usage();
        return Ok(0);
    }
    let env_assets = std::env::var("KK_ASSETS").ok();
    let (assets, need_extract) = asset_root(&exe_dir, env_assets.as_deref());

    if need_extract {
        let game = match find_game(cli.game.as_deref(), &exe_dir, std::env::var("KKGAME").ok().as_deref(), &program_roots()) {
            Some(g) => g,
            None => ask_game_path(&exe_dir)?,
        };
        if cli.game.is_some() {
            let _ = std::fs::write(exe_dir.join(GAME_PATH_FILE), format!("{}\n", game.display()));
        }
        println!("Game files: {}", game.display());
        if cli.rebuild || !kk_extract::is_complete(&assets) {
            println!("Building assets from your game files into {} (first run only)...", assets.display());
            let opts = kk_extract::Options { force: cli.rebuild, ..Default::default() };
            let report = kk_extract::build_with(&game, &assets, &opts, &mut |p| {
                println!("[{}/{}] {}: {}", p.step, p.total, p.asset, p.message);
            })
            .map_err(|e| format!("asset build failed: {e}"))?;
            for (a, m) in &report.failed {
                eprintln!("FAILED {a}: {m}");
            }
            if !report.ok() {
                return Err("some assets could not be built (see above)".into());
            }
            println!("Assets ready: {} built, {} up to date.", report.built.len(), report.skipped.len());
        } else {
            println!("Assets are up to date.");
        }
    } else {
        println!("Developer asset folder: {} (no extraction)", assets.display());
    }
    if cli.extract_only {
        return Ok(0);
    }

    let bin = game_binary(&exe_dir);
    if !bin.is_file() {
        return Err(format!("game binary not found: {} (run build_release.bat)", bin.display()));
    }
    println!("Starting {} ...", bin.display());
    let mut cmd = Command::new(&bin);
    cmd.args(&cli.rest).env("KK_ASSETS", &assets).current_dir(&exe_dir);
    for (k, v) in game_env(&cli) {
        cmd.env(k, v);
    }
    let status = cmd.status().map_err(|e| format!("could not start the game: {e}"))?;
    Ok(status.code().unwrap_or(1))
}

fn main() -> ExitCode {
    match run() {
        Ok(0) => ExitCode::SUCCESS,
        Ok(c) => ExitCode::from(c.clamp(1, 255) as u8),
        Err(e) => {
            eprintln!("error: {e}");
            if cfg!(windows) && std::env::var("KK_NO_PAUSE").is_err() {
                eprintln!("Press Enter to close.");
                let _ = std::io::stdin().read_line(&mut String::new());
            }
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kkl_{}_{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }
    fn mk_game(d: &Path) {
        fs::create_dir_all(d).unwrap();
        for f in ["kkmaps.BF", "KKTextures.bf", "Sound_Common.bf"] {
            fs::write(d.join(f), b"x").unwrap();
        }
    }

    #[test]
    fn args() {
        let c = parse_args(["--batch", "b7", "--out=o", "--scene", "testarea", "-x"].map(String::from)).unwrap();
        assert_eq!(c.batch.as_deref(), Some("b7"));
        assert_eq!(c.out.as_deref(), Some("o"));
        assert_eq!(c.rest, vec!["-x"]);
        let e = game_env(&c);
        assert!(e.contains(&("KK_BATCH", "b7".into())) && e.contains(&("KK_SCENE", "testarea".into())) && e.contains(&("KK_BATCH_OUT", "o".into())));
        assert!(parse_args(["--game".to_string()]).is_err());
    }

    #[test]
    fn game_dir_detection_case_insensitive_and_parent() {
        let d = tmp("gd");
        mk_game(&d.join("Peter Jackson's King Kong"));
        assert!(is_game_dir(&d.join("Peter Jackson's King Kong")));
        assert!(!is_game_dir(&d));
        assert_eq!(resolve_game_dir(&d), Some(d.join("Peter Jackson's King Kong")));
        fs::write(d.join("Peter Jackson's King Kong").join("KingKong8.exe"), b"").unwrap();
        assert_eq!(resolve_game_dir(&d.join("Peter Jackson's King Kong").join("KingKong8.exe")), Some(d.join("Peter Jackson's King Kong")));
    }

    #[test]
    fn clean() {
        assert_eq!(clean_path("  \"C:\\a b\\\"  "), "C:\\a b\\");
    }

    #[test]
    fn order_arg_then_file_then_env_then_common() {
        let d = tmp("ord");
        let (a, f, k, nxt) = (d.join("a"), d.join("f"), d.join("k"), d.join("exe"));
        for p in [&a, &f, &k] {
            mk_game(p);
        }
        fs::create_dir_all(&nxt).unwrap();
        fs::write(nxt.join(GAME_PATH_FILE), format!("\"{}\"\n", f.display())).unwrap();
        assert_eq!(find_game(Some(a.to_str().unwrap()), &nxt, Some(k.to_str().unwrap()), &[]), Some(a));
        assert_eq!(find_game(None, &nxt, Some(k.to_str().unwrap()), &[]), Some(f));
        fs::remove_file(nxt.join(GAME_PATH_FILE)).unwrap();
        assert_eq!(find_game(None, &nxt, Some(k.to_str().unwrap()), &[]), Some(k));
        assert_eq!(find_game(None, &nxt, None, &[]), None);
    }

    #[test]
    fn common_paths_and_next_to_exe() {
        let d = tmp("common");
        let pf = d.join("Program Files (x86)");
        mk_game(&pf.join("Ubisoft").join("Peter Jackson's King Kong - Gamers Edition"));
        let exe = d.join("exe");
        fs::create_dir_all(&exe).unwrap();
        assert_eq!(find_game(None, &exe, None, &[pf]), Some(d.join("Program Files (x86)/Ubisoft/Peter Jackson's King Kong - Gamers Edition")));
        mk_game(&exe.join("game"));
        assert_eq!(find_game(None, &exe, None, &[]), Some(exe.join("game")));
    }

    #[test]
    fn asset_root_rules() {
        let d = tmp("ar");
        assert_eq!(asset_root(&d, None), (d.join("assets"), true));
        assert_eq!(asset_root(&d, Some("/x")), (PathBuf::from("/x"), false));
        fs::create_dir_all(d.join("assets_dev")).unwrap();
        assert_eq!(asset_root(&d, None), (d.join("assets_dev"), false));
        assert_eq!(asset_root(&d, Some("")), (d.join("assets_dev"), false));
    }
}
