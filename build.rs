fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/pola.ico");
        winresource::WindowsResource::new()
            .set_icon("assets/pola.ico")
            .compile()
            .expect("failed to embed application icon");
    }
}
