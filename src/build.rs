fn main() {
    println!("cargo:rerun-if-changed=icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest_dir =
        std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo should provide CARGO_MANIFEST_DIR");
    let icon_path = std::path::Path::new(&manifest_dir).join("icon.ico");
    let icon_path = icon_path
        .to_str()
        .expect("icon path must be valid UTF-8 for winres");

    let mut resources = winres::WindowsResource::new();
    resources.set_icon(icon_path);
    resources
        .compile()
        .expect("failed to embed icon.ico; check that a Windows resource compiler is installed");

    // On GNU targets winres puts the resources in a static archive containing
    // no referenced symbols. The linker can silently omit that archive member,
    // so pass the resource object directly to the binary linker instead.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") {
        let out_dir = std::env::var_os("OUT_DIR").expect("Cargo should provide OUT_DIR");
        let resource_object = std::path::Path::new(&out_dir).join("resource.o");
        println!("cargo:rustc-link-arg-bins={}", resource_object.display());
    }
}
