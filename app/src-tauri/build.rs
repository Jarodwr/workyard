use std::{env, fs, path::Path};

// `bundle.externalBin` makes tauri_build fail when the staged sidecar is missing, which breaks
// `cargo check/test --workspace` on a fresh checkout. Create a placeholder script then. Real builds
// stage the CLI first (tauri.conf.json build commands run scripts/stage-sidecar.sh), and
// `scripts/stage-sidecar.sh --check` rejects the placeholder.
fn main() {
    if let Ok(triple) = env::var("TARGET") {
        let ext = if triple.contains("windows") { ".exe" } else { "" };
        let path = Path::new("binaries").join(format!("workyard-{triple}{ext}"));
        if !path.exists() {
            let _ = fs::create_dir_all("binaries");
            let _ = fs::write(
                &path,
                "#!/bin/sh\n# workyard-placeholder-sidecar\necho 'workyard: placeholder sidecar; run scripts/stage-sidecar.sh' >&2\nexit 1\n",
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o755));
            }
        }
    }
    // tauri_build copies the staged sidecar over target/<profile>/workyard, which is where the CLI
    // crate builds to. In dev and test builds that file is the CLI itself (the dev app finds its
    // sidecar next to its own executable), so the copy could only replace a fresh CLI with a stale
    // or placeholder one, depending on build order. Skip it for the debug profile; release builds
    // and the bundler (which reads tauri.conf.json itself) still ship the staged sidecar.
    if env::var("PROFILE").as_deref() == Ok("debug") && env::var_os("TAURI_CONFIG").is_none() {
        let patch = r#"{"bundle":{"externalBin":[]}}"#;
        // SAFETY: the build script is single-threaded here.
        unsafe { env::set_var("TAURI_CONFIG", patch) };
        println!("cargo:rustc-env=TAURI_CONFIG={patch}");
    }
    println!("cargo:rerun-if-env-changed=TAURI_CONFIG");
    println!("cargo:rerun-if-changed=binaries");
    tauri_build::build();
}
