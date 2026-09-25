//! Linear-light mipmaps for opaque sRGB map textures.

pub fn append_srgb_mips(data: &mut Vec<u8>, mut width: usize, mut height: usize) -> u32 {
    assert!(width > 0 && height > 0);
    assert_eq!(data.len(), width * height * 4);
    let linear: [f32; 256] = std::array::from_fn(|i| {
        let value = i as f32 / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    });
    let encode = |value: f32| -> u8 {
        let srgb = if value <= 0.0031308 {
            value * 12.92
        } else {
            1.055 * value.powf(1.0 / 2.4) - 0.055
        };
        (srgb * 255.0).round().clamp(0.0, 255.0) as u8
    };
    let mut offset = 0;
    let mut levels = 1;
    while width > 1 || height > 1 {
        let next_width = (width / 2).max(1);
        let next_height = (height / 2).max(1);
        let mut next = Vec::with_capacity(next_width * next_height * 4);
        for y in 0..next_height {
            for x in 0..next_width {
                // Area filtering includes every texel of odd and narrow textures.
                let x0 = x as f32 * width as f32 / next_width as f32;
                let x1 = (x + 1) as f32 * width as f32 / next_width as f32;
                let y0 = y as f32 * height as f32 / next_height as f32;
                let y1 = (y + 1) as f32 * height as f32 / next_height as f32;
                let mut sum = [0.0; 4];
                let area = (x1 - x0) * (y1 - y0);
                for sy in y0.floor() as usize..y1.ceil() as usize {
                    for sx in x0.floor() as usize..x1.ceil() as usize {
                        let weight = (x1.min((sx + 1) as f32) - x0.max(sx as f32))
                            * (y1.min((sy + 1) as f32) - y0.max(sy as f32));
                        let pixel = &data[offset + (sy * width + sx) * 4..][..4];
                        for c in 0..3 {
                            sum[c] += linear[pixel[c] as usize] * weight;
                        }
                        sum[3] += pixel[3] as f32 * weight;
                    }
                }
                next.extend_from_slice(&[
                    encode(sum[0] / area),
                    encode(sum[1] / area),
                    encode(sum[2] / area),
                    (sum[3] / area).round() as u8,
                ]);
            }
        }
        offset = data.len();
        data.extend_from_slice(&next);
        width = next_width;
        height = next_height;
        levels += 1;
    }
    levels
}
