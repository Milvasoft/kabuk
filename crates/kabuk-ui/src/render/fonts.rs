//! The fonts shipped in `assets/fonts` (OFL), loaded from memory: they are never
//! installed into Windows. Only the families of the current look are loaded.

use windows::Win32::Graphics::DirectWrite::{IDWriteFactory5, IDWriteFontCollection1, IDWriteInMemoryFontFileLoader};

use super::{Context, Result};

/// `(file name, family, bytes)` of every embedded font file.
pub const FONT_FILES: &[(&str, &str, &[u8])] = &[
    (
        "Archivo-Variable.ttf",
        "Archivo",
        include_bytes!("../../../../assets/fonts/Archivo-Variable.ttf"),
    ),
    (
        "Cantarell-Bold.ttf",
        "Cantarell",
        include_bytes!("../../../../assets/fonts/Cantarell-Bold.ttf"),
    ),
    (
        "Cantarell-Regular.ttf",
        "Cantarell",
        include_bytes!("../../../../assets/fonts/Cantarell-Regular.ttf"),
    ),
    (
        "ChakraPetch-Bold.ttf",
        "Chakra Petch",
        include_bytes!("../../../../assets/fonts/ChakraPetch-Bold.ttf"),
    ),
    (
        "ChakraPetch-Medium.ttf",
        "Chakra Petch",
        include_bytes!("../../../../assets/fonts/ChakraPetch-Medium.ttf"),
    ),
    (
        "ChakraPetch-Regular.ttf",
        "Chakra Petch",
        include_bytes!("../../../../assets/fonts/ChakraPetch-Regular.ttf"),
    ),
    (
        "ChakraPetch-SemiBold.ttf",
        "Chakra Petch",
        include_bytes!("../../../../assets/fonts/ChakraPetch-SemiBold.ttf"),
    ),
    (
        "Figtree-Variable.ttf",
        "Figtree",
        include_bytes!("../../../../assets/fonts/Figtree-Variable.ttf"),
    ),
    (
        "Geist-Variable.ttf",
        "Geist",
        include_bytes!("../../../../assets/fonts/Geist-Variable.ttf"),
    ),
    (
        "GeistMono-Variable.ttf",
        "Geist Mono",
        include_bytes!("../../../../assets/fonts/GeistMono-Variable.ttf"),
    ),
    (
        "IBMPlexMono-Medium.ttf",
        "IBM Plex Mono",
        include_bytes!("../../../../assets/fonts/IBMPlexMono-Medium.ttf"),
    ),
    (
        "IBMPlexMono-Regular.ttf",
        "IBM Plex Mono",
        include_bytes!("../../../../assets/fonts/IBMPlexMono-Regular.ttf"),
    ),
    (
        "IBMPlexSansCondensed-Medium.ttf",
        "IBM Plex Sans Condensed",
        include_bytes!("../../../../assets/fonts/IBMPlexSansCondensed-Medium.ttf"),
    ),
    (
        "IBMPlexSansCondensed-Regular.ttf",
        "IBM Plex Sans Condensed",
        include_bytes!("../../../../assets/fonts/IBMPlexSansCondensed-Regular.ttf"),
    ),
    (
        "IBMPlexSansCondensed-SemiBold.ttf",
        "IBM Plex Sans Condensed",
        include_bytes!("../../../../assets/fonts/IBMPlexSansCondensed-SemiBold.ttf"),
    ),
    (
        "Inter-Variable.ttf",
        "Inter",
        include_bytes!("../../../../assets/fonts/Inter-Variable.ttf"),
    ),
    (
        "JetBrainsMono-Variable.ttf",
        "JetBrains Mono",
        include_bytes!("../../../../assets/fonts/JetBrainsMono-Variable.ttf"),
    ),
    (
        "Manrope-Variable.ttf",
        "Manrope",
        include_bytes!("../../../../assets/fonts/Manrope-Variable.ttf"),
    ),
    (
        "MartianMono-Variable.ttf",
        "Martian Mono",
        include_bytes!("../../../../assets/fonts/MartianMono-Variable.ttf"),
    ),
    (
        "Newsreader-Italic-Variable.ttf",
        "Newsreader",
        include_bytes!("../../../../assets/fonts/Newsreader-Italic-Variable.ttf"),
    ),
    (
        "Newsreader-Variable.ttf",
        "Newsreader",
        include_bytes!("../../../../assets/fonts/Newsreader-Variable.ttf"),
    ),
];

/// A collection holding the embedded faces of `families`, and the loader that owns their
/// data (unregister it with [`release`] when the collection is replaced).
pub fn collection(
    factory: &IDWriteFactory5,
    families: &[&str],
) -> Result<(IDWriteFontCollection1, IDWriteInMemoryFontFileLoader)> {
    // SAFETY: plain factory calls; the loader is registered before use and font data
    // is copied by DirectWrite (no owner object is passed).
    unsafe {
        let loader = factory.CreateInMemoryFontFileLoader().ctx("font loader")?;
        factory.RegisterFontFileLoader(&loader).ctx("register font loader")?;
        let builder = factory.CreateFontSetBuilder().ctx("font set builder")?;
        for (name, family, data) in FONT_FILES {
            if !families.contains(family) {
                continue;
            }
            let file = loader
                .CreateInMemoryFontFileReference(
                    factory,
                    data.as_ptr().cast(),
                    u32::try_from(data.len()).unwrap_or_default(),
                    None,
                )
                .ctx("font file reference")?;
            if let Err(e) = builder.AddFontFile(&file) {
                tracing::error!(font = name, error = %e, "font file rejected");
            }
        }
        let set = builder.CreateFontSet().ctx("font set")?;
        Ok((
            factory.CreateFontCollectionFromFontSet(&set).ctx("font collection")?,
            loader,
        ))
    }
}

/// Unregisters a loader whose collection is no longer used, freeing its font data.
pub fn release(factory: &IDWriteFactory5, loader: &IDWriteInMemoryFontFileLoader) {
    // SAFETY: the loader was registered with this factory in `collection`.
    let _ = unsafe { factory.UnregisterFontFileLoader(loader) };
}

#[cfg(test)]
mod tests {
    //! The table lists exactly the font files in `assets/fonts`; every family has an OFL
    //! licence file next to it.
    use super::*;

    #[test]
    fn table_matches_the_assets_directory_and_licences_exist() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts");
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .collect();
        let mut fonts: Vec<&String> = names
            .iter()
            .filter(|n| n.ends_with(".ttf") || n.ends_with(".otf"))
            .collect();
        fonts.sort();
        let mut listed: Vec<String> = FONT_FILES.iter().map(|(n, _, _)| n.to_string()).collect();
        listed.sort();
        assert_eq!(fonts.into_iter().cloned().collect::<Vec<_>>(), listed);
        for (_, family, _) in FONT_FILES {
            let licence = format!("OFL-{}.txt", family.replace(' ', ""));
            assert!(names.contains(&licence), "missing {licence}");
        }
    }
}
