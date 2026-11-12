//! End-to-end tests of the econtoy binary.

use std::process::{Command, Output};

fn econtoy(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_econtoy"))
        .args(args)
        .output()
        .expect("run econtoy")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn builtin_scenario_runs_and_shows_the_government() {
    let out = econtoy(&["--scenario", "stimulus"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("scenario: Fiscal stimulus in a slump"));
    assert!(text.contains("gov-spending=40 @t66"));
    assert!(text.contains("public debt"));
    assert!(text.contains("Debt "));
}

#[test]
fn flags_after_a_scenario_override_it() {
    let out = econtoy(&["--scenario", "balanced", "--ticks", "30", "--csv"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out).lines().count(), 31);
}

#[test]
fn scenario_file_from_disk() {
    let dir = std::env::temp_dir().join(format!("econtoy-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mine.scenario");
    std::fs::write(&path, "name = Mine\nticks = 40\nat 10: income-tax = 30\n").unwrap();
    let out = econtoy(&["--scenario", path.to_str().unwrap(), "--json"]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let json = stdout(&out);
    assert!(json.contains("\"scenario\": \"Mine\""));
    assert!(json.contains("\"shocks\": [\"set:10:income-tax:30\"]"));
}

#[test]
fn bad_scenarios_fail_with_a_location() {
    let out = econtoy(&["--scenario", "no-such-thing"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("built-in: balanced, stimulus"));

    let dir = std::env::temp_dir().join(format!("econtoy-cli-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bad.scenario");
    std::fs::write(&path, "ticks = 50\nat 80: gov-spending = 10\n").unwrap();
    let out = econtoy(&["--scenario", path.to_str().unwrap()]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("scheduled at tick 80"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn list_scenarios_and_quiet_line() {
    let out = econtoy(&["--list-scenarios"]);
    assert!(out.status.success());
    assert_eq!(stdout(&out).lines().count(), 5);

    let out = econtoy(&["--quiet"]);
    assert!(!stdout(&out).contains("debt"));
    let out = econtoy(&["--quiet", "--income-tax", "20", "--gov-spending", "20"]);
    assert!(stdout(&out).contains("debt/GDP="));
}

#[test]
fn unknown_flags_are_rejected() {
    for args in [
        &["--set", "1:mpc:1"][..],
        &["--bogus"][..],
        &["--ticks"][..],
    ] {
        let out = econtoy(args);
        assert!(!out.status.success(), "{args:?}");
    }
}
