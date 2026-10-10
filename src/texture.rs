use std::{error::Error, fs, path::Path};

use crate::map::MaterialDef;

const CACHE_SIZE: u32 = 16;

/// Colours of the magenta checkerboard that stands in for missing textures.
const MISSING_LIGHT: [u8; 3] = [0xd1, 0x1b, 0xb6];
const MISSING_DARK: [u8; 3] = [0x99, 0x11, 0x85];

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
        let rgba = load_texture(&material.texture);
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

/// Loads a 16x16 texture for the material array. Any missing, unreadable or
/// invalid file falls back to the magenta checkerboard so an absent texture is
/// immediately obvious in-game instead of failing the whole scene.
fn load_texture(path: &Path) -> Vec<u8> {
    if let Ok(text) = fs::read_to_string(path) {
        if let Some((w, h, rgb)) = parse_ppm(&text) {
            if w <= 32 && h <= 32 {
                return resample(&rgb, w, h);
            }
        }
    }
    checkerboard()
}

/// Two-tone magenta checkerboard used as the stand-in for missing textures.
fn checkerboard() -> Vec<u8> {
    let mut rgba = Vec::with_capacity((CACHE_SIZE * CACHE_SIZE * 4) as usize);
    for y in 0..CACHE_SIZE {
        for x in 0..CACHE_SIZE {
            let color = if ((x / 8) + (y / 8)) % 2 == 0 {
                MISSING_LIGHT
            } else {
                MISSING_DARK
            };
            rgba.extend_from_slice(&[color[0], color[1], color[2], 255]);
        }
    }
    rgba
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_textures_fall_back_to_checkerboard() {
        let path = Path::new("assets").join("__missing_texture__.ppm");
        let rgba = load_texture(&path);
        assert_eq!(rgba.len(), (CACHE_SIZE * CACHE_SIZE * 4) as usize);
        assert!(!path.exists(), "the fallback must not write a file");
        for pixel in rgba.chunks_exact(4) {
            let color = [pixel[0], pixel[1], pixel[2]];
            assert!(
                color == MISSING_LIGHT || color == MISSING_DARK,
                "unexpected checker colour {color:?}"
            );
            assert_eq!(pixel[3], 255);
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
