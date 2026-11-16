use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

const EXE: &str = env!("CARGO_BIN_EXE_workyard");

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("workyard-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn version_prints_name_and_version_and_json_adds_schema() {
    let out = Command::new(EXE).arg("--version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), format!("workyard {}", env!("CARGO_PKG_VERSION")));
    let out = Command::new(EXE).args(["--version", "--json"]).output().unwrap();
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["data"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(value["data"]["schema_version"].as_i64().unwrap() > 0);
}

#[test]
fn doctor_fails_when_claude_is_missing() {
    let dir = temp_dir("doctor");
    // A PATH holding only a fake git, so `claude` cannot be found.
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let git = bin.join("git");
    std::fs::write(&git, "#!/bin/sh\necho 'git version 0.0-fake'\n").unwrap();
    std::fs::set_permissions(&git, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let run = |json: bool| {
        let mut cmd = Command::new(EXE);
        cmd.env("PATH", &bin).args(["--db", dir.join("t.db").to_str().unwrap()]);
        if json {
            cmd.arg("--json");
        }
        cmd.arg("doctor").output().unwrap()
    };
    let text = run(false);
    assert_eq!(text.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&text.stdout);
    assert!(stdout.contains("FAIL  claude"), "{stdout}");
    assert!(stdout.contains("OK    git"), "{stdout}");
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")), "{stdout}");
    let json = run(true);
    assert_eq!(json.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["ok"], false);
    let checks = value["data"]["checks"].as_array().unwrap();
    assert!(checks.iter().any(|c| c["name"] == "claude" && c["status"] == "FAIL"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn doctor_passes_with_claude_and_git_present() {
    let dir = temp_dir("doctor-ok");
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for tool in ["git", "claude"] {
        let path = bin.join(tool);
        std::fs::write(&path, format!("#!/bin/sh\necho '{tool} 1.2.3'\n")).unwrap();
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    }
    let out = Command::new(EXE)
        .env("PATH", &bin)
        .env("WORKYARD_PLUGIN_DIR", dir.join("plugin"))
        .args(["--db", dir.join("t.db").to_str().unwrap(), "doctor"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stdout));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn versions_are_consistent_everywhere() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = Command::new("sh").arg(root.join("scripts/check-version.sh")).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}
