fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        configure_bazel_windres()?;
    }
    tauri_build::build();
    Ok(())
}

fn configure_bazel_windres() -> Result<(), Box<dyn std::error::Error>> {
    let Some(source) = std::env::var_os("CODEX_BAZEL_WINDRES") else {
        return Ok(());
    };
    let project_dir = std::env::current_dir()?;
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").ok_or_else(|| {
        std::io::Error::other("OUT_DIR is not defined for the Cargo build script")
    })?);
    let shadow_project = out_dir.join("tauri-project");
    std::fs::create_dir_all(&shadow_project)?;
    std::fs::write(
        shadow_project.join("Cargo.toml"),
        concat!(
            "[package]\n",
            "name = \"codex-desktop\"\n",
            "version = \"0.0.0\"\n",
            "edition = \"2024\"\n",
            "\n",
            "[[bin]]\n",
            "name = \"codex-desktop\"\n",
            "path = \"src/main.rs\"\n",
        ),
    )?;
    for relative_path in [
        "tauri.conf.json",
        "icons/32x32.png",
        "icons/128x128.png",
        "icons/128x128@2x.png",
        "icons/icon.icns",
        "icons/icon.ico",
        "icons/icon.png",
    ] {
        let destination = shadow_project.join(relative_path);
        let parent = destination
            .parent()
            .ok_or_else(|| std::io::Error::other("shadow project file has no parent directory"))?;
        std::fs::create_dir_all(parent)?;
        std::fs::copy(project_dir.join(relative_path), destination)?;
    }

    let executable_name = if cfg!(windows) {
        "windres.exe"
    } else {
        "windres"
    };
    std::fs::copy(source, out_dir.join(executable_name))?;

    let mut path_entries = vec![out_dir];
    if let Some(clang) = std::env::var_os("CODEX_BAZEL_CLANG").map(std::path::PathBuf::from)
        && let Some(parent) = clang.parent()
    {
        path_entries.push(parent.to_path_buf());
    }
    let current_path = std::env::var_os("PATH").unwrap_or_default();
    path_entries.extend(std::env::split_paths(&current_path));
    let updated_path = std::env::join_paths(path_entries)?;
    // SAFETY: this build script is single-threaded and updates the environment before Tauri runs.
    unsafe {
        std::env::set_var("PATH", updated_path);
        std::env::set_var("CARGO_CFG_TARGET_ENV", "gnullvm");
    }
    std::env::set_current_dir(shadow_project)?;
    Ok(())
}
