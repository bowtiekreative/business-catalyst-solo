//! Typed visual inserts and absolute-time zoom. File preparation is explicit and
//! blocking; scene rendering itself does not read files or start processes.
mod decode;
mod video;
pub(crate) use decode::FrameReader;
pub(crate) use video::VideoCursor;
pub(crate) use video::inspect_video as probe_video;
pub use video::{InsertVideo, import_video, inspect_video, prepare_video};

use crate::{OutputSize, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsertPlacement {
    Fullscreen,
    Beside,
    Behind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoomEasing {
    Smooth,
    Steady,
    Snappy,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZoomPoint {
    /// Source-relative center in thousandths,0..1000.
    pub center_x: u16,
    pub center_y: u16,
    pub percent: u16,
}
impl Default for ZoomPoint {
    fn default() -> Self {
        Self {
            center_x: 500,
            center_y: 500,
            percent: 100,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZoomSpec {
    pub start: ZoomPoint,
    pub end: ZoomPoint,
    pub duration_ms: u64,
    pub easing: ZoomEasing,
}
impl Default for ZoomSpec {
    fn default() -> Self {
        Self {
            start: ZoomPoint::default(),
            end: ZoomPoint::default(),
            duration_ms: 5000,
            easing: ZoomEasing::Smooth,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InsertSource {
    Photo { path: String },
    Video { path: String, source_in_ms: u64 },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InsertContent {
    pub source: InsertSource,
    pub placement: InsertPlacement,
    pub zoom: ZoomSpec,
}
impl InsertContent {
    pub fn validate(&self) -> Result<()> {
        crate::model::validate_media_path(self.path())?;
        self.zoom.validate()?;
        if let InsertSource::Video { source_in_ms, .. } = &self.source {
            frame_index(*source_in_ms)?;
        }
        Ok(())
    }
    pub fn path(&self) -> &str {
        match &self.source {
            InsertSource::Photo { path } | InsertSource::Video { path, .. } => path,
        }
    }
    pub fn source_frame(&self, elapsed_ms: u64) -> Result<Option<u64>> {
        match self.source {
            InsertSource::Photo { .. } => Ok(None),
            InsertSource::Video { source_in_ms, .. } => Ok(Some(frame_index(
                source_in_ms
                    .checked_add(elapsed_ms)
                    .ok_or("Insert source time overflows")?,
            )?)),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneLayout {
    pub media: Viewport,
    pub camera: Option<Viewport>,
}
impl SceneLayout {
    pub fn new(output: OutputSize, camera: OutputSize, placement: InsertPlacement) -> Result<Self> {
        dimensions(output)?;
        dimensions(camera)?;
        let w = f64::from(output.width);
        let h = f64::from(output.height);
        let full = Viewport {
            x: 0.,
            y: 0.,
            width: w,
            height: h,
        };
        let (media, camera) = match placement {
            InsertPlacement::Fullscreen => (full, None),
            InsertPlacement::Beside if w >= h => (
                Viewport {
                    x: w / 2.,
                    width: w / 2.,
                    ..full
                },
                Some(Viewport {
                    width: w / 2.,
                    ..full
                }),
            ),
            InsertPlacement::Beside => (
                Viewport {
                    y: h / 2.,
                    height: h / 2.,
                    ..full
                },
                Some(Viewport {
                    height: h / 2.,
                    ..full
                }),
            ),
            InsertPlacement::Behind => {
                let margin = w.min(h) * 0.035;
                let scale =
                    (w * 0.32 / f64::from(camera.width)).min(h * 0.36 / f64::from(camera.height));
                let cw = f64::from(camera.width) * scale;
                let ch = f64::from(camera.height) * scale;
                (
                    full,
                    Some(Viewport {
                        x: margin,
                        y: h - margin - ch,
                        width: cw,
                        height: ch,
                    }),
                )
            }
        };
        Ok(Self { media, camera })
    }
}
impl ZoomPoint {
    fn validate(self) -> Result<()> {
        if self.center_x > 1000 || self.center_y > 1000 || !(100..=250).contains(&self.percent) {
            return Err(
                "Zoom must stay between100% and250%, with its center inside the image".into(),
            );
        }
        Ok(())
    }
    pub fn from_viewport(source: OutputSize, target: OutputSize, view: Viewport) -> Result<Self> {
        dimensions(source)?;
        dimensions(target)?;
        let cover = cover_view(source, target)?;
        if ![view.x, view.y, view.width, view.height]
            .iter()
            .all(|v| v.is_finite())
            || view.width <= 0.
            || view.height <= 0.
            || view.x < 0.
            || view.y < 0.
            || view.x + view.width > f64::from(source.width) + 0.001
            || view.y + view.height > f64::from(source.height) + 0.001
        {
            return Err("Keep the zoom box inside the source image".into());
        }
        if (view.width / view.height - f64::from(target.width) / f64::from(target.height)).abs()
            > 0.002
        {
            return Err("Zoom box must match the output panel shape".into());
        }
        let percent = (cover.width / view.width * 100.).round();
        if !(100.0..=250.0).contains(&percent) {
            return Err("Zoom box must stay between100% and250%".into());
        }
        Ok(Self {
            center_x: ((view.x + view.width / 2.) / f64::from(source.width) * 1000.)
                .round()
                .clamp(0., 1000.) as u16,
            center_y: ((view.y + view.height / 2.) / f64::from(source.height) * 1000.)
                .round()
                .clamp(0., 1000.) as u16,
            percent: percent as u16,
        })
    }
}
impl ZoomSpec {
    pub fn validate(&self) -> Result<()> {
        self.start.validate()?;
        self.end.validate()?;
        if self.duration_ms == 0 {
            return Err("Choose a zoom duration greater than zero".into());
        }
        Ok(())
    }
    pub fn viewport(
        &self,
        source: OutputSize,
        target: OutputSize,
        elapsed_ms: u64,
        reduced_motion: bool,
    ) -> Result<Viewport> {
        self.validate()?;
        let base = cover_view(source, target)?;
        let t = if reduced_motion {
            0.
        } else {
            ease(
                self.easing,
                (elapsed_ms as f64 / self.duration_ms as f64).clamp(0., 1.),
            )
        };
        let lerp = |a: u16, b: u16| f64::from(a) + (f64::from(b) - f64::from(a)) * t;
        let zoom = lerp(self.start.percent, self.end.percent) / 100.;
        let width = base.width / zoom;
        let height = base.height / zoom;
        let cx = f64::from(source.width) * lerp(self.start.center_x, self.end.center_x) / 1000.;
        let cy = f64::from(source.height) * lerp(self.start.center_y, self.end.center_y) / 1000.;
        Ok(Viewport {
            x: (cx - width / 2.).clamp(0., f64::from(source.width) - width),
            y: (cy - height / 2.).clamp(0., f64::from(source.height) - height),
            width,
            height,
        })
    }
}
pub(crate) fn dimensions(size: OutputSize) -> Result<()> {
    if size.width == 0
        || size.height == 0
        || size.width > 16384
        || size.height > 16384
        || u64::from(size.width) * u64::from(size.height) > 64 * 1024 * 1024
    {
        return Err("Image geometry exceeds its pixel resource limit".into());
    }
    Ok(())
}
pub(crate) fn cover_view(source: OutputSize, target: OutputSize) -> Result<Viewport> {
    dimensions(source)?;
    dimensions(target)?;
    let scale = (f64::from(target.width) / f64::from(source.width))
        .max(f64::from(target.height) / f64::from(source.height));
    let width = f64::from(target.width) / scale;
    let height = f64::from(target.height) / scale;
    Ok(Viewport {
        x: (f64::from(source.width) - width) / 2.,
        y: (f64::from(source.height) - height) / 2.,
        width,
        height,
    })
}
pub(crate) fn frame_index(ms: u64) -> Result<u64> {
    u64::try_from((u128::from(ms) * 30 + 15) / 1000)
        .map_err(|_| "Insert source frame overflows".into())
}
pub(crate) fn ease(kind: ZoomEasing, t: f64) -> f64 {
    match kind {
        ZoomEasing::Steady => t,
        ZoomEasing::Smooth => t * t * (3. - 2. * t),
        ZoomEasing::Snappy => {
            if t == 0. || t == 1. {
                return t;
            }
            let cubic = |v: f64, a: f64, b: f64| {
                3. * (1. - v).powi(2) * v * a + 3. * (1. - v) * v * v * b + v.powi(3)
            };
            let (mut lo, mut hi) = (0., 1.);
            for _ in 0..32 {
                let mid = (lo + hi) / 2.;
                if cubic(mid, 0.16, 0.3) < t {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            cubic((lo + hi) / 2., 1., 1.)
        }
    }
}
#[cfg(all(test, feature = "capture-test"))]
mod integration;
#[cfg(test)]
mod tests;
