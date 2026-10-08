//! Differential test: the Rust decoder/framing against the original C tool (`tools/lzo1x.c`, compiled
//! to `$KK_TEST_DATA/lzo1x`) on random valid LZO1X streams (generator in `kk_extract::lzo::gen`).

use kk_extract::lzo::gen;
use kk_extract::stream::unstream;

#[test]
fn rust_matches_c_tool_on_random_streams() {
    let Some(dir) = std::env::var_os("KK_TEST_DATA").map(std::path::PathBuf::from) else {
        eprintln!("KK_TEST_DATA not set: skipping");
        return;
    };
    let exe = dir.join("lzo1x");
    if !exe.exists() {
        eprintln!("no compiled lzo1x tool in KK_TEST_DATA: skipping");
        return;
    }
    let tmp = std::env::temp_dir().join(format!("kk_lzo_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let (mut total, mut compressed_blocks) = (0usize, 0usize);
    for batch in 0..20u64 {
        let mut framed = Vec::new();
        let mut want = Vec::new();
        for k in 0..8u64 {
            let (c, e) = gen::stream(batch * 100 + k + 1, 200 + (k as usize) * 300);
            if c.len() >= e.len() {
                continue; // would be framed as a raw block by the game's rule (clen >= ulen)
            }
            framed.extend_from_slice(&(e.len() as u32).to_le_bytes());
            framed.extend_from_slice(&(c.len() as u32).to_le_bytes());
            framed.extend_from_slice(&c);
            framed.extend_from_slice(&[0, 0, 0, 0]);
            want.extend_from_slice(&e);
            compressed_blocks += 1;
        }
        framed.extend_from_slice(&[0u8; 8]);
        let (rin, rout) = (tmp.join("in.raw"), tmp.join("out.dec"));
        std::fs::write(&rin, &framed).unwrap();
        let st = std::process::Command::new(&exe).arg(&rin).arg(&rout).output().unwrap();
        assert!(st.status.success(), "C tool failed: {}", String::from_utf8_lossy(&st.stderr));
        let c_out = std::fs::read(&rout).unwrap();
        let r_out = unstream(&framed).unwrap();
        assert_eq!(c_out, want, "C tool output differs from generator expectation (batch {batch})");
        assert_eq!(r_out, c_out, "Rust output differs from C tool (batch {batch})");
        total += r_out.len();
    }
    eprintln!("{compressed_blocks} LZO blocks, {total} bytes: Rust == C tool == expected");
    let _ = std::fs::remove_dir_all(&tmp);
}
