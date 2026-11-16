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
    println!("cargo:rerun-if-changed=binaries");
    tauri_build::build();
}
