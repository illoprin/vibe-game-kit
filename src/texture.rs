use std::{error::Error, fs, path::Path};

use crate::map::MaterialDef;

const CACHE_SIZE: u32 = 16;

pub fn create_material_array(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    materials: &[MaterialDef],
) -> Result<wgpu::Texture, Box<dyn Error>> {
    if materials.is_empty() {
        return Err("map must declare at least one material".into());
    }
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("map material texture array"),
        size: wgpu::Extent3d {
            width: CACHE_SIZE,
            height: CACHE_SIZE,
            depth_or_array_layers: materials.len() as u32,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (layer, material) in materials.iter().enumerate() {
        let rgba = load_or_generate(&material.texture, &material.name)?;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: layer as u32,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(CACHE_SIZE * 4),
                rows_per_image: Some(CACHE_SIZE),
            },
            wgpu::Extent3d {
                width: CACHE_SIZE,
                height: CACHE_SIZE,
                depth_or_array_layers: 1,
            },
        );
    }
    Ok(texture)
}

fn load_or_generate(path: &Path, name: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    if let Ok(text) = fs::read_to_string(path) {
        if let Some((w, h, rgb)) = parse_ppm(&text) {
            if w <= 32 && h <= 32 {
                return Ok(resample(&rgb, w, h));
            }
        }
    }
    let pixels = generate(name);
    write_ppm(path, &pixels)?;
    Ok(pixels
        .chunks_exact(3)
        .flat_map(|p| [p[0], p[1], p[2], 255])
        .collect())
}

fn parse_ppm(text: &str) -> Option<(u32, u32, Vec<u8>)> {
    let tokens: Vec<_> = text
        .lines()
        .flat_map(|l| l.split('#').next().unwrap_or("").split_whitespace())
        .collect();
    if *tokens.first()? != "P3" {
        return None;
    }
    let w: u32 = tokens.get(1)?.parse().ok()?;
    let h: u32 = tokens.get(2)?.parse().ok()?;
    let max: u32 = tokens.get(3)?.parse().ok()?;
    if w == 0 || h == 0 || w > 32 || h > 32 || max == 0 {
        return None;
    }
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for token in tokens.iter().skip(4).take((w * h * 3) as usize) {
        rgb.push((token.parse::<u32>().ok()?.saturating_mul(255) / max).min(255) as u8);
    }
    (rgb.len() == (w * h * 3) as usize).then_some((w, h, rgb))
}

fn resample(rgb: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut rgba = Vec::with_capacity((CACHE_SIZE * CACHE_SIZE * 4) as usize);
    for y in 0..CACHE_SIZE {
        for x in 0..CACHE_SIZE {
            let sx = x * width / CACHE_SIZE;
            let sy = y * height / CACHE_SIZE;
            let index = ((sy * width + sx) * 3) as usize;
            rgba.extend_from_slice(&[rgb[index], rgb[index + 1], rgb[index + 2], 255]);
        }
    }
    rgba
}

fn write_ppm(path: &Path, rgba: &[u8]) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut text = format!("P3\n{CACHE_SIZE} {CACHE_SIZE}\n255\n");
    for pixel in rgba.chunks_exact(3) {
        text.push_str(&format!("{} {} {}\n", pixel[0], pixel[1], pixel[2]));
    }
    fs::write(path, text)?;
    Ok(())
}

fn generate(name: &str) -> Vec<u8> {
    let mut rgb = Vec::with_capacity((CACHE_SIZE * CACHE_SIZE * 3) as usize);
    for y in 0..CACHE_SIZE {
        for x in 0..CACHE_SIZE {
            let n = hash(x, y) as i16 - 4;
            let color = match name {
                "concrete" => {
                    let base = if x % 8 == 0 || y % 8 == 0 {
                        66
                    } else {
                        112 + n * 2
                    };
                    (base, base + 5, base + 8)
                }
                "crate" => {
                    let nail = (x == 2 || x == 13) && y % 8 == 2;
                    if nail {
                        (175, 177, 166)
                    } else if x % 8 == 0 {
                        (63, 39, 26)
                    } else {
                        let g = if (x + y / 4 * 3) % 7 == 0 { 20 } else { 0 };
                        (132 + n * 3 + g, 78 + n * 2 + g, 42 + n + g / 2)
                    }
                }
                "gunmetal" => {
                    let streak = (x * 7 + y * 5) % 21 == 0;
                    let pit = x % 6 == 4 && y % 7 == 2;
                    let base = 42 + n * 2;
                    if streak {
                        (base + 30, base + 34, base + 42)
                    } else if pit {
                        (base - 10, base - 8, base - 4)
                    } else {
                        (base, base + 3, base + 8)
                    }
                }
                "walnut" => {
                    let grain = y % 6 == 1 || (x * 5 + y * 3) % 13 == 0;
                    let (r, g, b) = if grain {
                        (64i16, 40, 22)
                    } else {
                        (108, 66, 38)
                    };
                    (r + n, g + n / 2, b + n / 2)
                }
                _ => {
                    if (x + y) % 8 < 2 {
                        (224, 157, 57)
                    } else {
                        let b = 69 + n * 2;
                        (b, b + 16, b + 25)
                    }
                }
            };
            rgb.extend_from_slice(&[
                color.0.clamp(0, 255) as u8,
                color.1.clamp(0, 255) as u8,
                color.2.clamp(0, 255) as u8,
            ]);
        }
    }
    rgb
}

fn hash(x: u32, y: u32) -> u8 {
    let mut v = x.wrapping_mul(0x45d9_f3b) ^ y.wrapping_mul(0x119d_e1f3);
    v = (v ^ (v >> 16)).wrapping_mul(0x45d9_f3b);
    ((v ^ (v >> 16)) % 9) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn procedural_textures_cache_to_assets() {
        for name in ["gunmetal", "walnut"] {
            let path = Path::new("assets").join(format!("{name}.ppm"));
            let rgba = load_or_generate(&path, name).expect("texture should load or generate");
            assert_eq!(rgba.len(), (CACHE_SIZE * CACHE_SIZE * 4) as usize);
        }
    }

    #[test]
    fn shipped_pickup_and_weapon_textures_are_valid_small_ppm_files() {
        for entry in fs::read_dir("assets").expect("assets should be readable") {
            let path = entry.expect("directory entry should be readable").path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("ppm") {
                continue;
            }
            let text = fs::read_to_string(&path).expect("PPM should be UTF-8 text");
            let (width, height, _) = parse_ppm(&text)
                .unwrap_or_else(|| panic!("{} should contain a valid P3 texture", path.display()));
            assert!(width <= 32 && height <= 32);
        }
    }
}
