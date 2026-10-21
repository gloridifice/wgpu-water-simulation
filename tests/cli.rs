use std::process::Command;

#[test]
fn command_line_runs_and_rejects_invalid_arguments() {
    let exe = env!("CARGO_BIN_EXE_water-simulation");
    let result = Command::new(exe)
        .args(["--cells", "41", "--time", "0.2", "--frames", "2"])
        .output()
        .unwrap();
    assert!(result.status.success());
    let stdout = String::from_utf8(result.stdout).unwrap();
    assert!(stdout.contains("t=0.000s") && stdout.contains("t=0.200s"));
    assert!(stdout.contains("*=surface") && stdout.contains("relative_mass_error="));
    assert!(
        !stdout.contains('\x1b'),
        "default output must work without ANSI support"
    );

    let help = Command::new(exe).arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8(help.stdout).unwrap().contains("Usage:"));
    for args in [
        vec!["--cells", "2"],
        vec!["--time", "NaN"],
        vec!["--frames", "0"],
        vec!["--cfl", "0.9"],
        vec!["--cells"],
        vec!["--unknown"],
        vec!["--quiet", "--animate"],
        vec!["--tui", "--headless"],
        vec!["--paused", "--quiet"],
        vec!["--tui"],
    ] {
        let output = Command::new(exe).args(args).output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8(output.stderr).unwrap().contains("error:"));
    }
}

#[test]
fn explicit_headless_and_redirected_output_keep_batch_mode() {
    let exe = env!("CARGO_BIN_EXE_water-simulation");
    for args in [
        vec!["--headless", "--quiet", "--time", "0.1"],
        vec!["--cells", "41"],
    ] {
        let result = Command::new(exe).args(args).output().unwrap();
        assert!(result.status.success());
        let text = String::from_utf8(result.stdout).unwrap();
        assert!(text.contains("relative_mass_error=") || text.contains("PIC t="));
        assert!(!text.contains('\x1b'));
    }
}

#[test]
fn pic_headless_mode_and_invalid_grid_arguments() {
    let exe = env!("CARGO_BIN_EXE_water-simulation");
    let output = Command::new(exe)
        .args([
            "--pic",
            "--headless",
            "--quiet",
            "--time",
            "0.02",
            "--nx",
            "48",
            "--ny",
            "32",
            "--no-pulse",
            "--no-waves",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("PIC t=0.020s")
            && text.contains("particles=")
            && text.contains("divergence=")
    );
    for args in [
        vec!["--nx", "0"],
        vec!["--ny", "10000"],
        vec!["--cell-size", "NaN"],
        vec!["--pressure-iterations", "0"],
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
}
