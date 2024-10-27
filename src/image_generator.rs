use magick_rust::{
    DrawingWand,
    MagickWand,
    PixelWand,
    MagickError,
    GravityType,
};

pub async fn create_text_image(
    text: Option<&String>,
    background_color: &str,
    foreground_color: &str,
    width: usize,
    height: usize,
    font_size: f64,
) -> Result<Vec<u8>, MagickError> {
    let mut background = PixelWand::new();
    background.set_color(background_color)?;

    let mut foreground_pixel = PixelWand::new();
    foreground_pixel.set_color(foreground_color)?;

    let mut magick = MagickWand::new();
    magick.new_image(width, height, &background)?;

    if let Some(text) = text {
        let mut foreground = DrawingWand::new();
        foreground.set_fill_color(&foreground_pixel);
        foreground.set_gravity(GravityType::Center);
        foreground.set_font("DejaVu-Sans")?;
        foreground.set_font_size(font_size);
        foreground.draw_annotation(0.0, 0.0, text)?;
        magick.set_gravity(GravityType::Center)?;
        magick.draw_image(&foreground)?;
    }

    magick.write_image_blob("PNG")
}
