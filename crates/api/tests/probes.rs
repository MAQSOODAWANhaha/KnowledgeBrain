//! Mode-aware probe script contract: --help/usage, no secret leakage.

use std::process::Command;

fn script() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/health/mode-aware-probe.sh")
}

#[test]
fn mode_aware_probe_help_exits_zero() {
    let output = Command::new("bash")
        .arg(script())
        .arg("--help")
        .output()
        .expect("run mode-aware-probe --help");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("kind="), "stdout={stdout}");
    assert!(stdout.contains("target="), "stdout={stdout}");
    assert!(stdout.contains("/live"), "stdout={stdout}");
    assert!(stdout.contains("/ready"), "stdout={stdout}");
    let lower = stdout.to_ascii_lowercase();
    assert!(!lower.contains("password"), "help leaked a password");
    assert!(!stdout.contains("SECRET"), "help leaked a secret");
    assert!(!stdout.contains("DATABASE_URL"), "help leaked DATABASE_URL");
}

#[test]
fn mode_aware_probe_usage_on_bad_args() {
    let output = Command::new("bash")
        .arg(script())
        .arg("nope")
        .output()
        .expect("run mode-aware-probe with bad args");
    assert!(!output.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("Usage"), "combined={combined}");
    let lower = combined.to_ascii_lowercase();
    assert!(!lower.contains("password"));
    assert!(!combined.contains("SECRET"));
    assert!(!combined.contains("DATABASE_URL"));
}
