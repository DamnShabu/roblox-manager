//! The app's typefaces, built into it so every machine draws the design's
//! type rather than whatever the desktop falls back to: Inter for the
//! interface, JetBrains Mono for what is typed or read as code. Written
//! where Pango can load them, and added to the font map every widget draws
//! with.
//!
//! Inter is © The Inter Project Authors and JetBrains Mono © The JetBrains
//! Mono Project Authors, both under the SIL Open Font License 1.1; the files
//! are unchanged from Inter 4.1 and JetBrains Mono 2.304, and their licences
//! sit beside them in resources/fonts.

use std::path::Path;

use gtk::prelude::WidgetExt;
use pango::prelude::FontMapExt;

/// (file name, contents)
const FONTS: &[(&str, &[u8])] = &[
    ("InterVariable.ttf", include_bytes!("../../resources/fonts/InterVariable.ttf")),
    ("JetBrainsMono.ttf", include_bytes!("../../resources/fonts/JetBrainsMono.ttf")),
];

/// Write the fonts under `dir`, unless they are there already, and add them
/// to the font map. Without them the desktop's fonts stand in, so a failure
/// is said and the app goes on.
pub fn install(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // A widget's context draws with the display's own font map.
    let map = gtk::Label::new(None)
        .pango_context()
        .font_map()
        .ok_or_else(|| "no font map to add the fonts to".to_owned())?;
    for (name, bytes) in FONTS {
        let path = dir.join(name);
        let current = std::fs::metadata(&path).is_ok_and(|m| m.len() == bytes.len() as u64);
        if !current {
            std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        map.add_font_file(&path).map_err(|e| format!("{name}: {e}"))?;
    }
    Ok(())
}
