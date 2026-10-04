//! SVGs are parsed once and rasterised in zoom/DPI buckets. Both the source
//! cache and uploaded textures are bounded, shared by canvas and palette.

use egui::{
    Align2, Color32, ColorImage, FontId, Id, Painter, Rect, Stroke, StrokeKind, TextureHandle,
};
use resvg::{tiny_skia, usvg};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const MAX_SOURCES: usize = 128;
const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
const MAX_TEXTURE_BYTES: usize = 32 * 1024 * 1024;
const MAX_SIDE: u32 = 2048;

#[derive(Default)]
struct IconCache {
    sources: HashMap<usize, Source>,
    tick: u64,
}

struct Source {
    // Keeping the owner prevents an address being reused for another SVG.
    svg: Arc<str>,
    tree: Option<usvg::Tree>,
    textures: HashMap<u32, Texture>,
    last_used: u64,
}

struct Texture {
    handle: Option<TextureHandle>,
    bytes: usize,
    last_used: u64,
}

fn svg_options() -> usvg::Options<'static> {
    usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver {
            // Embedded resources are self-contained; files and URLs are refused.
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    }
}

fn bucket(rect: Rect, pixels_per_point: f32) -> u32 {
    ((rect.width().max(rect.height()) * pixels_per_point)
        .ceil()
        .clamp(16.0, MAX_SIDE as f32) as u32)
        .next_power_of_two()
}

/// Convert resvg's already-premultiplied bytes directly: multiplying twice
/// would darken translucent provider artwork and antialiased edges.
fn rasterize(tree: &usvg::Tree, side: u32) -> Option<ColorImage> {
    let size = tree.size();
    let scale = side as f32 / size.width().max(size.height());
    let width = (size.width() * scale).ceil().max(1.0) as u32;
    let height = (size.height() * scale).ceil().max(1.0) as u32;
    let mut pixels = tiny_skia::Pixmap::new(width, height)?;
    resvg::render(
        tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixels.as_mut(),
    );
    Some(ColorImage::from_rgba_premultiplied(
        [width as usize, height as usize],
        pixels.data(),
    ))
}

fn fitted(rect: Rect, size: usvg::Size) -> Rect {
    let scale = (rect.width() / size.width()).min(rect.height() / size.height());
    Rect::from_center_size(
        rect.center(),
        egui::vec2(size.width() * scale, size.height() * scale),
    )
}

impl IconCache {
    fn texture(
        &mut self,
        ctx: &egui::Context,
        svg: &Arc<str>,
        side: u32,
    ) -> Option<(TextureHandle, usvg::Size)> {
        if svg.len() > MAX_SOURCE_BYTES {
            return None;
        }
        self.tick += 1;
        let tick = self.tick;
        let key = svg.as_ptr() as usize;
        let source = self.sources.entry(key).or_insert_with(|| Source {
            svg: svg.clone(),
            tree: usvg::Tree::from_str(svg, &svg_options()).ok(),
            textures: HashMap::new(),
            last_used: tick,
        });
        source.last_used = tick;
        // None is retained too, so invalid SVG is not parsed every frame.
        let result = source.tree.as_ref().and_then(|tree| {
            let texture = source.textures.entry(side).or_insert_with(|| {
                let image = rasterize(tree, side);
                let bytes = image.as_ref().map_or(0, |image| image.pixels.len() * 4);
                Texture {
                    handle: image.map(|image| {
                        ctx.load_texture("cloud icon", image, egui::TextureOptions::LINEAR)
                    }),
                    bytes,
                    last_used: tick,
                }
            });
            texture.last_used = tick;
            Some((texture.handle.clone()?, tree.size()))
        });
        self.trim(key, side);
        result
    }

    fn trim(&mut self, current: usize, side: u32) {
        while self.sources.len() > MAX_SOURCES
            || self.sources.values().map(|s| s.svg.len()).sum::<usize>() > MAX_SOURCE_BYTES
        {
            let oldest = self
                .sources
                .iter()
                .filter(|(key, _)| **key != current)
                .min_by_key(|(_, source)| source.last_used)
                .map(|(key, _)| *key);
            let Some(oldest) = oldest else { break };
            self.sources.remove(&oldest);
        }
        while self
            .sources
            .values()
            .flat_map(|s| s.textures.values())
            .map(|texture| texture.bytes)
            .sum::<usize>()
            > MAX_TEXTURE_BYTES
        {
            let oldest = self
                .sources
                .iter()
                .flat_map(|(key, source)| {
                    source
                        .textures
                        .iter()
                        .map(move |(bucket, texture)| (*key, *bucket, texture.last_used))
                })
                .filter(|(key, bucket, _)| *key != current || *bucket != side)
                .min_by_key(|(_, _, last_used)| *last_used);
            let Some((key, bucket, _)) = oldest else {
                break;
            };
            self.sources
                .get_mut(&key)
                .expect("cached source")
                .textures
                .remove(&bucket);
        }
    }
}

pub(super) fn paint(painter: &Painter, rect: Rect, svg: &Arc<str>, opacity: f64) {
    if !rect.is_finite() || rect.width() <= 0.0 || rect.height() <= 0.0 || opacity <= 0.0 {
        return;
    }
    let ctx = painter.ctx();
    let cache = ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<Mutex<IconCache>>>(Id::new("bp-cloud-icon-cache"))
            .clone()
    });
    let texture = cache.lock().expect("cloud icon cache").texture(
        ctx,
        svg,
        bucket(rect, ctx.pixels_per_point()),
    );
    if let Some((texture, size)) = texture {
        painter.image(
            texture.id(),
            fitted(rect, size),
            Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            Color32::WHITE.gamma_multiply(opacity.clamp(0.0, 1.0) as f32),
        );
    } else {
        let colour = Color32::GRAY.gamma_multiply(opacity.clamp(0.0, 1.0) as f32);
        painter.rect_stroke(rect, 4.0, Stroke::new(1.0, colour), StrokeKind::Inside);
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "SVG",
            FontId::proportional(12.0),
            colour,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Original test art: transparent square with an asymmetric two-colour mark.
    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10" viewBox="0 0 20 10"><rect width="10" height="10" fill="#ff0000"/><rect x="10" width="10" height="10" fill="#0080ff" fill-opacity="0.5"/></svg>"##;

    #[test]
    fn raster_keeps_provider_colours_and_premultiplied_alpha() {
        let tree = usvg::Tree::from_str(SVG, &svg_options()).unwrap();
        let image = rasterize(&tree, 32).unwrap();
        assert_eq!(image.size, [32, 16]);
        assert_eq!(image.pixels[4 * 32 + 4], Color32::RED);
        assert_eq!(image.pixels[4 * 32 + 28].to_array(), [0, 64, 128, 128]);
    }

    #[test]
    fn contain_meets_without_cropping_or_stretching() {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0));
        let fit = fitted(rect, usvg::Size::from_wh(20.0, 10.0).unwrap());
        assert_eq!(
            fit,
            Rect::from_min_max(egui::pos2(0.0, 25.0), egui::pos2(100.0, 75.0))
        );
        assert_eq!(bucket(rect, 1.0), 128);
        assert_eq!(bucket(rect, 2.0), 256);
    }

    #[test]
    fn repeated_draw_reuses_parse_and_upload_and_changes_dpi_bucket() {
        let ctx = egui::Context::default();
        let mut cache = IconCache::default();
        let svg: Arc<str> = SVG.into();
        let (first, _) = cache.texture(&ctx, &svg, 64).unwrap();
        let (again, _) = cache.texture(&ctx, &svg, 64).unwrap();
        assert_eq!(first.id(), again.id());
        assert_eq!(cache.sources.len(), 1);
        let (larger, _) = cache.texture(&ctx, &svg, 128).unwrap();
        assert_ne!(first.id(), larger.id());
        let invalid: Arc<str> = "invalid SVG".into();
        assert!(cache.texture(&ctx, &invalid, 64).is_none());
        assert!(cache.texture(&ctx, &invalid, 128).is_none());
        assert_eq!(cache.sources.len(), 2);
        assert!(cache.sources[&(invalid.as_ptr() as usize)].tree.is_none());
    }

    #[test]
    fn external_images_are_never_resolved() {
        let options = svg_options();
        assert!((options.image_href_resolver.resolve_string)("/tmp/icon.svg", &options).is_none());
        assert!(
            (options.image_href_resolver.resolve_string)("https://example.com/icon.svg", &options)
                .is_none()
        );
    }

    #[test]
    fn cache_evicts_old_sources_and_raster_buckets() {
        let ctx = egui::Context::default();
        let mut cache = IconCache::default();
        for n in 0..(MAX_SOURCES + 10) {
            let svg = Arc::<str>::from(format!("invalid SVG {n}"));
            assert!(cache.texture(&ctx, &svg, 64).is_none());
        }
        assert_eq!(cache.sources.len(), MAX_SOURCES);

        let svg: Arc<str> = SVG.into();
        // Five 2048x1024 textures exceed the 32 MiB budget and evict the
        // oldest buckets while retaining the currently drawn one.
        let key = svg.as_ptr() as usize;
        cache.sources.insert(
            key,
            Source {
                svg: svg.clone(),
                tree: None,
                textures: (1..=5)
                    .map(|n| {
                        (
                            n,
                            Texture {
                                handle: None,
                                bytes: 2048 * 1024 * 4,
                                last_used: u64::from(n),
                            },
                        )
                    })
                    .collect(),
                last_used: cache.tick + 1,
            },
        );
        cache.trim(key, 5);
        assert_eq!(cache.sources[&key].textures.len(), 4);
        assert!(!cache.sources[&key].textures.contains_key(&1));
        assert!(cache.sources[&key].textures.contains_key(&5));
    }
}
