//! Load precomputed mip pixels. Filtering runs only in the build script.
include!(concat!(env!("OUT_DIR"), "/mip_assets.rs"));

pub fn unpack(atlas: &[u8], width: u32, height: u32, levels: u32) -> Vec<u8> {
    let (mut w, mut h, mut y) = (width as usize, height as usize, 0usize);
    let bytes = (0..levels)
        .map(|level| ((width >> level).max(1) as usize) * ((height >> level).max(1) as usize) * 4)
        .sum();
    let mut data = Vec::with_capacity(bytes);
    for _ in 0..levels {
        for row in 0..h {
            let start = ((y + row) * width as usize) * 4;
            data.extend_from_slice(&atlas[start..start + w * 4]);
        }
        y += h;
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    assert_eq!(
        atlas.len(),
        y * width as usize * 4,
        "invalid baked mip atlas"
    );
    data
}
