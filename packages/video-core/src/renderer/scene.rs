//! Core scene composition. Preparation may do IO; render methods never do.
use super::*;
use crate::{
    insert::{self, InsertSource, SceneLayout, Viewport},
    media::Cancellation,
};
use tiny_skia::{FilterQuality, Mask, Pattern, SpreadMode};
#[cfg(test)]
static PROFILE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
fn profile(last: &mut std::time::Instant, name: &str) {
    if PROFILE.load(std::sync::atomic::Ordering::Relaxed) {
        eprintln!(
            "scene stage {name}: {:.3}ms",
            last.elapsed().as_secs_f64() * 1000.
        );
    }
    *last = std::time::Instant::now();
}

pub fn active_overlays(studio: &Studio, sequence_ms: u64) -> Result<Vec<ActiveOverlay<'_>>> {
    let mut cursor = 0_u64;
    let mut current = None;
    for clip in studio.clips() {
        let end = cursor
            .checked_add(clip.duration_ms())
            .ok_or("Sequence time overflows")?;
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
        active.push(ActiveOverlay {
            content: cue.content().ok_or(
                "This cue has no approved overlay content; choose its content before playback",
            )?,
            elapsed_ms: local_ms - cue.offset_ms(),
            duration_ms: cue.duration_ms(),
        });
    }
    Ok(active)
}
fn visual_insert<'a, 'b>(active: &'a [ActiveOverlay<'b>]) -> Result<Option<&'a ActiveOverlay<'b>>> {
    let mut inserts = active
        .iter()
        .filter(|o| o.elapsed_ms < o.duration_ms && o.content.insert.is_some());
    let first = inserts.next();
    if inserts.next().is_some() {
        return Err("Two visual inserts overlap; move or shorten one of their cues".into());
    }
    Ok(first)
}
impl OverlayAssets {
    /// Natural-aspect source thumbnail from explicitly prepared assets. No IO,
    /// decoding or invented placeholder media occurs in this method.
    pub fn insert_preview(&self, content: &OverlayContent, elapsed_ms: u64) -> Result<RgbaFrame> {
        content.validate()?;
        let overlay = ActiveOverlay {
            content,
            elapsed_ms,
            duration_ms: elapsed_ms.saturating_add(1),
        };
        Ok(straight(self.insert_image(&overlay)?.clone()))
    }
    pub fn prepare_timeline(
        &mut self,
        store: &ProjectStore,
        studio: &Studio,
        time_ms: u64,
        cancel: &Cancellation,
    ) -> Result<()> {
        self.prepare_live(store, &active_overlays(studio, time_ms)?, cancel)
    }
    pub fn prepare_live(
        &mut self,
        store: &ProjectStore,
        active: &[ActiveOverlay<'_>],
        cancel: &Cancellation,
    ) -> Result<()> {
        cancel.check()?;
        if self
            .root
            .as_deref()
            .is_some_and(|root| root != store.root())
        {
            return Err("Clear scene assets before opening another project".into());
        }
        let insert = visual_insert(active)?;
        // Invalidate before any fallible preparation. A failed decode must not
        // allow a previous frame or another project's image to stand in.
        self.video_frame = None;
        for overlay in active.iter().filter(|o| o.elapsed_ms < o.duration_ms) {
            self.load_content(store, overlay.content)?;
        }
        let Some(overlay) = insert else {
            self.video_cursor = None;
            return Ok(());
        };
        let content = overlay
            .content
            .insert
            .as_ref()
            .ok_or("Missing visual insert")?;
        match &content.source {
            InsertSource::Photo { .. } => self.video_cursor = None,
            InsertSource::Video { path, .. } => {
                if self
                    .video_cursor
                    .as_ref()
                    .is_none_or(|cursor| !cursor.matches(store, path))
                {
                    self.video_cursor = Some(insert::VideoCursor::new(store, path, cancel)?);
                }
                let frame = content
                    .source_frame(overlay.elapsed_ms)?
                    .ok_or("Missing insert source frame")?;
                let rgba = self
                    .video_cursor
                    .as_mut()
                    .ok_or("Missing video insert cursor")?
                    .frame_at(frame, cancel)?;
                self.video_frame = Some((path.clone(), frame, Arc::new(premultiplied(rgba)?)));
                self.root = Some(store.root().into());
            }
        }
        Ok(())
    }
    fn insert_image(&self, overlay: &ActiveOverlay<'_>) -> Result<&Pixmap> {
        let content = overlay
            .content
            .insert
            .as_ref()
            .ok_or("Missing insert content")?;
        match &content.source {
            InsertSource::Photo { path } => self
                .images
                .get(path)
                .map(Arc::as_ref)
                .ok_or_else(|| "Prepare this photo insert before playback".into()),
            InsertSource::Video { path, .. } => {
                let frame = content
                    .source_frame(overlay.elapsed_ms)?
                    .ok_or("Missing source frame")?;
                self.video_frame.as_ref().filter(|(p,f,_)|p==path&&*f==frame).map(|(_,_,image)|image.as_ref()).ok_or_else(||"Prepare this exact video insert frame before playback; stale frames are not shown".into())
            }
        }
    }
}
impl OverlayRenderer {
    pub fn render_scene_timeline(
        &mut self,
        studio: &Studio,
        time_ms: u64,
        camera: &RgbaFrame,
        assets: &OverlayAssets,
        options: RenderOptions,
    ) -> Result<RgbaFrame> {
        self.render_scene_live(
            studio.orientation().output_size(),
            camera,
            &active_overlays(studio, time_ms)?,
            assets,
            options,
        )
    }
    pub fn render_scene_live(
        &mut self,
        size: OutputSize,
        camera: &RgbaFrame,
        active: &[ActiveOverlay<'_>],
        assets: &OverlayAssets,
        options: RenderOptions,
    ) -> Result<RgbaFrame> {
        // shortcut: full-HD insert scenes can exceed33ms; remeasure before promising30fps.
        #[cfg(test)]
        let mut stamp = std::time::Instant::now();
        #[cfg(test)]
        let reference = self.scene_reference;
        #[cfg(not(test))]
        let reference = false;
        if size != Orientation::Horizontal.output_size()
            && size != Orientation::Vertical.output_size()
        {
            return Err("Choose the project's horizontal or vertical output size".into());
        }
        if active
            .iter()
            .filter(|o| o.elapsed_ms < o.duration_ms)
            .count()
            > MAX_ACTIVE
        {
            return Err("More than nine overlays overlap".into());
        }
        let insert = visual_insert(active)?;
        let camera_size = OutputSize {
            width: camera.width,
            height: camera.height,
        };
        let camera = premultiplied(camera)?;
        let mut canvas =
            Pixmap::new(size.width, size.height).ok_or("Cannot allocate scene frame")?;
        canvas.fill(tiny_skia::Color::from_rgba8(
            IVORY[0], IVORY[1], IVORY[2], 255,
        ));
        #[cfg(test)]
        profile(&mut stamp, "setup-camera");
        if let Some(overlay) = insert {
            if !self
                .validated_inserts
                .iter()
                .any(|known| known == overlay.content)
            {
                overlay.content.validate()?;
                self.validated_inserts.push_front(overlay.content.clone());
                self.validated_inserts.truncate(MAX_LAYOUTS);
            }
            let content = overlay
                .content
                .insert
                .as_ref()
                .ok_or("Missing visual insert")?;
            let layout = SceneLayout::new(size, camera_size, content.placement)?;
            let image = assets.insert_image(overlay)?;
            let target = OutputSize {
                width: layout.media.width.round() as u32,
                height: layout.media.height.round() as u32,
            };
            let view = content.zoom.viewport(
                OutputSize {
                    width: image.width(),
                    height: image.height(),
                },
                target,
                overlay.elapsed_ms,
                options.reduced_motion,
            )?;
            #[cfg(test)]
            profile(&mut stamp, "content-validation-layout");
            draw_view(
                &mut canvas,
                image,
                view,
                layout.media,
                reference,
                &mut self.scene_scratch,
                self.scene_threads,
            )?;
            #[cfg(test)]
            profile(&mut stamp, "insert-blit");
            if let Some(target) = layout.camera {
                if content.placement == insert::InsertPlacement::Behind {
                    let border = 5.;
                    rounded(
                        &mut canvas,
                        rect(
                            target.x as f32 - border,
                            target.y as f32 - border,
                            target.width as f32 + 2. * border,
                            target.height as f32 + 2. * border,
                        )?,
                        8.,
                        VIOLET,
                    );
                }
                draw_view(
                    &mut canvas,
                    &camera,
                    insert::cover_view(
                        camera_size,
                        OutputSize {
                            width: target.width.round() as u32,
                            height: target.height.round() as u32,
                        },
                    )?,
                    target,
                    reference,
                    &mut self.scene_scratch,
                    self.scene_threads,
                )?;
            }
        } else {
            draw_view(
                &mut canvas,
                &camera,
                insert::cover_view(camera_size, size)?,
                Viewport {
                    x: 0.,
                    y: 0.,
                    width: f64::from(size.width),
                    height: f64::from(size.height),
                },
                reference,
                &mut self.scene_scratch,
                self.scene_threads,
            )?;
        }
        #[cfg(test)]
        profile(&mut stamp, "camera-blit");
        let cards: Vec<_> = active
            .iter()
            .copied()
            .filter(|o| o.content.insert.is_none())
            .collect();
        if !cards.is_empty() {
            let cards = self.render_cards(size, &cards, assets, options)?;
            #[cfg(test)]
            profile(&mut stamp, "cards");
            if reference {
                canvas.draw_pixmap(
                    0,
                    0,
                    cards.as_ref(),
                    &PixmapPaint::default(),
                    Transform::identity(),
                    None,
                );
            } else {
                compose_cards(&mut canvas, &cards)?;
            }
        }
        #[cfg(test)]
        profile(&mut stamp, "cards-blit");
        let result = straight(canvas);
        #[cfg(test)]
        profile(&mut stamp, "straight");
        Ok(result)
    }
}
fn premultiplied(frame: &RgbaFrame) -> Result<Pixmap> {
    insert::dimensions(OutputSize {
        width: frame.width,
        height: frame.height,
    })?;
    let count = u64::from(frame.width) * u64::from(frame.height) * 4;
    if count > 64 * 1024 * 1024 || count != frame.pixels.len() as u64 {
        return Err("Invalid or oversized RGBA source frame".into());
    }
    let mut pixels = frame.pixels.clone();
    for p in pixels.as_chunks_mut::<4>().0 {
        if p[3] != 255 {
            for c in 0..3 {
                p[c] = ((u16::from(p[c]) * u16::from(p[3]) + 127) / 255) as u8;
            }
        }
    }
    Pixmap::from_vec(
        pixels,
        IntSize::from_wh(frame.width, frame.height).ok_or("Invalid source size")?,
    )
    .ok_or_else(|| "Invalid source frame".into())
}
fn draw_view(
    canvas: &mut Pixmap,
    source: &Pixmap,
    view: Viewport,
    target: Viewport,
    full_mask: bool,
    scratch: &mut Vec<Pixmap>,
    threads: usize,
) -> Result<()> {
    let sx = target.width / view.width;
    let sy = target.height / view.height;
    let transform = Transform::from_scale(sx as f32, sy as f32).post_translate(
        (target.x - view.x * sx) as f32,
        (target.y - view.y * sy) as f32,
    );
    if !full_mask
        && target.width * target.height >= 512_000.
        && [target.x, target.y, target.width, target.height]
            .iter()
            .all(|v| v.fract() == 0.)
    {
        return draw_parallel_panel(canvas, source, target, transform, scratch, threads);
    }
    let mask = if full_mask
        || target.x != 0.
        || target.y != 0.
        || target.width != f64::from(canvas.width())
        || target.height != f64::from(canvas.height())
    {
        let mut path = PathBuilder::new();
        path.push_rect(rect(
            target.x as f32,
            target.y as f32,
            target.width as f32,
            target.height as f32,
        )?);
        let mut mask =
            Mask::new(canvas.width(), canvas.height()).ok_or("Cannot allocate scene crop mask")?;
        mask.fill_path(
            &path.finish().ok_or("Invalid scene crop path")?,
            FillRule::Winding,
            true,
            Transform::identity(),
        );
        Some(mask)
    } else {
        None
    };
    canvas.draw_pixmap(
        0,
        0,
        source.as_ref(),
        &PixmapPaint {
            quality: FilterQuality::Bilinear,
            ..PixmapPaint::default()
        },
        transform,
        mask.as_ref(),
    );
    Ok(())
}
// Preserve original global sampling coordinates rather than translating local
// tiles (which can change floating-point rounding). Four full-size scratch
// targets are bounded; only each target's assigned rows are rasterized/copied.
fn draw_parallel_panel(
    canvas: &mut Pixmap,
    source: &Pixmap,
    target: Viewport,
    transform: Transform,
    scratch: &mut Vec<Pixmap>,
    threads: usize,
) -> Result<()> {
    let width = canvas.width();
    let height = canvas.height();
    let original = canvas.data();
    let y = target.y as u32;
    let rows = target.height as u32;
    if target.x < 0.
        || target.y < 0.
        || target.x + target.width > f64::from(width)
        || y + rows > height
    {
        return Err("Scene panel lies outside the output frame".into());
    }
    if scratch.len() != threads
        || scratch
            .iter()
            .any(|pixmap| pixmap.width() != width || pixmap.height() != height)
    {
        scratch.clear();
        for _ in 0..threads {
            scratch.push(
                Pixmap::new(width, height).ok_or("Cannot allocate bounded scene raster target")?,
            );
        }
    }
    let row_bytes = width as usize * 4;
    std::thread::scope(|scope| -> Result<()> {
        let mut jobs = Vec::new();
        for (index, stripe) in scratch.iter_mut().enumerate() {
            let top = y + rows * index as u32 / threads as u32;
            let bottom = y + rows * (index + 1) as u32 / threads as u32;
            jobs.push(scope.spawn(move || -> Result<()> {
                let first = top as usize * row_bytes;
                let last = bottom as usize * row_bytes;
                stripe.data_mut()[first..last].copy_from_slice(&original[first..last]);
                let paint = Paint {
                    shader: Pattern::new(
                        source.as_ref(),
                        SpreadMode::Pad,
                        FilterQuality::Bilinear,
                        1.,
                        transform,
                    ),
                    anti_alias: false,
                    ..Paint::default()
                };
                stripe.fill_rect(
                    rect(
                        target.x as f32,
                        top as f32,
                        target.width as f32,
                        (bottom - top) as f32,
                    )?,
                    &paint,
                    Transform::identity(),
                    None,
                );
                Ok(())
            }));
        }
        for job in jobs {
            job.join().map_err(|_| "Scene raster worker failed")??;
        }
        Ok(())
    })?;
    for (index, stripe) in scratch.iter().enumerate() {
        let top = y + rows * index as u32 / threads as u32;
        let bottom = y + rows * (index + 1) as u32 / threads as u32;
        let first = top as usize * row_bytes;
        let last = bottom as usize * row_bytes;
        canvas.data_mut()[first..last].copy_from_slice(&stripe.data()[first..last]);
    }
    Ok(())
}
// Identity, unmasked SourceOver with the same byte rounding as
// tiny-skia0.11's identity pattern path. Skip transparent pixels without invoking a
// full-frame pattern sampler. No layout or resampling happens here.
fn compose_cards(canvas: &mut Pixmap, cards: &Pixmap) -> Result<()> {
    if (canvas.width(), canvas.height()) != (cards.width(), cards.height()) {
        return Err("Card canvas size does not match the scene".into());
    }
    for (dst, src) in canvas
        .data_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(cards.data().as_chunks::<4>().0)
    {
        if src[3] == 0 {
            continue;
        }
        if src[3] == 255 {
            *dst = *src;
            continue;
        }
        let inverse = 255 - u16::from(src[3]);
        for c in 0..4 {
            dst[c] = (u16::from(src[c]) + ((u16::from(dst[c]) * inverse + 127) / 255)) as u8;
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Edit, Editor, InsertContent, InsertPlacement, InsertSource, SlotKey, ZoomPoint, ZoomSpec,
    };
    fn camera() -> RgbaFrame {
        RgbaFrame {
            width: 64,
            height: 48,
            pixels: [20, 80, 140, 255].repeat(64 * 48),
        }
    }
    fn content(placement: InsertPlacement) -> OverlayContent {
        OverlayContent::from_insert(
            "Generated insert",
            InsertContent {
                source: InsertSource::Photo {
                    path: "media/photo.png".into(),
                },
                placement,
                zoom: ZoomSpec {
                    end: ZoomPoint {
                        center_x: 650,
                        center_y: 450,
                        percent: 250,
                    },
                    ..ZoomSpec::default()
                },
            },
        )
    }
    fn project() -> (tempfile::TempDir, ProjectStore, Editor) {
        let temp = tempfile::tempdir().unwrap();
        let (store, studio) = ProjectStore::create(temp.path().join("scene"), "Scene").unwrap();
        image::RgbaImage::from_fn(96, 64, |x, y| {
            image::Rgba([(x * 2) as u8, (y * 3) as u8, 90, 255])
        })
        .save(store.root().join("media/photo.png"))
        .unwrap();
        std::fs::write(store.root().join("media/camera.mp4"), b"model fixture").unwrap();
        let mut editor = Editor::new(studio).unwrap();
        let source = editor.studio().next_media_id();
        editor
            .apply(Edit::AddMedia {
                path: "media/camera.mp4".into(),
                duration_ms: 6000,
            })
            .unwrap();
        editor
            .apply(Edit::AddClip {
                source,
                name: "Camera".into(),
                in_ms: 0,
                out_ms: 6000,
            })
            .unwrap();
        (temp, store, editor)
    }
    #[test]
    fn photo_scene_live_timeline_match_at_three_times_in_both_shapes_and_three_placements() {
        let (_temp, store, mut editor) = project();
        let mut renderer = OverlayRenderer::new().unwrap();
        for orientation in [Orientation::Horizontal, Orientation::Vertical] {
            editor.apply(Edit::SetOrientation(orientation)).unwrap();
            for placement in [
                InsertPlacement::Fullscreen,
                InsertPlacement::Beside,
                InsertPlacement::Behind,
            ] {
                let content = content(placement);
                let clip = editor.studio().clips()[0].id();
                let cue = editor.studio().next_cue_id();
                editor
                    .apply(Edit::AddCueFromSnapshot {
                        clip,
                        offset_ms: 0,
                        slot: SlotKey::try_from(1).unwrap(),
                        content: content.clone(),
                        duration_ms: 6000,
                    })
                    .unwrap();
                let mut changed = content.clone();
                changed.insert.as_mut().unwrap().zoom.end.percent = 100;
                editor
                    .apply(Edit::SetSlotContent {
                        slot: SlotKey::try_from(1).unwrap(),
                        content: changed,
                    })
                    .unwrap();
                let mut assets = OverlayAssets::new();
                assets.load_content(&store, &content).unwrap();
                let mut previous = None;
                for ms in [0, 2500, 5000] {
                    let active = [ActiveOverlay {
                        content: &content,
                        elapsed_ms: ms,
                        duration_ms: 6000,
                    }];
                    let live = renderer
                        .render_scene_live(
                            orientation.output_size(),
                            &camera(),
                            &active,
                            &assets,
                            RenderOptions::default(),
                        )
                        .unwrap();
                    let timeline = renderer
                        .render_scene_timeline(
                            editor.studio(),
                            ms,
                            &camera(),
                            &assets,
                            RenderOptions::default(),
                        )
                        .unwrap();
                    assert_eq!(live, timeline);
                    assert!(live.pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
                    if let Some(old) = previous {
                        assert_ne!(live, old);
                    }
                    previous = Some(live);
                }
                editor.apply(Edit::RemoveCue { cue }).unwrap();
            }
        }
    }
    #[test]
    fn missing_stale_overlapping_insert_and_legacy_overlay_api_fail_instead_of_dropping_media() {
        let (_temp, store, _) = project();
        let content = content(InsertPlacement::Fullscreen);
        let active = [ActiveOverlay {
            content: &content,
            elapsed_ms: 0,
            duration_ms: 6000,
        }];
        let mut renderer = OverlayRenderer::new().unwrap();
        let mut assets = OverlayAssets::new();
        assert!(
            renderer
                .render_scene_live(
                    Orientation::Horizontal.output_size(),
                    &camera(),
                    &active,
                    &assets,
                    RenderOptions::default()
                )
                .is_err()
        );
        assert!(
            renderer
                .render_live(
                    Orientation::Horizontal.output_size(),
                    &active,
                    &assets,
                    RenderOptions::default()
                )
                .is_err()
        );
        assets
            .prepare_live(&store, &active, &Cancellation::default())
            .unwrap();
        let preview = assets.insert_preview(&content, 0).unwrap();
        assert_eq!((preview.width, preview.height), (96, 64));
        std::fs::remove_file(store.root().join("media/photo.png")).unwrap();
        assert_eq!(assets.insert_preview(&content, 5000).unwrap(), preview);
        assert!(
            renderer
                .render_scene_live(
                    Orientation::Horizontal.output_size(),
                    &camera(),
                    &active,
                    &assets,
                    RenderOptions::default()
                )
                .is_ok()
        );
        assert!(
            renderer
                .render_scene_live(
                    Orientation::Horizontal.output_size(),
                    &camera(),
                    &[active[0], active[0]],
                    &assets,
                    RenderOptions::default()
                )
                .is_err()
        );
        let first = renderer
            .render_scene_live(
                Orientation::Horizontal.output_size(),
                &camera(),
                &active,
                &assets,
                RenderOptions {
                    reduced_motion: true,
                },
            )
            .unwrap();
        let later = [ActiveOverlay {
            elapsed_ms: 5000,
            ..active[0]
        }];
        assert_eq!(
            first,
            renderer
                .render_scene_live(
                    Orientation::Horizontal.output_size(),
                    &camera(),
                    &later,
                    &assets,
                    RenderOptions {
                        reduced_motion: true
                    }
                )
                .unwrap()
        );
        let mut broken = camera();
        broken.pixels.pop();
        assert!(
            renderer
                .render_scene_live(
                    Orientation::Horizontal.output_size(),
                    &broken,
                    &[],
                    &assets,
                    RenderOptions::default()
                )
                .is_err()
        );
    }
    #[test]
    fn behind_is_opaque_camera_card_and_beside_rectangles_are_honest() {
        let (_temp, store, _) = project();
        let mut renderer = OverlayRenderer::new().unwrap();
        let mut assets = OverlayAssets::new();
        let content = content(InsertPlacement::Behind);
        assets.load_content(&store, &content).unwrap();
        let output = Orientation::Horizontal.output_size();
        let frame = renderer
            .render_scene_live(
                output,
                &camera(),
                &[ActiveOverlay {
                    content: &content,
                    elapsed_ms: 0,
                    duration_ms: 6000,
                }],
                &assets,
                RenderOptions::default(),
            )
            .unwrap();
        let box_ = SceneLayout::new(
            output,
            OutputSize {
                width: 64,
                height: 48,
            },
            InsertPlacement::Behind,
        )
        .unwrap()
        .camera
        .unwrap();
        let x = (box_.x + box_.width / 2.) as usize;
        let y = (box_.y + box_.height / 2.) as usize;
        assert_eq!(
            &frame.pixels[(y * output.width as usize + x) * 4..][..4],
            &[20, 80, 140, 255]
        );
    }
}
#[cfg(test)]
mod timing {
    use super::*;
    use crate::{InsertContent, InsertPlacement, InsertSource, OverlayBlock, ZoomPoint, ZoomSpec};
    #[test]
    #[ignore = "explicit generated cached scene cost, no IO in timed frames"]
    fn cached_scene_insert_and_two_cards_timing() {
        let temp = tempfile::tempdir().unwrap();
        let (store, _) = ProjectStore::create(temp.path().join("timing"), "Scene timing").unwrap();
        image::RgbaImage::from_fn(960, 540, |x, y| {
            image::Rgba([(x % 250) as u8, (y % 250) as u8, 90, 255])
        })
        .save(store.root().join("media/photo.png"))
        .unwrap();
        let insert = OverlayContent::from_insert(
            "Generated insert",
            InsertContent {
                source: InsertSource::Photo {
                    path: "media/photo.png".into(),
                },
                placement: InsertPlacement::Behind,
                zoom: ZoomSpec {
                    end: ZoomPoint {
                        percent: 250,
                        ..ZoomPoint::default()
                    },
                    ..ZoomSpec::default()
                },
            },
        );
        let mut first = OverlayContent::new(OverlayKind::LowerThird, "Ryan Perez");
        first.blocks.push(OverlayBlock {
            role: "body".into(),
            text: "Documentary filmmaker".into(),
        });
        let second = OverlayContent::new(OverlayKind::Stat, "84%");
        let camera = RgbaFrame {
            width: 1280,
            height: 720,
            pixels: [20, 80, 140, 255].repeat(1280 * 720),
        };
        let size = Orientation::Horizontal.output_size();
        let mut assets = OverlayAssets::new();
        assets.load_content(&store, &insert).unwrap();
        let mut renderer = OverlayRenderer::new().unwrap();
        let active = |ms| {
            [
                ActiveOverlay {
                    content: &insert,
                    elapsed_ms: ms,
                    duration_ms: 6000,
                },
                ActiveOverlay {
                    content: &first,
                    elapsed_ms: ms,
                    duration_ms: 6000,
                },
                ActiveOverlay {
                    content: &second,
                    elapsed_ms: ms,
                    duration_ms: 6000,
                },
            ]
        };
        for ms in [0, 500, 1000] {
            renderer
                .render_scene_live(
                    size,
                    &camera,
                    &active(ms),
                    &assets,
                    RenderOptions::default(),
                )
                .unwrap();
        }
        PROFILE.store(true, std::sync::atomic::Ordering::Relaxed);
        renderer
            .render_scene_live(
                size,
                &camera,
                &active(1500),
                &assets,
                RenderOptions::default(),
            )
            .unwrap();
        PROFILE.store(false, std::sync::atomic::Ordering::Relaxed);
        let mut times = Vec::new();
        for frame in 0..30 {
            let start = std::time::Instant::now();
            let result = renderer
                .render_scene_live(
                    size,
                    &camera,
                    &active(frame * 33),
                    &assets,
                    RenderOptions::default(),
                )
                .unwrap();
            std::hint::black_box(result);
            times.push(start.elapsed().as_secs_f64() * 1000.);
        }
        times.sort_by(f64::total_cmp);
        eprintln!(
            "cached scene insert+2cards1920x1080,30frames: mean={:.3}ms median={:.3}ms p95={:.3}ms max={:.3}ms",
            times.iter().sum::<f64>() / 30.,
            times[15],
            times[28],
            times[29]
        );
    }
}
#[cfg(test)]
mod equivalence {
    use super::*;
    use crate::{InsertContent, InsertPlacement, InsertSource, OverlayBlock, ZoomPoint, ZoomSpec};
    #[test]
    #[ignore = "explicit full-resolution exact scene optimization/reference proof"]
    fn scene_optimization_matches_full_mask_and_full_card_canvas_reference() {
        let temp = tempfile::tempdir().unwrap();
        let (store, _) =
            ProjectStore::create(temp.path().join("reference"), "Scene reference").unwrap();
        image::RgbaImage::from_fn(96, 64, |x, y| {
            image::Rgba([
                (x * 2) as u8,
                (y * 3) as u8,
                90,
                if x < 48 { 180 } else { 255 },
            ])
        })
        .save(store.root().join("media/photo.png"))
        .unwrap();
        let camera = RgbaFrame {
            width: 64,
            height: 48,
            pixels: [20, 80, 140, 255].repeat(64 * 48),
        };
        let mut renderer = OverlayRenderer::new().unwrap();
        let mut first = OverlayContent::new(OverlayKind::LowerThird, "Exact approved words");
        first.blocks.push(OverlayBlock {
            role: "body".into(),
            text: "Preserve alpha and clipped edges.".into(),
        });
        let second = OverlayContent::new(OverlayKind::Quote, "A quotation");
        let mut count = 0;
        for shape in [Orientation::Horizontal, Orientation::Vertical] {
            for placement in [
                InsertPlacement::Fullscreen,
                InsertPlacement::Beside,
                InsertPlacement::Behind,
            ] {
                let insert = OverlayContent::from_insert(
                    "Generated insert",
                    InsertContent {
                        source: InsertSource::Photo {
                            path: "media/photo.png".into(),
                        },
                        placement,
                        zoom: ZoomSpec {
                            end: ZoomPoint {
                                center_x: 730,
                                center_y: 350,
                                percent: 250,
                            },
                            ..ZoomSpec::default()
                        },
                    },
                );
                let mut assets = OverlayAssets::new();
                assets.load_content(&store, &insert).unwrap();
                for ms in [0, 117, 350, 700, 2500, 5000] {
                    let active = [
                        ActiveOverlay {
                            content: &insert,
                            elapsed_ms: ms,
                            duration_ms: 6000,
                        },
                        ActiveOverlay {
                            content: &first,
                            elapsed_ms: ms,
                            duration_ms: 6000,
                        },
                        ActiveOverlay {
                            content: &second,
                            elapsed_ms: ms,
                            duration_ms: 6000,
                        },
                    ];
                    renderer.scene_reference = true;
                    let reference = renderer
                        .render_scene_live(
                            shape.output_size(),
                            &camera,
                            &active,
                            &assets,
                            RenderOptions::default(),
                        )
                        .unwrap();
                    renderer.scene_reference = false;
                    let actual = renderer
                        .render_scene_live(
                            shape.output_size(),
                            &camera,
                            &active,
                            &assets,
                            RenderOptions::default(),
                        )
                        .unwrap();
                    let first = actual
                        .pixels
                        .iter()
                        .zip(&reference.pixels)
                        .position(|(a, b)| a != b);
                    assert!(
                        actual == reference,
                        "scene reference mismatch for {shape:?}/{placement:?}/{ms}; first differing byte={first:?}"
                    );
                    count += 1;
                }
            }
        }
        eprintln!("Exact scene optimization/reference comparisons={count}");
    }
}
#[cfg(test)]
mod compositing {
    use super::*;
    use crate::{InsertContent, InsertPlacement, InsertSource, ZoomSpec};
    #[test]
    fn identity_card_compositor_matches_tiny_skia_for_every_alpha_and_gray() {
        for opaque in [false, true] {
            let mut source = Pixmap::new(256, 256).unwrap();
            let mut destination = Pixmap::new(256, 256).unwrap();
            for (index, (src, dst)) in source
                .data_mut()
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(destination.data_mut().as_chunks_mut::<4>().0)
                .enumerate()
            {
                let a = (index % 256) as u8;
                let color = (index / 256) as u8;
                *src = [a / 2, a / 3, a, a];
                *dst = if opaque {
                    [color, 255 - color, color.wrapping_mul(37), 255]
                } else {
                    [color / 2, color / 3, color, color]
                };
            }
            let mut expected = destination.clone();
            expected.draw_pixmap(
                0,
                0,
                source.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
            compose_cards(&mut destination, &source).unwrap();
            let first = destination
                .data()
                .iter()
                .zip(expected.data())
                .position(|(a, b)| a != b);
            assert!(
                destination.data() == expected.data(),
                "identity alpha composition differs from the pinned tiny-skia path; opaque={opaque}, first byte={first:?}"
            );
        }
    }
    #[test]
    fn validated_insert_caches_never_accept_a_changed_untrusted_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let (store, _) = ProjectStore::create(temp.path().join("cache"), "Cache").unwrap();
        image::RgbaImage::from_pixel(16, 16, image::Rgba([120, 96, 181, 255]))
            .save(store.root().join("media/photo.png"))
            .unwrap();
        let content = OverlayContent::from_insert(
            "Approved photo",
            InsertContent {
                source: InsertSource::Photo {
                    path: "media/photo.png".into(),
                },
                placement: InsertPlacement::Fullscreen,
                zoom: ZoomSpec::default(),
            },
        );
        let mut assets = OverlayAssets::new();
        assets.load_content(&store, &content).unwrap();
        let camera = RgbaFrame {
            width: 16,
            height: 9,
            pixels: [20, 80, 140, 255].repeat(16 * 9),
        };
        let mut renderer = OverlayRenderer::new().unwrap();
        renderer
            .render_scene_live(
                Orientation::Horizontal.output_size(),
                &camera,
                &[ActiveOverlay {
                    content: &content,
                    elapsed_ms: 0,
                    duration_ms: 6000,
                }],
                &assets,
                RenderOptions::default(),
            )
            .unwrap();
        let mut bad = content.clone();
        bad.insert.as_mut().unwrap().zoom.end.percent = 251;
        let mut secret = content.clone();
        secret.title = "api_key=sk-ant-very-private-key-value-do-not-save".into();
        for changed in [bad, secret] {
            assert!(assets.load_content(&store, &changed).is_err());
            assert!(
                renderer
                    .render_scene_live(
                        Orientation::Horizontal.output_size(),
                        &camera,
                        &[ActiveOverlay {
                            content: &changed,
                            elapsed_ms: 0,
                            duration_ms: 6000
                        }],
                        &assets,
                        RenderOptions::default()
                    )
                    .is_err()
            );
        }
        assert_eq!(assets.validated.len(), 1);
        assert_eq!(renderer.validated_inserts.len(), 1);
    }
}
