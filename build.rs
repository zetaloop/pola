use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is not set"));
    let root =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set"));

    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("windows") => windows(&out),
        Ok("macos") if out.starts_with(root.join("target/app-build/MacOS")) => macos(&root),
        _ => {}
    }
}

fn macos(root: &Path) {
    println!("cargo:rerun-if-changed=assets/Info.plist");
    println!("cargo:rerun-if-changed=assets/pola.icns");

    let contents = root.join("target/pola.app/Contents");
    let resources = contents.join("Resources");
    fs::create_dir_all(&resources).expect("failed to create macOS app resources");
    fs::copy("assets/pola.icns", resources.join("pola.icns"))
        .expect("failed to stage macOS app icon");

    let plist = fs::read_to_string("assets/Info.plist")
        .expect("failed to read macOS app metadata")
        .replace(
            "@VERSION@",
            &env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION is not set"),
        );
    fs::write(contents.join("Info.plist"), plist).expect("failed to write macOS app metadata");
}

fn windows(out: &Path) {
    println!("cargo:rerun-if-changed=assets/pola.ico");

    let profile = env::var_os("PROFILE").expect("PROFILE is not set");
    let target = out
        .ancestors()
        .find(|path| path.file_name() == Some(profile.as_ref()))
        .expect("Cargo target directory not found");

    fs::copy("assets/pola.ico", target.join("pola.ico"))
        .expect("failed to stage the notification icon");

    #[cfg(windows)]
    {
        windows_reactor_setup::as_self_contained();

        winresource::WindowsResource::new()
            .set_icon("assets/pola.ico")
            .compile()
            .expect("failed to embed Windows resources");
    }
}
