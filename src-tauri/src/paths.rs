//! macOS app bundles are read-only. Keep mutable files in Application Support.
use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Some(path) = std::env::var_os("AINPUT_DATA_DIR") {
            return PathBuf::from(path);
        }
        PathBuf::from(std::env::var_os("HOME").expect("HOME is unavailable"))
            .join("Library/Application Support/com.ainput.dev")
    }
    #[cfg(not(target_os = "macos"))]
    {
        PathBuf::from(".")
    }
}

pub fn initialize() -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::{
            fs::OpenOptions,
            io::Write,
            os::unix::fs::{OpenOptionsExt, PermissionsExt},
        };
        let directory = data_dir();
        std::fs::create_dir_all(&directory)?;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory.join("config.toml"))
        {
            Ok(mut file) => file.write_all(include_bytes!("../config_macos.toml"))?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
