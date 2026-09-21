use sha2::{Digest, Sha256};
use std::{env, fs, path::Path};

fn hash_path(root: &Path, path: &Path, digest: &mut Sha256) {
    println!("cargo:rerun-if-changed={}", path.display());
    if path.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for entry in entries {
            hash_path(root, &entry, digest);
        }
    } else {
        let name = path.strip_prefix(root).unwrap().to_str().unwrap();
        let bytes = fs::read(path).unwrap();
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
}

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let root = Path::new(&manifest).parent().unwrap().parent().unwrap();
    let mut digest = Sha256::new();
    for path in [
        "Cargo.toml",
        "Cargo.lock",
        "crates/shared/Cargo.toml",
        "crates/shared/build.rs",
        "crates/server/Cargo.toml",
        "crates/client/Cargo.toml",
        "crates/shared/src",
        "crates/server/src",
        "crates/client/src/lib.rs",
        "assets/stormkeep/built/map.json",
        "assets/stormkeep/built/collision.bin",
    ] {
        hash_path(root, &root.join(path), &mut digest);
    }
    println!(
        "cargo:rustc-env=HOOKRUNNER_SIMULATION_BUILD={:x}",
        digest.finalize()
    );
}
