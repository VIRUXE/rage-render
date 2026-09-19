# rage-render

Draws the drawables [`rage-formats`](https://github.com/VIRUXE/rage-formats)
parses: a small CPU rasteriser (no GPU, runs under wasm) that renders a
`.ydr`/`.ydd`/`.yft` with its textures from fixed camera angles, contact
sheets of several renders, a bitmap font for labels, and a wasm-exposed
glTF exporter (`convert_to_gltf`).

```toml
[dependencies]
rage-render = "0.1"
```

### Rendering

A small CPU rasterizer (no GPU, runs under wasm) turns a parsed `Drawable`
into a preview image from one of six fixed camera angles:

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

`render_drawable` is the single-view shorthand when only `options.view`
matters.

A fragment is more than its main drawable: wheels, doors and other physics
children are drawables of their own, each placed on the body by a transform.
`Fragment::render_parts` lists them (filling empty wheel slots from the
front/rear wheel mesh and mirroring right-hand wheels, as CodeWalker does)
and `render_parts` frames them together:

```rust
use rage_formats::parse_yft;
use rage_render::{render_parts, RenderOptions, RenderPart, TextureSet, View};

let fragment = parse_yft(&yft_bytes)?;
let parts: Vec<RenderPart<'_>> = fragment.render_parts().into_iter().map(RenderPart::from).collect();
let rendered = render_parts(&parts, &textures, &RenderOptions::default(), &[View::Iso])?;
```

A `RenderPart` carries the drawable, a transform applied to all of its models
and an optional per-bone pose selected by each model's bone index, so any
composite of drawables can be rendered the same way.

Each geometry is drawn the way its shader's RAGE render bucket says: bucket 0
is opaque and ignores alpha, bucket 3 (`cutout`, foliage, fences) is
alpha-tested at half, and buckets 1 and 2 (`*_alpha`, `decal`) are blended
over what is already drawn, after all solid geometry, back to front, without
writing depth. Blending over a transparent background keeps the coverage in
the output alpha.

One line each on two smaller pieces the renderer and CLI build on:
- **Contact sheet**: `compose_sheet`/`sheet_layout` lay out a grid of
  labelled thumbnails (e.g. one render per view, or a batch of extracted
  textures) into a single `RgbaImage`.
- **Bitmap font**: `draw_text`/`text_width` (backed by the `FONT_5X7` glyph
  table) draw simple pixel labels directly onto an `RgbaImage`, with no font
  file or text-shaping dependency.

## License

[Unlicense](LICENSE) — public domain.
