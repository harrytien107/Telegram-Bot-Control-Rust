use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=assets/app.ico");

    if env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let res = out_dir.join("app.res");
    let status = Command::new("rc.exe")
        .arg("/nologo")
        .arg(format!("/fo{}", res.display()))
        .arg("app.rc")
        .status()
        .expect("failed to run rc.exe; build from the existing MSVC developer environment");
    assert!(status.success(), "rc.exe failed");
    println!("cargo:rustc-link-arg={}", res.display());
}
