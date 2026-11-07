//! Binary glTF 2.0 (`.glb`) export of the same geometry the rasterizer
//! draws: every part placed, every texture resolved through the caller's
//! [`TextureSet`], tint palettes as vertex colours, vehicle paint as a
//! colour factor and each shader's render bucket as the alpha mode.
//!
//! GTA is Z-up with props facing -Y and vehicles facing +Y; glTF is Y-up
//! with the front along +Z. The export rotates (never mirrors) the model so
//! it stands upright and faces +Z either way.

use std::collections::HashMap;
use std::io::Cursor;

use anyhow::{Context, Result};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, RgbaImage};

use rage_formats::ydd::LodLevel;
use crate::render::mesh::{self, BlendMode, PreparedGeometry};
use crate::render::{is_vehicle_paint_shader, Facing, RenderPart, RenderReport, TextureSet};

/// What the export needs besides the model and its textures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlbOptions {
    pub lod: LodLevel,
    pub facing: Facing,
    /// Body colour for `vehicle_paint*.sps` geometry, as in `RenderOptions::paint`.
    pub paint: Option<[u8; 3]>,
    /// Longest texture side kept; larger textures are downscaled. 0 keeps every size.
    pub max_texture: u32,
    /// JPEG quality (1-100) for textures drawn opaque; alpha-tested and blended ones stay PNG.
    pub jpeg_quality: u8,
}

impl Default for GlbOptions {
    fn default() -> Self {
        Self { lod: LodLevel::High, facing: Facing::Auto, paint: None, max_texture: 1024, jpeg_quality: 85 }
    }
}

/// The flat grey the rasterizer draws untextured geometry in (153/255 sRGB), as a linear factor.
const UNTEXTURED_LINEAR: f32 = 0.318;

/// Decodes the parts like `render_parts` does and reports what was found,
/// without encoding anything: cheap enough to call before deciding whether
/// more texture dictionaries are needed.
pub fn glb_report(parts: &[RenderPart<'_>], tex: &TextureSet, o: &GlbOptions) -> RenderReport {
    prepare_all(parts, tex, o).1
}

/// Exports the parts as one binary glTF with embedded textures, and the
/// report of what was drawn (missing textures, triangles, LOD).
pub fn export_glb(parts: &[RenderPart<'_>], tex: &TextureSet, o: &GlbOptions) -> Result<(Vec<u8>, RenderReport)> {
    let (geometries, report, forward) = prepare_all(parts, tex, o);
    let glb = write_glb(&geometries, forward, o)?;
    Ok((glb, report))
}

fn prepare_all<'a>(parts: &[RenderPart<'_>], tex: &'a TextureSet, o: &GlbOptions) -> (Vec<PreparedGeometry<'a>>, RenderReport, f32) {
    let mut report = RenderReport::default();
    let mut geometries = Vec::new();
    let mut is_vehicle = false;
    for part in parts {
        let d = part.drawable;
        let Some(lod) = d.lod(o.lod).filter(|lod| !lod.models.is_empty()).or_else(|| d.best_lod()) else { continue };
        report.lod.get_or_insert(lod.level);
        geometries.extend(mesh::prepare(d, lod, &part.transform, part.bone_transforms, tex, o.paint, part.diffuse_override, &mut report));
        if o.facing == Facing::Auto && !is_vehicle {
            is_vehicle = lod.models.iter().flat_map(|m| m.geometries.iter()).filter_map(|g| d.shader(g.shader_id)).any(|s| is_vehicle_paint_shader(s.file_name_hash));
        }
    }
    let forward = match o.facing {
        Facing::PositiveY => 1.0,
        Facing::NegativeY => -1.0,
        Facing::Auto => if is_vehicle { 1.0 } else { -1.0 },
    };
    report.faces_positive_y = forward > 0.0;
    report.missing_textures.sort();
    report.missing_textures.dedup();
    (geometries, report, forward)
}

/// GTA (Z-up) to glTF (Y-up), turned so the model's front lands on +Z. Both
/// mappings are proper rotations, so triangle winding is unchanged.
fn to_gltf(v: [f32; 3], forward: f32) -> [f32; 3] {
    let [x, y, z] = v;
    if forward > 0.0 { [-x, z, y] } else { [x, z, -y] }
}

/// The binary chunk, with one buffer view per attribute or index list.
struct Bin {
    data: Vec<u8>,
    views: json::JsonValue,
    accessors: json::JsonValue,
}

impl Bin {
    fn align(&mut self) {
        while self.data.len() % 4 != 0 {
            self.data.push(0);
        }
    }

    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        self.align();
        let offset = self.data.len();
        self.data.extend_from_slice(bytes);
        let mut view = json::object! { "buffer": 0, "byteOffset": offset, "byteLength": bytes.len() };
        if let Some(target) = target {
            view["target"] = target.into();
        }
        let _ = self.views.push(view);
        self.views.len() - 1
    }

    fn floats(&mut self, values: &[f32], width: usize, kind: &str, bounds: bool) -> usize {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(34962));
        let mut accessor = json::object! { "bufferView": view, "componentType": 5126, "count": values.len() / width, "type": kind };
        if bounds {
            let mut min = vec![f32::INFINITY; width];
            let mut max = vec![f32::NEG_INFINITY; width];
            for chunk in values.chunks_exact(width) {
                for (i, v) in chunk.iter().enumerate() {
                    min[i] = min[i].min(*v);
                    max[i] = max[i].max(*v);
                }
            }
            accessor["min"] = min.into();
            accessor["max"] = max.into();
        }
        let _ = self.accessors.push(accessor);
        self.accessors.len() - 1
    }

    fn indices(&mut self, indices: &[u32]) -> usize {
        let bytes: Vec<u8> = indices.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(34963));
        let _ = self.accessors.push(json::object! { "bufferView": view, "componentType": 5125, "count": indices.len(), "type": "SCALAR" });
        self.accessors.len() - 1
    }
}

fn has_alpha(image: &RgbaImage) -> bool {
    image.pixels().any(|p| p.0[3] < 255)
}

/// The texture downscaled to `max` and encoded: JPEG when it is drawn
/// opaque, PNG when its alpha matters.
fn encode_texture(image: &RgbaImage, keep_alpha: bool, o: &GlbOptions) -> Result<(Vec<u8>, &'static str)> {
    let (w, h) = image.dimensions();
    let longest = w.max(h);
    let scaled;
    let source = if o.max_texture > 0 && longest > o.max_texture {
        let nw = ((w as u64 * o.max_texture as u64) / longest as u64).max(1) as u32;
        let nh = ((h as u64 * o.max_texture as u64) / longest as u64).max(1) as u32;
        scaled = image::imageops::resize(image, nw, nh, FilterType::Triangle);
        &scaled
    } else {
        image
    };
    let mut out = Vec::new();
    if keep_alpha {
        DynamicImage::ImageRgba8(source.clone()).write_to(&mut Cursor::new(&mut out), ImageFormat::Png).context("png encode")?;
        Ok((out, "image/png"))
    } else {
        let rgb = DynamicImage::ImageRgba8(source.clone()).to_rgb8();
        JpegEncoder::new_with_quality(&mut out, o.jpeg_quality.clamp(1, 100)).encode_image(&rgb).context("jpeg encode")?;
        Ok((out, "image/jpeg"))
    }
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn write_glb(geometries: &[PreparedGeometry<'_>], forward: f32, o: &GlbOptions) -> Result<Vec<u8>> {
    let mut bin = Bin { data: Vec::new(), views: json::JsonValue::new_array(), accessors: json::JsonValue::new_array() };
    let mut images = json::JsonValue::new_array();
    let mut textures = json::JsonValue::new_array();
    let mut materials = json::JsonValue::new_array();
    let mut primitives = json::JsonValue::new_array();
    // Keyed by the decoded image's address: the same RgbaImage is shared by every geometry that names it.
    let mut texture_ids: HashMap<(usize, bool), usize> = HashMap::new();
    let mut material_ids: HashMap<(Option<usize>, u8, [u32; 3]), usize> = HashMap::new();

    for g in geometries {
        let blend_code = match g.blend {
            BlendMode::Opaque => 0u8,
            BlendMode::Cutout => 1,
            BlendMode::Blend | BlendMode::Decal => 2,
        };
        let texture = match g.texture {
            Some(image) => {
                let keep_alpha = blend_code != 0 && has_alpha(image);
                let key = (image as *const RgbaImage as usize, keep_alpha);
                let id = match texture_ids.get(&key) {
                    Some(id) => *id,
                    None => {
                        let (bytes, mime) = encode_texture(image, keep_alpha, o)?;
                        let view = bin.view(&bytes, None);
                        let _ = images.push(json::object! { "bufferView": view, "mimeType": mime });
                        let _ = textures.push(json::object! { "source": images.len() - 1, "sampler": 0 });
                        texture_ids.insert(key, textures.len() - 1);
                        textures.len() - 1
                    }
                };
                Some(id)
            }
            None => None,
        };
        let tint = g.tint.unwrap_or([1.0, 1.0, 1.0]);
        let tint_key = tint.map(f32::to_bits);
        let material = match material_ids.get(&(texture, blend_code, tint_key)) {
            Some(id) => *id,
            None => {
                let base = if texture.is_some() { 1.0 } else { UNTEXTURED_LINEAR };
                let factor = [base * srgb_to_linear(tint[0]), base * srgb_to_linear(tint[1]), base * srgb_to_linear(tint[2]), 1.0];
                let mut pbr = json::object! { "baseColorFactor": factor.to_vec(), "metallicFactor": 0.0, "roughnessFactor": 0.9 };
                if let Some(t) = texture {
                    pbr["baseColorTexture"] = json::object! { "index": t };
                }
                let mut m = json::object! { "pbrMetallicRoughness": pbr, "doubleSided": true };
                match blend_code {
                    1 => {
                        m["alphaMode"] = "MASK".into();
                        m["alphaCutoff"] = 0.5.into();
                    }
                    2 => m["alphaMode"] = "BLEND".into(),
                    _ => {}
                }
                let _ = materials.push(m);
                material_ids.insert((texture, blend_code, tint_key), materials.len() - 1);
                materials.len() - 1
            }
        };

        let mut positions = Vec::with_capacity(g.verts.len() * 3);
        let mut normals = Vec::with_capacity(g.verts.len() * 3);
        let mut uvs = Vec::with_capacity(g.verts.len() * 2);
        for v in &g.verts {
            positions.extend(to_gltf([v.position.x, v.position.y, v.position.z], forward));
            let n = to_gltf([v.normal.x, v.normal.y, v.normal.z], forward);
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            normals.extend(if len > 1e-6 { n.map(|c| c / len) } else { [0.0, 1.0, 0.0] });
            uvs.extend([v.texcoord0.x, v.texcoord0.y]);
        }
        let mut attributes = json::object! { "POSITION": bin.floats(&positions, 3, "VEC3", true) };
        if g.has_normals {
            attributes["NORMAL"] = bin.floats(&normals, 3, "VEC3", false).into();
        }
        if texture.is_some() {
            attributes["TEXCOORD_0"] = bin.floats(&uvs, 2, "VEC2", false).into();
        }
        if g.vertex_tint.len() == g.verts.len() && !g.vertex_tint.is_empty() {
            let colors: Vec<f32> = g.vertex_tint.iter().flat_map(|c| c.map(srgb_to_linear)).collect();
            attributes["COLOR_0"] = bin.floats(&colors, 3, "VEC3", false).into();
        }
        let indices = bin.indices(&g.indices);
        let _ = primitives.push(json::object! { "attributes": attributes, "indices": indices, "material": material, "mode": 4 });
    }
    bin.align();

    let mut root = json::object! {
        "asset": { "version": "2.0", "generator": concat!("rage-render ", env!("CARGO_PKG_VERSION")) },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{}],
        "buffers": [{ "byteLength": bin.data.len() }],
    };
    root["bufferViews"] = bin.views;
    root["accessors"] = bin.accessors;
    if !primitives.is_empty() {
        root["nodes"][0]["mesh"] = 0.into();
        let mut mesh = json::JsonValue::new_object();
        mesh["primitives"] = primitives;
        root["meshes"] = json::array![mesh];
    }
    if !materials.is_empty() {
        root["materials"] = materials;
    }
    if !textures.is_empty() {
        root["textures"] = textures;
        root["images"] = images;
        // Repeat wrapping, linear filtering with mipmaps, as the game samples.
        root["samplers"] = json::array![{ "magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497 }];
    }
    if bin.data.is_empty() {
        root.remove("buffers");
        root.remove("bufferViews");
        root.remove("accessors");
    }

    let mut text = root.dump().into_bytes();
    while text.len() % 4 != 0 {
        text.push(b' ');
    }
    let total = 12 + 8 + text.len() + if bin.data.is_empty() { 0 } else { 8 + bin.data.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    if !bin.data.is_empty() {
        out.extend_from_slice(&(bin.data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin.data);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Props (front -Y) and vehicles (front +Y) both end up upright and facing +Z.
    #[test]
    fn axes_turn_the_front_to_positive_z() {
        assert_eq!(to_gltf([0.0, -1.0, 0.0], -1.0), [0.0, 0.0, 1.0]);
        assert_eq!(to_gltf([0.0, 1.0, 0.0], 1.0), [0.0, 0.0, 1.0]);
        assert_eq!(to_gltf([0.0, 0.0, 1.0], -1.0), [0.0, 1.0, 0.0]);
        assert_eq!(to_gltf([0.0, 0.0, 1.0], 1.0), [0.0, 1.0, 0.0]);
    }

    #[test]
    fn an_empty_model_is_still_a_valid_glb() {
        let glb = write_glb(&[], -1.0, &GlbOptions::default()).unwrap();
        assert_eq!(&glb[0..4], b"glTF");
        assert_eq!(u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize, glb.len());
        let len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let root = json::parse(std::str::from_utf8(&glb[20..20 + len]).unwrap().trim_end()).unwrap();
        assert_eq!(root["asset"]["version"], "2.0");
        assert!(root["meshes"].is_null());
    }
}
