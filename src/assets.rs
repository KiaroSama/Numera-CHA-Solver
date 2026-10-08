use crate::{config::Settings, core::contracts::Error};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Rgb, RgbImage};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Cursor, Read},
    path::Path,
};

pub fn read_asset(path: &Path, settings: &Settings) -> Result<Vec<u8>, Error> {
    settings.validate()?;
    let file = open_regular(path)
        .map_err(|_| Error::new("invalid_asset", "Asset must be a regular local file."))?;
    let mut content = Vec::new();
    file.take(settings.max_asset_bytes as u64 + 1)
        .read_to_end(&mut content)
        .map_err(|_| Error::new("invalid_asset", "Asset could not be safely read."))?;
    if content.len() > settings.max_asset_bytes {
        return Err(Error::new(
            "asset_too_large",
            "Asset exceeds the byte limit.",
        ));
    }
    Ok(content)
}

pub fn image(content: &[u8], settings: &Settings) -> Result<RgbImage, Error> {
    settings.validate()?;
    if content.is_empty() || content.len() > settings.max_asset_bytes {
        return Err(Error::new(
            "invalid_asset",
            "Image is empty or exceeds the byte limit.",
        ));
    }
    let format = image::guess_format(content).map_err(|_| decode_error())?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Bmp
    ) {
        return Err(Error::new(
            "invalid_asset",
            "Only PNG, JPEG, WEBP and BMP still images are accepted.",
        ));
    }
    let mut reader = ImageReader::with_format(Cursor::new(content), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(settings.max_pixels.min(u32::MAX as u64) as u32);
    limits.max_image_height = limits.max_image_width;
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|_| decode_error())?;
    let (width, height) = decoder.dimensions();
    if width < 2 || height < 2 || u64::from(width) * u64::from(height) > settings.max_pixels {
        return Err(Error::new(
            "invalid_asset",
            "Image dimensions exceed the supported limits.",
        ));
    }
    if frame_count(content, format)? > 1 {
        return Err(Error::new(
            "unsupported_temporal",
            "Animated images require an independently evaluated temporal engine.",
        ));
    }
    let orientation = decoder.orientation().map_err(|_| decode_error())?;
    let mut decoded = DynamicImage::from_decoder(decoder).map_err(|_| decode_error())?;
    decoded.apply_orientation(orientation);
    // Pillow clips integer grayscale but truncates 16-bit multichannel PNG to its high byte.
    let rgba = match &decoded {
        DynamicImage::ImageLuma16(gray) => {
            image::RgbaImage::from_fn(gray.width(), gray.height(), |x, y| {
                let value = gray.get_pixel(x, y).0[0].min(255) as u8;
                image::Rgba([value, value, value, 255])
            })
        }
        DynamicImage::ImageLumaA16(gray) => {
            image::RgbaImage::from_fn(gray.width(), gray.height(), |x, y| {
                let pixel = gray.get_pixel(x, y).0;
                let value = (pixel[0] >> 8) as u8;
                image::Rgba([value, value, value, (pixel[1] >> 8) as u8])
            })
        }
        DynamicImage::ImageRgb16(rgb) => {
            image::RgbaImage::from_fn(rgb.width(), rgb.height(), |x, y| {
                let pixel = rgb.get_pixel(x, y).0;
                image::Rgba([
                    (pixel[0] >> 8) as u8,
                    (pixel[1] >> 8) as u8,
                    (pixel[2] >> 8) as u8,
                    255,
                ])
            })
        }
        DynamicImage::ImageRgba16(rgba) => {
            image::RgbaImage::from_fn(rgba.width(), rgba.height(), |x, y| {
                image::Rgba(rgba.get_pixel(x, y).0.map(|value| (value >> 8) as u8))
            })
        }
        _ => decoded.to_rgba8(),
    };
    Ok(RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let pixel = rgba.get_pixel(x, y).0;
        let alpha = u32::from(pixel[3]);
        Rgb([0, 1, 2].map(|channel| {
            ((u32::from(pixel[channel]) * alpha + 255 * (255 - alpha) + 127) / 255) as u8
        }))
    }))
}

fn decode_error() -> Error {
    Error::new("invalid_asset", "Image could not be safely decoded.")
}

// Metadata-only walks reject multiple frames before allocating or decoding their pixels.
fn frame_count(content: &[u8], format: ImageFormat) -> Result<u32, Error> {
    if format == ImageFormat::Png {
        let mut offset = 8usize;
        let mut animation = None;
        let mut default_image = false;
        let mut first_frame_control = false;
        while offset < content.len() {
            let header = content.get(offset..offset + 8).ok_or_else(decode_error)?;
            let length =
                u32::from_be_bytes(header[..4].try_into().map_err(|_| decode_error())?) as usize;
            let end = offset
                .checked_add(12)
                .and_then(|v| v.checked_add(length))
                .ok_or_else(decode_error)?;
            let data = content.get(offset + 8..end - 4).ok_or_else(decode_error)?;
            match &header[4..8] {
                b"acTL" => {
                    if data.len() != 8 {
                        return Err(decode_error());
                    }
                    animation = Some(u32::from_be_bytes(
                        data[..4].try_into().map_err(|_| decode_error())?,
                    ));
                }
                b"fcTL" => first_frame_control = true,
                b"IDAT" if !first_frame_control => default_image = true,
                b"IEND" => break,
                _ => {}
            }
            offset = end;
        }
        return Ok(animation.map_or(1, |frames| frames.saturating_add(u32::from(default_image))));
    }
    if format == ImageFormat::WebP {
        let mut offset = 12usize;
        let mut frames = 0u32;
        while offset < content.len() {
            let header = content.get(offset..offset + 8).ok_or_else(decode_error)?;
            let length =
                u32::from_le_bytes(header[4..8].try_into().map_err(|_| decode_error())?) as usize;
            let end = offset
                .checked_add(8)
                .and_then(|v| v.checked_add(length))
                .ok_or_else(decode_error)?;
            if end > content.len() {
                return Err(decode_error());
            }
            if &header[..4] == b"ANMF" {
                frames = frames.saturating_add(1);
            }
            offset = end.checked_add(length % 2).ok_or_else(decode_error)?;
        }
        return Ok(frames.max(1));
    }
    Ok(1)
}

pub(crate) fn open_regular(path: &Path) -> io::Result<File> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || is_link(&metadata) {
        return Err(io::Error::other("Not a regular local file."));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || is_link(&metadata) {
        return Err(io::Error::other("Not a regular local file."));
    }
    Ok(file)
}

pub(crate) fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
        {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}
