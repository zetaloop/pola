use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=assets/pola.ico");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is not set"));
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
