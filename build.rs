// SPDX-License-Identifier: GPL-3.0-or-later
//! Takes your own icon (assets/icon.png) into the program when it is there;
//! otherwise the drawn ChemChron logo is used (src/icon.rs).
fn main() {
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("custom_icon.png");
    let own = std::path::Path::new("assets/icon.png");
    println!("cargo:rerun-if-changed=assets/icon.png");
    match std::fs::read(own) {
        Ok(bytes) => std::fs::write(&out, bytes).unwrap(),
        Err(_) => std::fs::write(&out, []).unwrap(),
    }
}
