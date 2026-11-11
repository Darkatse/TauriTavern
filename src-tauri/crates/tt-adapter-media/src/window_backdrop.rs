//! Render transparent wallpaper strips behind native system bars.
use std::{io::Cursor, sync::Arc};

use image::{
    DynamicImage, GenericImageView, ImageDecoder, ImageFormat, ImageReader, Rgba, RgbaImage,
};
use tt_contracts::{
    client_asset_paths::parse_user_data_asset_request_path,
    window_layout::{WindowSnapshot, WindowWallpaper},
};
use tt_domain::errors::DomainError;
use tt_ports::host_resource::{
    HostResourceAssetStore, HostResourceSourceRequest, HostResourceStoreError,
};

pub struct WindowBackdropRenderer {
    resources: Arc<dyn HostResourceAssetStore>,
    render_lock: Arc<tokio::sync::Mutex<()>>,
}

pub struct BackdropStrip {
    pub edge: StripEdge,
    /// Mean premultiplied sRGB and alpha, all in 0..1, including transparent pixels.
    pub average: [f32; 4],
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub png: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StripEdge {
    Top,
    Bottom,
    Left,
    Right,
}

impl WindowBackdropRenderer {
    pub fn new(resources: Arc<dyn HostResourceAssetStore>) -> Self {
        Self {
            resources,
            render_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub async fn render(
        &self,
        window: WindowSnapshot,
        wallpaper: WindowWallpaper,
    ) -> Result<Vec<BackdropStrip>, DomainError> {
        // ponytail: queued requests still render; coalesce only if wallpaper/resize bursts
        // measurably delay the latest result. State acceptance never waits on this lock.
        let guard = self.render_lock.clone().lock_owned().await;
        let resources = self.resources.clone();
        tokio::task::spawn_blocking(move || {
            // The blocking task owns the guard even if its async caller is dropped.
            let _guard = guard;
            render(resources.as_ref(), &window, &wallpaper)
        })
        .await
        .map_err(|error| internal(format!("render task: {error}")))?
    }
}

fn failure(message: impl std::fmt::Display) -> DomainError {
    DomainError::InvalidData(format!("Window backdrop: {message}"))
}

fn render(
    resources: &dyn HostResourceAssetStore,
    window: &WindowSnapshot,
    wallpaper: &WindowWallpaper,
) -> Result<Vec<BackdropStrip>, DomainError> {
    let inset = window.insets;
    if window.width == 0
        || window.height == 0
        || !window.scale.is_finite()
        || window.scale <= 0.0
        || u64::from(inset.left) + u64::from(inset.right) > u64::from(window.width)
        || u64::from(inset.top) + u64::from(inset.bottom) > u64::from(window.height)
    {
        return Err(failure("invalid window geometry"));
    }
    let rects = [
        (StripEdge::Top, 0, 0, window.width, inset.top),
        (
            StripEdge::Bottom,
            0,
            window.height - inset.bottom,
            window.width,
            inset.bottom,
        ),
        (
            StripEdge::Left,
            0,
            inset.top,
            inset.left,
            window.height - inset.top - inset.bottom,
        ),
        (
            StripEdge::Right,
            window.width - inset.right,
            inset.top,
            inset.right,
            window.height - inset.top - inset.bottom,
        ),
    ];
    let Some(path) = &wallpaper.resource_path else {
        return Ok(Vec::new());
    };
    if rects
        .iter()
        .all(|(_, _, _, width, height)| *width == 0 || *height == 0)
    {
        return Ok(Vec::new());
    }
    let asset = parse_user_data_asset_request_path(path)
        .map_err(|error| failure(format!("invalid resource path: {error:?}")))?
        .ok_or_else(|| failure("unsupported background resource"))?;
    let bytes = resources
        .open(HostResourceSourceRequest::UserData {
            kind: asset.kind,
            relative_path: &asset.relative_path,
        })
        .and_then(|resource| resource.read(None))
        .map_err(|error| match error {
            HostResourceStoreError::NotFound(message) => DomainError::NotFound(message),
            HostResourceStoreError::Forbidden(message) => failure(message),
            HostResourceStoreError::Internal(message) => internal(message),
        })?;
    let mut decoder = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(failure)?
        .into_decoder()
        .map_err(failure)?;
    let orientation = decoder.orientation().map_err(failure)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(failure)?;
    image.apply_orientation(orientation);
    let area = [
        window.width as f64 / window.scale,
        window.height as f64 / window.scale,
    ];
    let intrinsic = [image.width() as f64, image.height() as f64];
    let size = background_size(&wallpaper.size, area, intrinsic)?;
    let position: Vec<_> = wallpaper.position.split_whitespace().collect();
    if position.len() != 2 {
        return Err(failure("unsupported background-position"));
    }
    let origin = [
        length(position[0], area[0] - size[0])?,
        length(position[1], area[1] - size[1])?,
    ];
    let mut strips = Vec::new();
    for (edge, x, y, width, height) in rects {
        if width == 0 || height == 0 {
            continue;
        }
        let (strip, average) =
            paint_strip(&image, (x, y, width, height), window.scale, size, origin);
        let mut png = Cursor::new(Vec::new());
        strip
            .write_to(&mut png, ImageFormat::Png)
            .map_err(|error| internal(format!("encode strip PNG: {error}")))?;
        strips.push(BackdropStrip {
            edge,
            average,
            x,
            y,
            width,
            height,
            png: png.into_inner(),
        });
    }
    Ok(strips)
}

fn internal(message: impl std::fmt::Display) -> DomainError {
    DomainError::InternalError(format!("Window backdrop: {message}"))
}

// Browsers interpolate premultiplied color. Convert only the sampled pixels so
// transparent edges match CSS without allocating a second full-size image.
struct PremultipliedImage<'a>(&'a DynamicImage);

impl GenericImageView for PremultipliedImage<'_> {
    type Pixel = Rgba<f32>;

    fn dimensions(&self) -> (u32, u32) {
        self.0.dimensions()
    }

    fn get_pixel(&self, x: u32, y: u32) -> Self::Pixel {
        let Rgba([r, g, b, a]) = self.0.get_pixel(x, y);
        let alpha = f32::from(a) / 255.0;
        Rgba([
            f32::from(r) * alpha,
            f32::from(g) * alpha,
            f32::from(b) * alpha,
            alpha,
        ])
    }
}

fn paint_strip(
    image: &DynamicImage,
    rect: (u32, u32, u32, u32),
    scale: f64,
    size: [f64; 2],
    origin: [f64; 2],
) -> (RgbaImage, [f32; 4]) {
    let (x, y, width, height) = rect;
    let intrinsic = [image.width() as f64, image.height() as f64];
    let samples = PremultipliedImage(image);
    let mut strip = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));
    let mut sum = [0.0_f64; 4];
    for (px, py, output) in strip.enumerate_pixels_mut() {
        // Map physical pixel centers into the CSS background rectangle.
        let local = [
            (x + px) as f64 / scale + 0.5 / scale - origin[0],
            (y + py) as f64 / scale + 0.5 / scale - origin[1],
        ];
        if local[0] < 0.0 || local[1] < 0.0 || local[0] >= size[0] || local[1] >= size[1] {
            continue;
        }
        let sx = (local[0] * intrinsic[0] / size[0] - 0.5).clamp(0.0, intrinsic[0] - 1.0);
        let sy = (local[1] * intrinsic[1] / size[1] - 0.5).clamp(0.0, intrinsic[1] - 1.0);
        let Rgba([r, g, b, alpha]) =
            image::imageops::interpolate_bilinear(&samples, sx as f32, sy as f32)
                .expect("sample is inside the decoded image");
        for (total, value) in sum.iter_mut().zip([r, g, b, alpha]) {
            *total += f64::from(value);
        }
        if alpha > 0.0 {
            *output = Rgba([
                (r / alpha).round() as u8,
                (g / alpha).round() as u8,
                (b / alpha).round() as u8,
                (alpha * 255.0).round() as u8,
            ]);
        }
    }
    for channel in &mut sum[..3] {
        *channel /= 255.0;
    }
    // Unpainted pixels contribute zero, but still occupy part of the strip.
    let count = f64::from(width) * f64::from(height);
    (strip, sum.map(|value| (value / count) as f32))
}

fn length(value: &str, reference: f64) -> Result<f64, DomainError> {
    let number = |value: &str| value.parse::<f64>().map_err(failure);
    let result = if let Some(value) = value.strip_suffix('%') {
        number(value)? * reference / 100.0
    } else if let Some(value) = value.strip_suffix("px") {
        number(value)?
    } else {
        return Err(failure(format!("unsupported CSS length: {value}")));
    };
    if !result.is_finite() {
        return Err(failure("non-finite CSS length"));
    }
    Ok(result)
}

fn background_size(value: &str, area: [f64; 2], image: [f64; 2]) -> Result<[f64; 2], DomainError> {
    if value == "cover" || value == "contain" {
        let ratios = [area[0] / image[0], area[1] / image[1]];
        let scale = if value == "cover" {
            ratios[0].max(ratios[1])
        } else {
            ratios[0].min(ratios[1])
        };
        return Ok([image[0] * scale, image[1] * scale]);
    }
    let parts: Vec<_> = value.split_whitespace().collect();
    if parts.is_empty() || parts.len() > 2 {
        return Err(failure("unsupported background-size"));
    }
    let component = |index: usize| -> Result<Option<f64>, DomainError> {
        match parts.get(index).copied().unwrap_or("auto") {
            "auto" => Ok(None),
            value => length(value, area[index]).map(Some),
        }
    };
    let size = match (component(0)?, component(1)?) {
        (None, None) => image,
        (Some(w), None) => [w, w * image[1] / image[0]],
        (None, Some(h)) => [h * image[0] / image[1], h],
        (Some(w), Some(h)) => [w, h],
    };
    if size.iter().any(|value| *value <= 0.0) {
        return Err(failure("empty background-size"));
    }
    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_preserves_alpha_and_samples_pixel_centers() {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_fn(2, 1, |x, _| {
            if x == 0 {
                Rgba([80, 0, 0, 128])
            } else {
                Rgba([0, 80, 0, 255])
            }
        }));
        let (strip, average) = paint_strip(&image, (0, 0, 5, 1), 1.0, [2.0, 1.0], [1.0, 0.0]);
        assert_eq!(
            strip.into_raw(),
            [
                0, 0, 0, 0, 80, 0, 0, 128, 0, 80, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0,
            ]
        );
        let expected = [
            80.0 / 255.0 * 128.0 / 255.0,
            80.0 / 255.0,
            0.0,
            128.0 / 255.0 + 1.0,
        ];
        for (actual, total) in average.into_iter().zip(expected) {
            assert!((actual - total / 5.0).abs() < 1e-6);
        }

        let edge = DynamicImage::ImageRgba8(RgbaImage::from_fn(2, 1, |x, _| {
            if x == 0 {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([0, 255, 0, 0])
            }
        }));
        let (middle, _) = paint_strip(&edge, (1, 0, 1, 1), 1.0, [3.0, 1.0], [0.0, 0.0]);
        assert_eq!(middle.get_pixel(0, 0), &Rgba([255, 0, 0, 128]));
    }

    #[test]
    fn css_fit_preserves_aspect_ratio_and_percent_position_uses_remaining_space() {
        let area = [400.0, 800.0];
        let image = [1200.0, 600.0];
        assert_eq!(
            background_size("cover", area, image).unwrap(),
            [1600.0, 800.0]
        );
        assert_eq!(length("50%", 400.0 - 1600.0).unwrap(), -600.0);
        assert_eq!(
            background_size("contain", area, image).unwrap(),
            [400.0, 200.0]
        );
        assert_eq!(
            background_size("100% auto", area, image).unwrap(),
            [400.0, 200.0]
        );
    }
}
