use crate::{assets, config::Settings, core::contracts::Error};
use image::{Rgb, RgbImage};

// Compatibility with the pinned Pillow8-bit separable bilinear rounding.
// Retain until model preprocessing is independently requalified after cutover.
fn coefficients(input: usize, output: usize) -> Vec<(usize, Vec<i64>)> {
    let scale = input as f64 / output as f64;
    let filter_scale = scale.max(1.0);
    (0..output)
        .map(|index| {
            let center = (index as f64 + 0.5) * scale;
            let start = ((center - filter_scale + 0.5) as isize).max(0) as usize;
            let end = ((center + filter_scale + 0.5) as usize).min(input);
            let weights: Vec<f64> = (start..end)
                .map(|x| (1.0 - ((x as f64 - center + 0.5) / filter_scale).abs()).max(0.0))
                .collect();
            let total: f64 = weights.iter().sum();
            (
                start,
                weights
                    .iter()
                    .map(|w| (0.5 + w / total * ((1u64 << 22) as f64)) as i64)
                    .collect(),
            )
        })
        .collect()
}
fn resize(source: &RgbImage, width: u32, height: u32) -> RgbImage {
    let horizontal = coefficients(source.width() as usize, width as usize);
    let vertical = coefficients(source.height() as usize, height as usize);
    let mut temporary = RgbImage::new(width, source.height());
    for y in 0..source.height() {
        for (x, (start, weights)) in horizontal.iter().enumerate() {
            let mut pixel = [0; 3];
            for (channel, value) in pixel.iter_mut().enumerate() {
                let sum = weights
                    .iter()
                    .enumerate()
                    .fold(1i64 << 21, |sum, (i, weight)| {
                        sum + source.get_pixel((*start + i) as u32, y)[channel] as i64 * weight
                    });
                *value = (sum >> 22).clamp(0, 255) as u8;
            }
            temporary.put_pixel(x as u32, y, Rgb(pixel));
        }
    }
    let mut result = RgbImage::new(width, height);
    for (y, (start, weights)) in vertical.iter().enumerate() {
        for x in 0..width {
            let mut pixel = [0; 3];
            for (channel, value) in pixel.iter_mut().enumerate() {
                let sum = weights
                    .iter()
                    .enumerate()
                    .fold(1i64 << 21, |sum, (i, weight)| {
                        sum + temporary.get_pixel(x, (*start + i) as u32)[channel] as i64 * weight
                    });
                *value = (sum >> 22).clamp(0, 255) as u8;
            }
            result.put_pixel(x, y as u32, Rgb(pixel));
        }
    }
    result
}

pub fn preprocess(content: &[u8], settings: &Settings) -> Result<(Vec<f32>, [usize; 4]), Error> {
    let source = assets::image(content, settings)?;
    let resized_width = (48.0 * source.width() as f64 / source.height() as f64)
        .ceil()
        .max(1.0) as usize;
    let width = resized_width.max(320);
    if width > 2048 {
        return Err(Error::new(
            "invalid_asset",
            "Text-line aspect ratio exceeds the model width budget.",
        ));
    }
    let image = resize(&source, resized_width as u32, 48);
    let mut batch = vec![0.0; 3 * 48 * width];
    for y in 0..48 {
        for x in 0..resized_width {
            for channel in 0..3 {
                batch[channel * 48 * width + y * width + x] =
                    (image.get_pixel(x as u32, y as u32)[2 - channel] as f32 / 255.0 - 0.5) / 0.5;
            }
        }
    }
    Ok((batch, [1, 3, 48, width]))
}
