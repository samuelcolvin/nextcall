use ab_glyph::{FontArc, PxScale};
use image::{ImageBuffer, Rgba, RgbaImage};
use imageproc::drawing::draw_text_mut;

pub fn create_icon_with_text(text: &str) -> tray_icon::Icon {
    let large = text.len() <= 1;
    let size = 64;
    let mut img: RgbaImage = ImageBuffer::new(size, size);

    // Fill with transparent background
    for pixel in img.pixels_mut() {
        *pixel = Rgba([0, 0, 0, 0]);
    }

    // Load a basic font (using DejaVu Sans as a fallback)
    // For a production app, you'd want to embed a font file
    let font_data = include_bytes!("../assets/DejaVuSans.ttf");
    let font = FontArc::try_from_slice(font_data).expect("Failed to load font");

    // Calculate font size based on text length (2x larger)
    let font_size = if large { 90.0 } else { 60.0 };

    let scale = PxScale::from(font_size);

    // Draw text in dark gray
    draw_text_mut(
        &mut img,
        Rgba([64u8, 64u8, 64u8, 255u8]),
        0,
        if large { -12 } else { 5 },
        scale,
        &font,
        text,
    );

    tray_icon::Icon::from_rgba(img.into_raw(), size, size).expect("Failed to create icon")
}

pub fn create_icon_infinity() -> tray_icon::Icon {
    create_icon_with_text("...")
}
