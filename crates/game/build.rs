#[path = "../../scripts/mip_filter.rs"]
mod mip_filter;
use std::{
    collections::{BTreeSet, hash_map::DefaultHasher},
    fs,
    hash::{Hash, Hasher},
    io::BufWriter,
    path::PathBuf,
};
fn digest(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let built = root.join("assets/stormkeep/built");
    let map = built.join("map.json");
    println!("cargo:rerun-if-changed={}", map.display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("scripts/mip_filter.rs").display()
    );
    let metadata: serde_json::Value = serde_json::from_slice(&fs::read(map).unwrap()).unwrap();
    let mut names = BTreeSet::new();
    for material in metadata["materials"].as_array().unwrap() {
        if material["alpha"].as_bool() == Some(true) || material["blend"].as_bool() == Some(true) {
            continue;
        }
        names.insert(material["texture"].as_str().unwrap());
        if let Some(glow) = material["glow"].as_str() {
            names.insert(glow);
        }
    }
    let output = built.join("mipmaps");
    fs::create_dir_all(&output).unwrap();
    println!("cargo:rerun-if-changed={}", output.display());
    let cache_path = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("mip-cache.json");
    let cache: serde_json::Value = fs::read(&cache_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let mut next_cache = serde_json::Map::new();
    let recipe =
        digest(include_bytes!("build.rs")) ^ digest(include_bytes!("../../scripts/mip_filter.rs"));
    let mut manifest = String::from("pub const BAKED_MIPS: &[(&str, u32, u32, u32)] = &[\n");
    for name in &names {
        let source = built.join("textures").join(name);
        println!("cargo:rerun-if-changed={}", source.display());
        let source_bytes = fs::read(source).unwrap();
        let key = digest(&source_bytes) ^ recipe;
        let previous = &cache[*name];
        let target = output.join(name);
        if previous["input"].as_u64() == Some(key)
            && fs::read(&target)
                .is_ok_and(|bytes| previous["output"].as_u64() == Some(digest(&bytes)))
        {
            manifest.push_str(&format!(
                "({name:?}, {}, {}, {}),\n",
                previous["width"], previous["height"], previous["levels"]
            ));
            next_cache.insert(name.to_string(), previous.clone());
            continue;
        }
        let mut decoder = png::Decoder::new(std::io::Cursor::new(source_bytes));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        pixels.truncate(info.buffer_size());
        let mut data = Vec::with_capacity(info.width as usize * info.height as usize * 4);
        match info.color_type {
            png::ColorType::Rgba => data = pixels,
            png::ColorType::Rgb => {
                for rgb in pixels.chunks_exact(3) {
                    data.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                }
            }
            png::ColorType::Grayscale => {
                for v in pixels {
                    data.extend_from_slice(&[v, v, v, 255]);
                }
            }
            png::ColorType::GrayscaleAlpha => {
                for pair in pixels.chunks_exact(2) {
                    data.extend_from_slice(&[pair[0], pair[0], pair[0], pair[1]]);
                }
            }
            _ => panic!("unsupported PNG color type"),
        }
        let levels =
            mip_filter::append_srgb_mips(&mut data, info.width as usize, info.height as usize);
        // A PNG atlas keeps every level losslessly compressed and works with the
        // existing WebGL2/native PNG loader; no GPU compression format is required.
        let mut height = 0;
        let mut h = info.height;
        for _ in 0..levels {
            height += h;
            h = (h / 2).max(1);
        }
        let mut atlas = vec![0; (info.width * height * 4) as usize];
        let (mut w, mut h, mut y, mut offset) = (info.width, info.height, 0, 0usize);
        for _ in 0..levels {
            for row in 0..h {
                let start = ((y + row) * info.width * 4) as usize;
                let bytes = (w * 4) as usize;
                atlas[start..start + bytes].copy_from_slice(&data[offset..offset + bytes]);
                offset += bytes;
            }
            y += h;
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }
        let temporary = output.join(format!("{name}.tmp-{}", std::process::id()));
        let mut encoder = png::Encoder::new(
            BufWriter::new(fs::File::create(&temporary).unwrap()),
            info.width,
            height,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::High);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&atlas).unwrap();
        writer.finish().unwrap();
        fs::rename(&temporary, &target).unwrap();
        next_cache.insert(
            name.to_string(),
            serde_json::json!({
                "input": key, "output": digest(&fs::read(&target).unwrap()),
                "width": info.width, "height": info.height, "levels": levels,
            }),
        );
        manifest.push_str(&format!(
            "({name:?}, {}, {}, {levels}),\n",
            info.width, info.height
        ));
    }
    // Remove outputs for textures no longer used by this map.
    for entry in fs::read_dir(&output).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file()
            && entry.path().extension().is_some_and(|ext| ext == "png")
            && !names.contains(entry.file_name().to_str().unwrap())
        {
            fs::remove_file(entry.path()).unwrap();
        }
    }
    fs::write(cache_path, serde_json::to_vec(&next_cache).unwrap()).unwrap();
    manifest.push_str("];\n");
    fs::write(
        PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("mip_assets.rs"),
        manifest,
    )
    .unwrap();
}
