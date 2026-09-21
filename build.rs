use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is not set"));
    let root =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set"));

    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("windows") => windows(&out),
        Ok("macos") if out.starts_with(root.join("target/app-build/MacOS")) => macos(&root, &out),
        _ => {}
    }
}

fn macos(root: &Path, out: &Path) {
    println!("cargo:rerun-if-changed=assets/Info.plist");
    println!("cargo:rerun-if-changed=assets/Pola.icon");
    println!("cargo:rerun-if-changed=assets/en.lproj");
    println!("cargo:rerun-if-changed=assets/zh-Hans.lproj");

    let contents = root.join("target/pola.app/Contents");
    let resources = contents.join("Resources");
    fs::create_dir_all(&resources).expect("failed to create macOS app resources");
    for language in ["en", "zh-Hans"] {
        let folder = format!("{language}.lproj");
        let target = resources.join(&folder);
        fs::create_dir_all(&target).expect("failed to create localization directory");
        fs::copy(
            root.join("assets").join(folder).join("InfoPlist.strings"),
            target.join("InfoPlist.strings"),
        )
        .expect("failed to copy localized app metadata");
    }
    let result = Command::new("xcrun")
        .args(["actool", "assets/Pola.icon", "--compile"])
        .arg(&resources)
        .args([
            "--app-icon",
            "Pola",
            "--platform",
            "macosx",
            "--minimum-deployment-target",
            "26.0",
            "--output-partial-info-plist",
        ])
        .arg(out.join("icon.plist"))
        .status()
        .expect("failed to run asset compiler");
    assert!(result.success(), "failed to compile app icon");

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
    println!("cargo:rerun-if-changed=Cargo.lock");

    let lock: toml::Value =
        toml::from_str(&fs::read_to_string("Cargo.lock").expect("failed to read Cargo.lock"))
            .expect("invalid Cargo.lock");
    let source = lock["package"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"].as_str() == Some("windows-reactor"))
        .and_then(|package| package["source"].as_str())
        .expect("Reactor dependency not found");
    let revision = source
        .rsplit_once('#')
        .expect("Reactor Git revision not found")
        .1;
    let metadata = out.join(revision);
    fs::create_dir_all(&metadata).expect("failed to create metadata directory");
    for name in [
        "Microsoft.Windows.Globalization.winmd",
        "Microsoft.Windows.ApplicationModel.Resources.winmd",
    ] {
        let file = metadata.join(name);
        if !file.exists() {
            let url = format!(
                "https://raw.githubusercontent.com/microsoft/windows-rs/{revision}/crates/tools/reactor/winmd/{name}"
            );
            let result = Command::new("curl")
                .args(["-fsSL", &url])
                .output()
                .expect("failed to download Windows metadata");
            assert!(
                result.status.success(),
                "failed to download Windows metadata: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            fs::write(file, result.stdout).expect("failed to save Windows metadata");
        }
    }
    windows_bindgen::builder()
        .input(metadata)
        .input_default()
        .filter("Microsoft.Windows.Globalization.ApplicationLanguages::put_PrimaryLanguageOverride")
        .minimal()
        .flat()
        .output(out.join("language.rs"))
        .write();

    #[cfg(windows)]
    {
        windows_reactor_setup::as_self_contained();

        winresource::WindowsResource::new()
            .set_icon("assets/pola.ico")
            .compile()
            .expect("failed to embed Windows resources");
    }
}
