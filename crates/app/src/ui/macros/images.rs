//! The images macros' `when`s look for: picked from a client, kept as PNGs
//! in the data folder by the name a macro calls each one.

use std::collections::HashMap;
use std::path::Path;

use gtk::prelude::*;
use gtk::{gdk, glib};
use rbxmgr_core::macros::grammar::{Macro, Sight};
use rbxmgr_core::macros::sight::{Image, free_image_name, image_file};

/// The image named `name`, as red, green, blue bytes.
pub fn load(dir: &Path, name: &str) -> Result<Image, String> {
    let file = image_file(dir, name);
    if !file.exists() {
        return Err(format!("there is no image named {name} -- pick its area again"));
    }
    let texture = gdk::Texture::from_filename(&file)
        .map_err(|e| format!("could not read the image {name}: {e}"))?;
    let mut downloader = gdk::TextureDownloader::new(&texture);
    downloader.set_format(gdk::MemoryFormat::R8g8b8);
    let (bytes, stride) = downloader.download_bytes();
    let (width, height) = (texture.width() as u32, texture.height() as u32);
    let row = width as usize * 3;
    let mut rgb = Vec::with_capacity(row * height as usize);
    for y in 0..height as usize {
        let line = bytes.get(y * stride..y * stride + row);
        rgb.extend_from_slice(line.ok_or_else(|| format!("the image {name} came short"))?);
    }
    Ok(Image { width, height, rgb })
}

/// Every image `m`'s `when`s name, or why each cannot be had: read before
/// it plays, on the main loop, where GDK reads them.
pub fn for_macro(dir: &Path, m: &Macro) -> HashMap<String, Result<Image, String>> {
    m.handlers
        .iter()
        .filter_map(|h| match &h.when.sight {
            Sight::Image { name, .. } => Some(name.clone()),
            Sight::Color { .. } => None,
        })
        .map(|name| {
            let image = load(dir, &name);
            (name, image)
        })
        .collect()
}

/// Every picked image, by name, or why each cannot be had: for a script,
/// which may ask for any of them. A folder not there yet holds none.
pub fn all(dir: &Path) -> HashMap<String, Result<Image, String>> {
    let Ok(entries) = std::fs::read_dir(dir) else { return HashMap::new() };
    entries
        .filter_map(|e| {
            let path = e.ok()?.path();
            let name = path.file_name()?.to_str()?.strip_suffix(".png")?.to_owned();
            Some(name)
        })
        .map(|name| {
            let image = load(dir, &name);
            (name, image)
        })
        .collect()
}

/// `image` saved under a name no other image has; that name.
pub fn save(dir: &Path, image: &Image) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("could not keep the image: {e}"))?;
    let name = free_image_name(dir);
    let bytes = glib::Bytes::from(&image.rgb);
    let texture = gdk::MemoryTexture::new(
        image.width as i32,
        image.height as i32,
        gdk::MemoryFormat::R8g8b8,
        &bytes,
        image.width as usize * 3,
    );
    texture
        .save_to_png(image_file(dir, &name))
        .map_err(|e| format!("could not keep the image: {e}"))?;
    Ok(name)
}
