//! Texture lookup for the renderer: decoded RGBA images keyed by name hash.

use std::collections::HashMap;

use image::RgbaImage;

use rage_formats::texture_utils::to_rgba_image;
use rage_formats::rage_joaat;
use rage_formats::ytd::YtdTexture;

/// Decoded textures, in priority layers: a name is resolved against layer 0
/// first, then layer 1, and so on. That lets an embedded texture dictionary
/// shadow a shared one without merging the two.
///
/// Keys are `rage_joaat` of the lowercased texture name.
#[derive(Debug, Default, Clone)]
pub struct TextureSet {
    layers: Vec<HashMap<u32, RgbaImage>>,
}

impl TextureSet {
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    /// Decodes `textures` into a new, lowest-priority layer. Returns the names
    /// of the textures that could not be decoded (they are simply absent).
    pub fn push_layer(&mut self, textures: &[YtdTexture]) -> Vec<String> {
        let mut failed = Vec::new();
        let mut layer = HashMap::with_capacity(textures.len());

        for texture in textures {
            match to_rgba_image(texture) {
                Ok(image) => {
                    layer.insert(rage_joaat(&texture.name.to_lowercase()), image);
                }
                Err(_) => failed.push(texture.name.clone()),
            }
        }

        self.layers.push(layer);
        failed
    }

    /// Makes `name` resolve to `target`'s image from now on, ahead of every
    /// layer — how a vehicle's `_sign_1` livery reference is pointed at
    /// `_sign_3`. False, and nothing changes, when `target` is not held.
    pub fn alias(&mut self, name: &str, target: &str) -> bool {
        let Some(image) = self.get(target).cloned() else { return false };
        if self.layers.first().is_none_or(|layer| !layer.is_empty()) {
            self.layers.insert(0, HashMap::new());
        }
        self.layers[0].insert(rage_joaat(&name.to_lowercase()), image);
        true
    }

    /// The image bound to `name`, searching layers in order.
    pub fn get(&self, name: &str) -> Option<&RgbaImage> {
        let hash = rage_joaat(&name.to_lowercase());
        self.layers.iter().find_map(|layer| layer.get(&hash))
    }

    /// True when no layer holds a decoded texture.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The total number of decoded textures across every layer.
    pub fn len(&self) -> usize {
        self.layers.iter().map(HashMap::len).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texture(name: &str, rgba: [u8; 4]) -> YtdTexture {
        YtdTexture {
            name: name.to_string(),
            name_hash: rage_joaat(&name.to_lowercase()),
            width: 1,
            height: 1,
            depth: 1,
            format: rage_formats::ytd::TextureFormat::A8B8G8R8,
            levels: 1,
            stride: 4,
            pixel_data: rgba.to_vec(),
        }
    }

    /// An alias points a name at another texture's image, ahead of every
    /// layer, and a target the set does not hold changes nothing.
    #[test]
    fn alias_points_a_name_at_another_texture() {
        let mut set = TextureSet::new();
        set.push_layer(&[texture("police_sign_1", [255, 0, 0, 255]), texture("police_sign_3", [0, 0, 255, 255])]);
        assert!(!set.alias("police_sign_1", "police_sign_9"));
        assert_eq!(set.get("police_sign_1").unwrap().get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert!(set.alias("POLICE_SIGN_1", "police_sign_3"));
        assert_eq!(set.get("police_sign_1").unwrap().get_pixel(0, 0).0, [0, 0, 255, 255]);
        assert_eq!(set.get("police_sign_3").unwrap().get_pixel(0, 0).0, [0, 0, 255, 255]);
        assert_eq!(set.len(), 3);
        // A later layer never shadows the alias.
        set.push_layer(&[texture("police_sign_1", [0, 255, 0, 255])]);
        assert_eq!(set.get("police_sign_1").unwrap().get_pixel(0, 0).0, [0, 0, 255, 255]);
    }
}
