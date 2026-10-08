//! `kk-extract --game "<install dir>" --out <assets dir> [--only <asset>] [--force] [--list]`
use std::path::PathBuf;
use std::process::ExitCode;

fn usage() {
    eprintln!(
        "kk-extract: build the game assets from your own copy of King Kong (2005) PC Gamer's Edition\n\n\
         usage: kk-extract --game <install dir> --out <assets dir> [--only <asset file>] [--force]\n\
                kk-extract --list\n\n\
         The install dir must contain KKMaps.bf and KKTextures.bf (Sound_Common.bf is not used yet).\n\
         Existing up-to-date outputs are skipped; --force rebuilds everything."
    );
}

fn main() -> ExitCode {
    let mut game: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut opts = kk_extract::Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--game" => game = args.next().map(PathBuf::from),
            "--out" => out = args.next().map(PathBuf::from),
            "--only" => opts.only = args.next(),
            "--force" => opts.force = true,
            "--list" => {
                for n in kk_extract::manifest_names() {
                    println!("{n}");
                }
                return ExitCode::SUCCESS;
            }
            "-h" | "--help" => {
                usage();
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument '{other}'");
                usage();
                return ExitCode::from(2);
            }
        }
    }
    let (Some(game), Some(out)) = (game, out) else {
        usage();
        return ExitCode::from(2);
    };
    let t0 = std::time::Instant::now();
    let mut cb = |p: &kk_extract::Progress| eprintln!("[{}/{}] {}: {}", p.step, p.total, p.asset, p.message);
    match kk_extract::build_with(&game, &out, &opts, &mut cb) {
        Ok(rep) => {
            eprintln!("done in {:.1}s: {} built, {} up to date, {} failed", t0.elapsed().as_secs_f32(), rep.built.len(), rep.skipped.len(), rep.failed.len());
            for (n, e) in &rep.failed {
                eprintln!("  FAILED {n}: {e}");
            }
            if rep.ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
