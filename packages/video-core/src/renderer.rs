//! One deterministic overlay path for live preview, timeline playback and export.
//! Asset preparation is explicit. Rendering does not read files or open devices.
use crate::{
    HighlightStyle, Orientation, OutputSize, OverlayContent, OverlayKind, OverlayMotion,
    ProjectStore, Result, Studio,
};
use catalyst_core::local::{self, FileMode, PathPolicy};
use cosmic_text::{
    Attrs, Buffer, Color, Family, FontSystem, LineIter, Metrics, Shaping, SwashCache, Weight, Wrap,
};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    io::{Cursor, Read},
    path::PathBuf,
    sync::Arc,
};
use tiny_skia::{FillRule, IntSize, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Transform};

pub const INTER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
pub const INTER_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");
const INK: [u8; 4] = [16, 19, 26, 255];
const BODY: [u8; 4] = [59, 63, 73, 255];
const VIOLET: [u8; 4] = [120, 96, 181, 255];
const IVORY: [u8; 4] = [247, 246, 242, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
const MARKER: [u8; 4] = [219, 207, 246, 255];
const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_IMAGES: usize = 16;
const MAX_ACTIVE: usize = 9;
const MAX_LAYOUTS: usize = 18;

/// Top-down row-major, STRAIGHT (nonpremultiplied) RGBA. Feed unchanged to egui's
/// from_rgba_unmultiplied or FFmpeg's rgba input. Transparent pixels are all zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderOptions {
    pub reduced_motion: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct ActiveOverlay<'a> {
    pub content: &'a OverlayContent,
    pub elapsed_ms: u64,
    pub duration_ms: u64,
}

/// Explicit decoded image cache, confined to one project until clear(). Failed
/// preparation never replaces an already loaded asset. No per-frame file reads.
#[derive(Default)]
pub struct OverlayAssets {
    root: Option<PathBuf>,
    images: HashMap<String, Arc<Pixmap>>,
    bytes: usize,
}
impl OverlayAssets {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn load_content(&mut self, store: &ProjectStore, content: &OverlayContent) -> Result<()> {
        content.validate()?;
        let Some(relative) = &content.image else {
            return Ok(());
        };
        if self
            .root
            .as_deref()
            .is_some_and(|root| root != store.root())
        {
            return Err("Clear the image cache before opening another project".into());
        }
        if self.images.contains_key(relative) {
            return Ok(());
        }
        if self.images.len() >= MAX_IMAGES {
            return Err(
                "Image cache is full; prepare only the overlays needed for this sequence".into(),
            );
        }
        let path = local::confined_file(
            store.root(),
            relative,
            PathPolicy::LocalProject,
            FileMode::Existing,
        )?;
        if fs::metadata(&path)?.len() > MAX_IMAGE_BYTES {
            return Err("Overlay image exceeds the 16 MiB encoded-image limit".into());
        }
        let mut bytes = Vec::new();
        fs::File::open(path)?
            .take(MAX_IMAGE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_IMAGE_BYTES {
            return Err("Overlay image grew beyond its resource limit".into());
        }
        let reader = image::ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;
        if !matches!(
            reader.format(),
            Some(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP)
        ) {
            return Err("Choose a PNG, JPEG or WebP overlay image".into());
        }
        let (width, height) = reader
            .into_dimensions()
            .map_err(|_| "Cannot read overlay image dimensions")?;
        let needed = u64::from(width) * u64::from(height) * 4;
        if width == 0
            || height == 0
            || width > 4096
            || height > 4096
            || needed > MAX_CACHE_BYTES as u64
            || self.bytes as u64 + needed > MAX_CACHE_BYTES as u64
        {
            return Err("Decoded overlay images exceed the 4096px/64 MiB cache limit".into());
        }
        let mut reader = image::ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(MAX_CACHE_BYTES as u64);
        reader.limits(limits);
        let mut pixels = reader
            .decode()
            .map_err(|_| "Overlay image cannot be decoded within its resource limits")?
            .to_rgba8()
            .into_raw();
        for p in pixels.as_chunks_mut::<4>().0 {
            for c in 0..3 {
                p[c] = ((u16::from(p[c]) * u16::from(p[3]) + 127) / 255) as u8;
            }
        }
        let pixmap = Pixmap::from_vec(
            pixels,
            IntSize::from_wh(width, height).ok_or("Invalid image size")?,
        )
        .ok_or("Invalid decoded image")?;
        self.bytes += pixmap.data().len();
        self.root = Some(store.root().to_owned());
        self.images.insert(relative.clone(), Arc::new(pixmap));
        Ok(())
    }
    pub fn load_studio(&mut self, store: &ProjectStore, studio: &Studio) -> Result<()> {
        studio.validate()?;
        for content in studio
            .slots()
            .iter()
            .filter_map(|slot| slot.content())
            .chain(studio.cues().iter().filter_map(|cue| cue.content()))
        {
            self.load_content(store, content)?;
        }
        Ok(())
    }
    fn image(&self, content: &OverlayContent) -> Result<Option<&Pixmap>> {
        content
            .image
            .as_ref()
            .map(|path| {
                self.images.get(path).map(Arc::as_ref).ok_or_else(|| {
                    "Prepare this overlay's project image before preview or export".into()
                })
            })
            .transpose()
    }
}

#[derive(PartialEq)]
struct LayoutKey {
    size: OutputSize,
    content: OverlayContent,
    image_size: Option<(u32, u32)>,
}
struct TextLayout {
    buffer: Buffer,
    width: f32,
    height: f32,
    regions: Vec<Rect>,
}
struct TextItem {
    layout: TextLayout,
    x: f32,
    y: f32,
    color: [u8; 4],
    number: Option<usize>,
}
struct CardLayout {
    card: Rect,
    texts: Vec<TextItem>,
    image: Option<Rect>,
    quote: bool,
}
pub struct OverlayRenderer {
    fonts: FontSystem,
    glyphs: SwashCache,
    layouts: VecDeque<(LayoutKey, Arc<CardLayout>)>,
}
impl OverlayRenderer {
    pub fn new() -> Result<Self> {
        // FontSystem::new/new_with_fonts scans the host. Construct the database
        // explicitly so both rendering and fallback can use ONLY bundled fonts.
        let mut db = fontdb::Database::new();
        db.load_font_data(INTER_REGULAR.to_vec());
        db.load_font_data(INTER_SEMIBOLD.to_vec());
        db.set_sans_serif_family("Inter");
        if db.faces().count() != 2 {
            return Err("Bundled Inter fonts could not be loaded".into());
        }
        Ok(Self {
            fonts: FontSystem::new_with_locale_and_db("en-US".into(), db),
            glyphs: SwashCache::new(),
            layouts: VecDeque::new(),
        })
    }
    pub fn render_timeline(
        &mut self,
        studio: &Studio,
        sequence_ms: u64,
        assets: &OverlayAssets,
        options: RenderOptions,
    ) -> Result<RgbaFrame> {
        // Find the current clip once. Do not perform a whole-project cue lookup
        // for every inactive cue on every frame of a long interview.
        let mut cursor = 0_u64;
        let mut current = None;
        for clip in studio.clips() {
            let end = cursor
                .checked_add(clip.duration_ms())
                .ok_or("Sequence time overflows milliseconds")?;
            if sequence_ms < end {
                current = Some((clip.id(), sequence_ms - cursor));
                break;
            }
            cursor = end;
        }
        let mut active = Vec::new();
        for cue in studio.cues() {
            let Some((clip, local_ms)) = current else {
                break;
            };
            if cue.clip_id() != clip
                || local_ms < cue.offset_ms()
                || local_ms - cue.offset_ms() >= cue.duration_ms()
            {
                continue;
            }
            if active.len() == MAX_ACTIVE {
                return Err("More than nine overlays overlap; shorten or move their cues".into());
            }
            let start = studio.cue_sequence_time(cue.id())?;
            if sequence_ms < start || sequence_ms - start >= cue.duration_ms() {
                continue;
            }
            let content = cue.content().ok_or(
                "This cue has no approved overlay content; choose its content before playback",
            )?;
            active.push(ActiveOverlay {
                content,
                elapsed_ms: sequence_ms - start,
                duration_ms: cue.duration_ms(),
            });
        }
        self.render_live(studio.orientation().output_size(), &active, assets, options)
    }
    pub fn render_live(
        &mut self,
        size: OutputSize,
        overlays: &[ActiveOverlay<'_>],
        assets: &OverlayAssets,
        options: RenderOptions,
    ) -> Result<RgbaFrame> {
        if size != Orientation::Horizontal.output_size()
            && size != Orientation::Vertical.output_size()
        {
            return Err("Choose the project's horizontal or vertical output size".into());
        }
        let active: Vec<_> = overlays
            .iter()
            .filter(|o| o.elapsed_ms < o.duration_ms)
            .take(MAX_ACTIVE + 1)
            .collect();
        if active.len() > MAX_ACTIVE {
            return Err("More than nine overlays overlap; shorten or move their cues".into());
        }
        let mut canvas =
            Pixmap::new(size.width, size.height).ok_or("Cannot allocate overlay frame")?;
        for overlay in active {
            overlay.content.validate()?;
            let image = assets.image(overlay.content)?;
            let key = LayoutKey {
                size,
                content: overlay.content.clone(),
                image_size: image.map(|i| (i.width(), i.height())),
            };
            let layout = if let Some(index) = self.layouts.iter().position(|(k, _)| k == &key) {
                let cached = self
                    .layouts
                    .remove(index)
                    .ok_or("Overlay layout cache changed")?;
                let layout = cached.1.clone();
                self.layouts.push_front(cached);
                layout
            } else {
                let layout = Arc::new(self.layout(size, overlay.content, image)?);
                self.layouts.push_front((key, layout.clone()));
                self.layouts.truncate(MAX_LAYOUTS);
                layout
            };
            let still = options.reduced_motion || overlay.content.motion == OverlayMotion::Still;
            let progress = enter_progress(overlay.elapsed_ms, 0, still);
            let mut layer =
                Pixmap::new(size.width, size.height).ok_or("Cannot allocate overlay layer")?;
            rounded(&mut layer, layout.card, 24., IVORY);
            let c = layout.card;
            rounded(&mut layer, rect(c.x(), c.y(), 8., c.height())?, 4., VIOLET);
            if let (Some(image), Some(bounds)) = (image, layout.image) {
                // Contain the complete approved image; never crop its evidence.
                let scale = (bounds.width() / image.width() as f32)
                    .min(bounds.height() / image.height() as f32);
                let x = bounds.x() + (bounds.width() - image.width() as f32 * scale) / 2.;
                let y = bounds.y() + (bounds.height() - image.height() as f32 * scale) / 2.;
                layer.draw_pixmap(
                    0,
                    0,
                    image.as_ref(),
                    &PixmapPaint::default(),
                    Transform::from_scale(scale, scale).post_translate(x, y),
                    None,
                );
            }
            if layout.quote {
                // A geometric quote mark is decoration; approved text stays exact.
                for x in [c.x() + 32., c.x() + 53.] {
                    rounded(&mut layer, rect(x, c.y() + 35., 12., 22.)?, 3., VIOLET);
                }
            }
            for (index, item) in layout.texts.iter().enumerate() {
                let p = enter_progress(overlay.elapsed_ms, index, still);
                self.text(&mut layer, item, overlay.content.highlight_style, p, still)?;
            }
            let paint = PixmapPaint {
                opacity: progress,
                ..PixmapPaint::default()
            };
            canvas.draw_pixmap(
                0,
                0,
                layer.as_ref(),
                &paint,
                Transform::from_translate(0., if still { 0. } else { 24. * (1. - progress) }),
                None,
            );
        }
        if self.glyphs.image_cache.len() > 8192 {
            self.glyphs = SwashCache::new();
        }
        Ok(straight(canvas))
    }

    fn shape(
        &mut self,
        text: &str,
        width: f32,
        font_size: f32,
        semibold: bool,
        highlights: &[(usize, usize)],
    ) -> Result<TextLayout> {
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(font_size, font_size * 1.32));
        buffer.set_size(&mut self.fonts, Some(width), None);
        buffer.set_wrap(&mut self.fonts, Wrap::WordOrGlyph);
        buffer.set_text(
            &mut self.fonts,
            text,
            &Attrs::new()
                .family(Family::Name("Inter"))
                .weight(Weight(if semibold { 600 } else { 400 })),
            Shaping::Advanced,
        );
        buffer.shape_until_scroll(&mut self.fonts, false);
        let scalar_bytes: Vec<_> = text
            .char_indices()
            .map(|(byte, _)| byte)
            .chain(std::iter::once(text.len()))
            .collect();
        let mut ranges: Vec<_> = LineIter::new(text).map(|(range, _)| range).collect();
        while ranges.len() < buffer.lines.len() {
            ranges.push(text.len()..text.len());
        }
        let mut regions = Vec::new();
        let mut height = 0_f32;
        for run in buffer.layout_runs() {
            height = height.max(run.line_top + run.line_height);
            if run.line_w > width + 0.5 {
                return Err(
                    "Overlay text is wider than its card; edit the text before playback".into(),
                );
            }
            if run.glyphs.iter().any(|glyph| glyph.glyph_id == 0) {
                return Err("Bundled Inter does not support a character in this overlay; edit it before playback".into());
            }
            let line = ranges
                .get(run.line_i)
                .ok_or("Overlay text line could not be mapped")?;
            for &(start, end) in highlights {
                let start = *scalar_bytes.get(start).ok_or("Invalid highlight start")?;
                let end = *scalar_bytes.get(end).ok_or("Invalid highlight end")?;
                if start >= line.end || end <= line.start {
                    continue;
                }
                let mut from = start.max(line.start) - line.start;
                let mut to = end.min(line.end) - line.start;
                // A scalar range can cut through one shaped glyph (a combining
                // mark or ligature). Emphasize that complete cluster rather
                // than losing the highlight or slicing the glyph itself.
                for glyph in run.glyphs {
                    if glyph.start < to && glyph.end > from {
                        from = from.min(glyph.start);
                        to = to.max(glyph.end);
                    }
                }
                let from = cosmic_text::Cursor::new(run.line_i, from);
                let to = cosmic_text::Cursor::new(run.line_i, to);
                if let Some((x, w)) = run.highlight(from, to)
                    && w > 0.
                {
                    regions.push(rect(x, run.line_top, w, run.line_height)?);
                }
            }
        }
        Ok(TextLayout {
            buffer,
            width,
            height,
            regions,
        })
    }
    fn layout(
        &mut self,
        size: OutputSize,
        content: &OverlayContent,
        image: Option<&Pixmap>,
    ) -> Result<CardLayout> {
        let vertical = size.height > size.width;
        let safe_x = if vertical { 54. } else { 96. };
        let safe_y = if vertical { 96. } else { 54. };
        let max_width: f32 = match content.kind {
            OverlayKind::LowerThird => 1120.,
            OverlayKind::Stat => 840.,
            OverlayKind::Quote => 1120.,
            _ => 960.,
        };
        let width = max_width.min(size.width as f32 - 2. * safe_x);
        let padding = 44.;
        let quote = content.kind == OverlayKind::Quote;
        let inner = width - 2. * padding;
        let title_size = match content.kind {
            OverlayKind::Stat => 76.,
            OverlayKind::LowerThird => 46.,
            OverlayKind::Quote => 30.,
            _ => 40.,
        };
        let title = self.shape(&content.title, inner, title_size, true, &[])?;
        let mut y = padding + if quote { 28. } else { 0. };
        let title_h = title.height;
        let mut texts = vec![TextItem {
            layout: title,
            x: padding,
            y,
            color: INK,
            number: None,
        }];
        y += title_h + 18.;
        let image =
            if let Some(image) = image {
                let height = (inner * image.height() as f32 / image.width() as f32)
                    .min(if vertical { 600. } else { 420. });
                let bounds = rect(padding, y, inner, height)?;
                y += height + 22.;
                Some(bounds)
            } else {
                None
            };
        for (index, block) in content.blocks.iter().enumerate() {
            let number = (content.kind == OverlayKind::NumberedList).then_some(index + 1);
            let inset = if number.is_some() { 62. } else { 0. };
            let font_size = match block.role.as_str() {
                "heading" => 38.,
                "source" | "label" => 26.,
                _ if quote => 38.,
                _ => 32.,
            };
            let highlights: Vec<_> = content
                .highlights
                .iter()
                .filter(|h| h.block == index)
                .map(|h| (h.start, h.end))
                .collect();
            let layout = self.shape(
                &block.text,
                inner - inset,
                font_size,
                block.role == "heading",
                &highlights,
            )?;
            let height = layout.height.max(if number.is_some() { 44. } else { 0. });
            texts.push(TextItem {
                layout,
                x: padding + inset,
                y,
                color: BODY,
                number,
            });
            y += height + 18.;
        }
        let height = y - 18. + padding;
        // Reserve the complete fade-up displacement inside the safe frame.
        if height > size.height as f32 - 2. * safe_y - 24. {
            return Err("Overlay content does not fit this shape at a readable size; shorten it or split it into another overlay".into());
        }
        let right = matches!(
            content.kind,
            OverlayKind::Stat
                | OverlayKind::ScreenshotCard
                | OverlayKind::NumberedList
                | OverlayKind::Photo
        );
        let x = if vertical {
            (size.width as f32 - width) / 2.
        } else if right {
            size.width as f32 - safe_x - width
        } else {
            safe_x
        };
        let top = size.height as f32 - safe_y - 24. - height;
        for item in &mut texts {
            item.x += x;
            item.y += top;
        }
        let image = image
            .map(|r| rect(r.x() + x, r.y() + top, r.width(), r.height()))
            .transpose()?;
        Ok(CardLayout {
            card: rect(x, top, width, height)?,
            texts,
            image,
            quote,
        })
    }

    fn text(
        &mut self,
        layer: &mut Pixmap,
        item: &TextItem,
        style: HighlightStyle,
        p: f32,
        still: bool,
    ) -> Result<()> {
        if p == 0. {
            return Ok(());
        }
        let y = item.y + if still { 0. } else { 24. * (1. - p) };
        let regions = &item.layout.regions;
        for region in regions {
            let reveal = if still { 1. } else { p };
            let (top, height, color) = if style == HighlightStyle::UnderlineDraw {
                (region.y() + region.height() - 5., 4., VIOLET)
            } else {
                (region.y() + 4., region.height() - 7., MARKER)
            };
            rounded(
                layer,
                rect(
                    item.x + region.x(),
                    y + top,
                    (region.width() * reveal).max(0.01),
                    height,
                )?,
                3.,
                alpha(color, p),
            );
        }
        if let Some(number) = item.number {
            rounded(
                layer,
                rect(item.x - 62., y + 2., 44., 44.)?,
                12.,
                alpha(VIOLET, p),
            );
            let label = self.shape(&number.to_string(), 38., 25., true, &[])?;
            draw_text(
                &mut self.fonts,
                &mut self.glyphs,
                layer,
                &label,
                item.x - 53.,
                y + 6.,
                WHITE,
                p,
                &[],
                TextPass::All,
            )?;
        }
        let faded = style == HighlightStyle::FadeRest;
        if style == HighlightStyle::ZoomToLine && !regions.is_empty() && !still {
            // Focus one highlighted line, leaving all copied text visible. The
            // 44px card padding contains the bounded emphasis transform.
            let focus = regions[0];
            let mut base = Pixmap::new(layer.width(), layer.height())
                .ok_or("Cannot allocate text emphasis")?;
            draw_text(
                &mut self.fonts,
                &mut self.glyphs,
                &mut base,
                &item.layout,
                item.x,
                y,
                item.color,
                p,
                &[focus],
                TextPass::WithoutFocusLine,
            )?;
            layer.draw_pixmap(
                0,
                0,
                base.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
            let mut zoom =
                Pixmap::new(layer.width(), layer.height()).ok_or("Cannot allocate focused line")?;
            draw_text(
                &mut self.fonts,
                &mut self.glyphs,
                &mut zoom,
                &item.layout,
                item.x,
                y,
                item.color,
                p,
                &[focus],
                TextPass::FocusLine,
            )?;
            let scale = 1. + (72. / item.layout.width).min(0.12) * p;
            let cx = item.x + item.layout.width / 2.;
            let cy = y + focus.y() + focus.height() / 2.;
            layer.draw_pixmap(
                0,
                0,
                zoom.as_ref(),
                &PixmapPaint::default(),
                Transform::from_scale(scale, scale)
                    .post_translate(cx * (1. - scale), cy * (1. - scale)),
                None,
            );
        } else {
            draw_text(
                &mut self.fonts,
                &mut self.glyphs,
                layer,
                &item.layout,
                item.x,
                y,
                item.color,
                p,
                if faded { regions } else { &[] },
                if faded {
                    TextPass::FadeRest
                } else {
                    TextPass::All
                },
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TextPass {
    All,
    WithoutFocusLine,
    FocusLine,
    FadeRest,
}
#[allow(clippy::too_many_arguments)]
fn draw_text(
    fonts: &mut FontSystem,
    cache: &mut SwashCache,
    pixmap: &mut Pixmap,
    text: &TextLayout,
    x: f32,
    y: f32,
    color: [u8; 4],
    opacity: f32,
    regions: &[Rect],
    pass: TextPass,
) -> Result<()> {
    let mut outside = false;
    text.buffer.draw(
        fonts,
        cache,
        Color::rgba(color[0], color[1], color[2], color[3]),
        |px, py, w, h, c| {
            let focused_line = regions
                .first()
                .is_some_and(|r| py as f32 >= r.y() && (py as f32) < r.bottom());
            if (pass == TextPass::FocusLine && !focused_line)
                || (pass == TextPass::WithoutFocusLine && focused_line)
            {
                return;
            }
            let highlighted = regions.iter().any(|r| {
                px as f32 >= r.x()
                    && (px as f32) < r.right()
                    && py as f32 >= r.y()
                    && (py as f32) < r.bottom()
            });
            let opacity = opacity
                * if pass == TextPass::FadeRest && !highlighted {
                    0.8
                } else {
                    1.
                };
            for dy in 0..h {
                for dx in 0..w {
                    let dest_x = x.round() as i32 + px + dx as i32;
                    let dest_y = y.round() as i32 + py + dy as i32;
                    if dest_x < 0
                        || dest_y < 0
                        || dest_x >= pixmap.width() as i32
                        || dest_y >= pixmap.height() as i32
                    {
                        if c.a() != 0 {
                            outside = true;
                        }
                        continue;
                    }
                    let offset = (dest_y as usize * pixmap.width() as usize + dest_x as usize) * 4;
                    blend(
                        &mut pixmap.data_mut()[offset..offset + 4],
                        [c.r(), c.g(), c.b(), c.a()],
                        opacity,
                    );
                }
            }
        },
    );
    if outside {
        return Err("Overlay text extends outside the frame; edit its content".into());
    }
    Ok(())
}
fn blend(destination: &mut [u8], color: [u8; 4], opacity: f32) {
    let a = (f32::from(color[3]) * opacity).round().clamp(0., 255.) as u16;
    for channel in 0..3 {
        destination[channel] =
            ((u16::from(color[channel]) * a + u16::from(destination[channel]) * (255 - a) + 127)
                / 255)
                .min(255) as u8;
    }
    destination[3] = (a + (u16::from(destination[3]) * (255 - a) + 127) / 255).min(255) as u8;
}
fn straight(pixmap: Pixmap) -> RgbaFrame {
    let width = pixmap.width();
    let height = pixmap.height();
    let mut pixels = pixmap.take();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let a = u32::from(pixel[3]);
        if a == 0 {
            pixel.fill(0);
        } else if a < 255 {
            for c in pixel.iter_mut().take(3) {
                *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    RgbaFrame {
        width,
        height,
        pixels,
    }
}
fn alpha(mut color: [u8; 4], opacity: f32) -> [u8; 4] {
    color[3] = (f32::from(color[3]) * opacity).round().clamp(0., 255.) as u8;
    color
}
fn rect(x: f32, y: f32, w: f32, h: f32) -> Result<Rect> {
    Rect::from_xywh(x, y, w, h).ok_or_else(|| "Invalid overlay layout bounds".into())
}
fn rounded(pixmap: &mut Pixmap, r: Rect, radius: f32, color: [u8; 4]) {
    let radius = radius.min(r.width() / 2.).min(r.height() / 2.);
    let mut path = PathBuilder::new();
    path.move_to(r.x() + radius, r.y());
    path.line_to(r.right() - radius, r.y());
    path.quad_to(r.right(), r.y(), r.right(), r.y() + radius);
    path.line_to(r.right(), r.bottom() - radius);
    path.quad_to(r.right(), r.bottom(), r.right() - radius, r.bottom());
    path.line_to(r.x() + radius, r.bottom());
    path.quad_to(r.x(), r.bottom(), r.x(), r.bottom() - radius);
    path.line_to(r.x(), r.y() + radius);
    path.quad_to(r.x(), r.y(), r.x() + radius, r.y());
    path.close();
    if let Some(path) = path.finish() {
        let mut paint = Paint::default();
        paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}
fn enter_progress(elapsed_ms: u64, index: usize, still: bool) -> f32 {
    if still {
        1.
    } else {
        ease(elapsed_ms.saturating_sub((index as u64).saturating_mul(90)) as f32 / 600.)
    }
}
fn ease(value: f32) -> f32 {
    let x = value.clamp(0., 1.);
    if x == 0. || x == 1. {
        return x;
    }
    let cubic = |t: f32, a: f32, b: f32| {
        3. * (1. - t) * (1. - t) * t * a + 3. * (1. - t) * t * t * b + t * t * t
    };
    let (mut low, mut high) = (0., 1.);
    for _ in 0..20 {
        let mid = (low + high) / 2.;
        if cubic(mid, 0.16, 0.3) < x {
            low = mid;
        } else {
            high = mid;
        }
    }
    cubic((low + high) / 2., 1., 1.)
}

#[cfg(test)]
mod tests;
