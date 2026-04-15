use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=run.sh");
    println!("cargo:rerun-if-changed=config.toml.example");
    println!("cargo:rerun-if-changed=static");

    let out_dir = env::var("OUT_DIR").unwrap();
    let profile_dir = PathBuf::from(out_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();

    let files_to_copy = vec!["run.sh", "config.toml.example"];
    for file in files_to_copy {
        let src = Path::new(file);
        let dest = profile_dir.join(file);
        if src.exists() {
            fs::copy(src, dest).ok();
        }
    }

    // Copy static directory
    let static_src = Path::new("static");
    let static_dest = profile_dir.join("static");
    if static_src.exists() {
        copy_dir_all(static_src, static_dest).ok();
    }
    
    // Create playlists directory
    let playlists_dest = profile_dir.join("playlists");
    if !playlists_dest.exists() {
        fs::create_dir_all(playlists_dest).ok();
    }
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}
