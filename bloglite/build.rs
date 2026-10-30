use std::{env, path::PathBuf};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let mut missing = Vec::new();
    for name in [
        "mathml.css",
        "mathml-LICENSE",
        "LibertinusMath-Regular.woff2",
        "LibertinusMath-OFL.txt",
    ] {
        let relative = format!("src/static/vendor/{name}");
        println!("cargo:rerun-if-changed={relative}");
        if !root.join(&relative).is_file() {
            missing.push(relative);
        }
    }
    if !missing.is_empty() {
        eprintln!(
            "Missing static assets:\n  {}\n\nRun `just static-assets` from the repository root, then retry your Cargo command.",
            missing.join("\n  ")
        );
        std::process::exit(1);
    }
}
