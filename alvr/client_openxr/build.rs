use std::{env, path::PathBuf};

fn main() {
    if env::var_os("CARGO_CFG_TARGET_OS").as_deref() != Some("android".as_ref()) {
        return;
    }

    let loader_directory =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"))
            .join("../../deps/android_openxr/armeabi-v7a");

    println!(
        "cargo:rustc-link-search=native={}",
        loader_directory.display()
    );
    println!("cargo:rerun-if-changed={}", loader_directory.display());
}
