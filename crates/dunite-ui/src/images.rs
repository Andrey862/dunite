//! Загрузка и растровые изображения для Live Preview (SPEC §7.5, §8.2).
//!
//! Декодирование идёт в фоновом потоке, результат кэшируется в Workspace.
//! Форматы: PNG, JPEG, GIF (первый кадр), WebP. SVG — вне MVP (нужен
//! отдельный растеризатор).

use std::sync::Arc;

use gpui::RenderImage;

/// Результат декодирования: размер и рендер-текстура gpui.
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub render: Arc<RenderImage>,
}

/// Максимальная высота отображаемой картинки (SPEC §8.2).
pub const MAX_IMAGE_HEIGHT: f32 = 240.;

/// Декодирует байты изображения в текстуру gpui.
pub fn decode_image(bytes: &[u8]) -> Option<DecodedImage> {
    let image = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    let frame = image::Frame::new(image);
    Some(DecodedImage {
        width,
        height,
        render: Arc::new(RenderImage::new(vec![frame])),
    })
}

/// Размеры для отображения: масштаб по доступной ширине, ограничение высоты
/// (SPEC §8.2: «масштабируется по ширине, с ограничением высоты»).
pub fn fit_size(width: u32, height: u32, avail_width: f32, max_height: f32) -> (f32, f32) {
    if width == 0 || height == 0 {
        return (0., 0.);
    }
    let natural_w = width as f32;
    let natural_h = height as f32;
    let mut scale = (avail_width / natural_w).min(1.0);
    if natural_h * scale > max_height {
        scale = max_height / natural_h;
    }
    (natural_w * scale, natural_h * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_scales_down_to_width() {
        let (w, h) = fit_size(400, 200, 200., 240.);
        assert_eq!((w, h), (200., 100.));
    }

    #[test]
    fn fit_caps_height() {
        let (w, h) = fit_size(100, 400, 500., 240.);
        assert!((w - 60.).abs() < 0.01 && (h - 240.).abs() < 0.01, "{w}x{h}");
    }

    #[test]
    fn fit_never_upscales() {
        let (w, h) = fit_size(50, 25, 500., 240.);
        assert_eq!((w, h), (50., 25.));
    }

    #[test]
    fn decode_png_roundtrip() {
        // PNG генерируем самим крейтом image — без рукописных байтов
        let img = image::RgbaImage::from_pixel(3, 2, image::Rgba([255, 0, 0, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let decoded = decode_image(bytes.get_ref()).expect("png декодируется");
        assert_eq!((decoded.width, decoded.height), (3, 2));
    }

    #[test]
    fn decode_garbage_fails() {
        assert!(decode_image("не изображение".as_bytes()).is_none());
    }
}
