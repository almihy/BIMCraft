//! DWG drawings on the command line, read as the apps read them: each new file first in a worker, a child process of the
//! program itself (`bimcraft-cli --dwg-worker`, `cadcraft_dwg::worker`), so a file that brings the decoder down takes only
//! the child with it; then straight into the import, its blocks placed by reference.

use std::path::PathBuf;
use std::process::Command;

const EXE: &str = env!("CARGO_BIN_EXE_bimcraft-cli");

fn dir(test: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("bimcraft-cli-dwg-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// What `import_cad` of `path` says, run by the program: its reply, or its error.
fn import(path: &std::path::Path) -> Result<serde_json::Value, String> {
    let cmd = format!("import_cad {}", serde_json::json!({"path": path, "layers": "all"}));
    let out = Command::new(EXE).args(["run", "--cmd", &cmd]).output().map_err(|e| e.to_string())?;
    let said = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        return Err(format!("{said}\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_str(said.lines().last().unwrap_or_default()).map_err(|e| format!("{e}: {said}"))
}

#[test]
fn import_cad_reads_a_dwg_through_the_programs_worker() {
    let path = dir("import").unwrap().join("site.dwg");
    std::fs::write(&path, bimcraft_core::dxf::dwg::sample::plan().unwrap()).unwrap();
    let reply = import(&path).unwrap();
    assert_eq!(
        (reply["source"].as_str(), reply["unit"].as_str(), reply["entities"].as_u64()),
        (Some("site.dwg"), Some("mm"), Some(11)),
        "{reply}"
    );
}

#[test]
fn a_file_that_isnt_a_dwg_says_so_and_the_program_goes_on() {
    let bad = dir("bad").unwrap().join("bad.dwg");
    std::fs::write(&bad, "AC1032 not a drawing at all").unwrap();
    let e = import(&bad).unwrap_err();
    assert!(e.contains("bad.dwg"), "{e}");
}
