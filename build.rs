//! Builds kjk's icon into kjk-launcher.exe on Windows.

fn main() {
    println!("cargo:rerun-if-changed=assets/kjk-launcher.rc");
    println!("cargo:rerun-if-changed=assets/kjk-launcher.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/kjk-launcher.rc", embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}
