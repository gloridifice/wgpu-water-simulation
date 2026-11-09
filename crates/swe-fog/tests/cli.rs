//! CLI delivery boundaries distinct from the geometry and settings owners.
use std::{path::PathBuf, process::Command};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "swe-fog-cli-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn command_line_bakes_requested_bounds_and_never_overwrites_an_existing_asset() {
    let dir = Directory::new();
    let path = dir.0.join("explicit.fogterrain");
    let exe = env!("CARGO_BIN_EXE_swe-fog");
    let output = Command::new(exe)
        .args([
            "--bake",
            path.to_str().unwrap(),
            "--resolution",
            "8x4",
            "--domain",
            "-2,-1,4,2",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let terrain =
        swe_fog::terrain::Terrain::load(&path, &swe_fog::terrain::BakeSource::playground())
            .unwrap();
    assert_eq!(terrain.metadata.resolution, [8, 4]);
    assert_eq!(terrain.metadata.domain, [-2.0, -1.0, 4.0, 2.0]);
    let before = std::fs::read(&path).unwrap();
    assert!(
        !Command::new(exe)
            .args(["--bake", path.to_str().unwrap()])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(std::fs::read(path).unwrap(), before);
}
#[test]
fn help_bad_arguments_and_strict_config_do_not_start_windows_or_repair_files() {
    let exe = env!("CARGO_BIN_EXE_swe-fog");
    assert!(
        Command::new(exe)
            .arg("--help")
            .output()
            .unwrap()
            .status
            .success()
    );
    for args in [
        ["--no-settings", "--frames", "0"],
        ["--no-settings", "--time", "NaN"],
        ["--unknown", "--frames", "1"],
    ] {
        assert!(
            !Command::new(exe)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let dir = Directory::new();
    let path = dir.0.join("broken.json");
    std::fs::write(&path, b"{truncated").unwrap();
    let result = Command::new(exe)
        .args(["--config", path.to_str().unwrap(), "--frames", "1"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(std::fs::read(&path).unwrap(), b"{truncated");
    assert_eq!(
        std::fs::read_dir(&dir.0).unwrap().count(),
        1,
        "read-only loading must not back up or rewrite damaged files"
    );
}
