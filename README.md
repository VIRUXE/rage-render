# rage-render

Draws the drawables [`rage-formats`](https://github.com/VIRUXE/rage-formats)
parses. A small CPU rasteriser with no GPU and no windowing, which is why it
runs in a CLI, in CI, and under wasm: renders of a `.ydr`, `.ydd` or `.yft`
with its textures from fixed camera angles, contact sheets of several
renders, a bitmap font for labels, and a wasm-exposed glTF exporter.

[`rage-cli`](https://github.com/VIRUXE/rage-cli)'s `screenshot` and
`textures --sheet` are thin wrappers over this crate.

```toml
[dependencies]
rage-render = "0.1"
```

## Pipeline

```
 Drawable (rage-formats)          TextureSet
   models, geometry, shaders        layers of YtdTexture, first hit wins
          |                              |
          v                              v
   RenderOptions: view, size, background, paint, cull, lod, vertex colours
          |
          v
   camera_for()   frames the model on its bounds (or its dominant cluster)
          |
          v
   rasteriser     per geometry, in render-bucket order:
                   opaque -> alpha-tested cutouts -> blended, back to front
          |
          v
   RgbaImage + RenderReport (triangles drawn, textures missing, geometries with no diffuse)
```

`render_views` renders one drawable from several `View`s; `render_drawable`
is the single-view shorthand; `render_parts` renders several placed
drawables into one image, which is how a vehicle's body and wheels become
one picture. `compose_sheet` tiles labelled images into a grid, and
`draw_text` puts a 5x7 bitmap font on any image without a font file.

## Examples

### Render a drawable

```rust
use rage_render::{render_views, RenderOptions, TextureSet, View};

let mut textures = TextureSet::new();
textures.push_layer(&embedded_textures); // e.g. from the drawable's own shader group
textures.push_layer(&shared_textures);   // lower-priority fallback layer

// `paint` tints geometries drawn with a vehicle_paint*.sps shader: the YFT
// carries no body colour, the game applies it at runtime from carcols.
let options = RenderOptions { view: View::Iso, paint: Some([200, 30, 30]), ..Default::default() };
let rendered = render_views(&drawable, &textures, &options, &View::ALL)?;
for (view, image, report) in rendered {
    println!("{view}: {} triangle(s), {} missing texture(s)", report.triangles, report.missing_textures.len());
    image.save(format!("{view}.png"))?;
}
```

Textures the drawable asks for but no layer supplies are drawn flat grey and
listed in `report.missing_textures`, so the caller knows which dictionary to
add. Geometries whose shader names no diffuse texture at all (lights, glass)
are counted separately in `geometries_without_diffuse`.

### Fragments: body plus parts

A fragment is more than its main drawable: wheels, doors and other physics
children are drawables of their own, each placed on the body by a transform.
`Fragment::render_parts` lists them, filling empty wheel slots from the
front and rear wheel meshes and mirroring right-hand wheels as CodeWalker
does, and `render_parts` frames them together:

```rust
use rage_formats::parse_yft;
use rage_render::{render_parts, RenderOptions, RenderPart, TextureSet, View};

let fragment = parse_yft(&yft_bytes)?;
let parts: Vec<RenderPart<'_>> = fragment.render_parts().into_iter().map(RenderPart::from).collect();
let rendered = render_parts(&parts, &textures, &RenderOptions::default(), &[View::Iso])?;
```

A `RenderPart` carries a drawable, a transform applied to all of its models
and an optional per-bone pose selected by each model's bone index, so any
composite of drawables can be rendered the same way.

### Materials

Each geometry is drawn the way its shader's RAGE render bucket says: bucket
0 is opaque and ignores alpha, bucket 3 (`cutout`, foliage, fences) is
alpha-tested at half, and buckets 1 and 2 (`*_alpha`, `decal`) are blended
over what is already drawn, after all solid geometry, back to front, without
writing depth. Blending over a transparent background keeps the coverage in
the output alpha, so a render with `background: None` drops onto any page.

Back-face culling is off by default because many GTA surfaces are
single-sided planes; `cull: true` keeps counter-clockwise front faces, which
is the convention retail drawables use, mirrored wheels included.

### Contact sheets and labels

```rust
use rage_render::{compose_sheet, draw_text, SheetItem, SheetOptions};

let sheet = compose_sheet(&items, &SheetOptions { cell: 256, ..Default::default() })?;
draw_text(&mut image, 8, 8, "front", 2, [255, 255, 255, 255]);
```

`compose_sheet` lays out a grid of labelled thumbnails, one render per view
or a batch of extracted textures, with a checkerboard behind anything that
has alpha. `draw_text` and `text_width` use the `FONT_5X7` glyph table
(Adafruit's `glcdfont` shapes) at any integer scale.

### glTF for the browser

`convert_to_gltf(ydd_bytes, ytd_bytes)` returns a GLB with a
`{"format": "unified_v2"}` envelope, and is exported through `wasm-bindgen`
so a web page can turn a drawable into something a three.js viewer loads.
The crate builds as `cdylib` and `rlib` for that reason.

## Where things live

```
src/
  render/mod.rs      render_views / render_parts / RenderOptions / RenderReport / TextureSet
  render/camera.rs   framing: fit the view to the bounds, or to the dominant geometry cluster
  render/cluster.rs  finding that cluster when a model is really several far-apart pieces
  render/mesh.rs     drawable geometry -> prepared triangles with blend mode per geometry
  render/raster.rs   the triangle rasteriser, depth buffer, alpha test and blending
  render/textures.rs texture lookup through the layered TextureSet
  sheet.rs           contact sheets
  font.rs            5x7 bitmap font
  wasm.rs            glTF export and the wasm entry point
```

## License

[Unlicense](LICENSE), public domain.
