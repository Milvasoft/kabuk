//! Text measurement and layout with DirectWrite. Lines are spaced like the design
//! page's CSS (`line-height: 1.55`, half-leading above and below), so text sits at the
//! same height as in the browser.

use std::cell::RefCell;
use std::collections::HashMap;

use windows::core::{Interface, HSTRING};
use windows::Win32::Graphics::DirectWrite::*;

use super::fonts;
use super::{Context, Result};
use crate::node::{FontRole, TextMeasure, TextMetrics, TextSpec};

/// `body { line-height: 1.55 }` on the design page, inherited by every bar text.
pub const CSS_LINE_HEIGHT: f32 = 1.55;
/// Grayscale text rendering tuned against Chrome's rendering of the design page
/// (compared side by side with `tools/compare-shots.py`). Chrome uses ClearType on its
/// opaque page; a transparent composition surface can only use grayscale, and this
/// gamma brings the stroke weight closest.
pub const TEXT_GAMMA: f32 = 2.2;
pub const TEXT_GRAYSCALE_CONTRAST: f32 = 1.0;

/// Cached text layouts kept at most (the bar shows a few dozen; a launcher search can
/// bring hundreds of rows).
const LAYOUT_CACHE_LIMIT: usize = 512;
/// A full cache drops this share of its entries at once (1/4), the least recently used,
/// so a flood of new texts does not rescan the cache on every miss.
const EVICT_DIVISOR: usize = 4;

/// Values by key, at most `limit` of them; when full, the least recently used quarter is
/// dropped, so the texts drawn again and again (the bar's) outlive a flood of new ones.
struct Recent<K, V> {
    entries: HashMap<K, (V, u64)>,
    /// Advances on every use; an entry keeps the time it was last used.
    clock: u64,
    limit: usize,
}

impl<K: std::hash::Hash + Eq, V> Recent<K, V> {
    fn new(limit: usize) -> Recent<K, V> {
        Recent {
            entries: HashMap::new(),
            clock: 0,
            limit: limit.max(1),
        }
    }

    /// The value of `key`, now the most recently used.
    fn get(&mut self, key: &K) -> Option<&V> {
        self.clock += 1;
        let now = self.clock;
        self.entries.get_mut(key).map(|e| {
            e.1 = now;
            &e.0
        })
    }

    fn insert(&mut self, key: K, value: V) {
        if self.entries.len() >= self.limit && !self.entries.contains_key(&key) {
            let count = (self.limit / EVICT_DIVISOR).max(1);
            let mut used: Vec<u64> = self.entries.values().map(|e| e.1).collect();
            // Uses are unique, so exactly `count` entries are at or before the cut.
            let (_, cut, _) = used.select_nth_unstable(count - 1);
            let cut = *cut;
            self.entries.retain(|_, e| e.1 > cut);
        }
        self.clock += 1;
        self.entries.insert(key, (value, self.clock));
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Font families per role, from the look's `ui` and `num` tokens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Families {
    pub ui: String,
    pub num: String,
}

#[derive(Clone, Copy)]
struct FaceMetrics {
    /// Ascent and descent as fractions of the em size.
    ascent: f32,
    descent: f32,
}

pub struct TextEngine {
    pub(crate) factory: IDWriteFactory5,
    /// The current look's faces and the loader that owns their data.
    collection: RefCell<(IDWriteFontCollection1, IDWriteInMemoryFontFileLoader)>,
    families: RefCell<Families>,
    formats: RefCell<HashMap<(String, u32, u16), IDWriteTextFormat>>,
    layouts: RefCell<Recent<TextSpec, (IDWriteTextLayout, TextMetrics)>>,
    metrics: RefCell<HashMap<(String, u16), FaceMetrics>>,
}

impl TextEngine {
    pub fn new() -> Result<TextEngine> {
        // SAFETY: plain factory creation.
        let factory: IDWriteFactory5 =
            unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.ctx("DWrite factory")?;
        let families = Families {
            ui: "Geist".into(),
            num: "Geist Mono".into(),
        };
        let collection = fonts::collection(&factory, &[&families.ui, &families.num])?;
        Ok(TextEngine {
            factory,
            collection: RefCell::new(collection),
            families: RefCell::new(families),
            formats: RefCell::new(HashMap::new()),
            metrics: RefCell::new(HashMap::new()),
            layouts: RefCell::new(Recent::new(LAYOUT_CACHE_LIMIT)),
        })
    }

    /// Rendering parameters for grayscale antialiased text on transparent surfaces.
    pub(crate) fn rendering_params(&self) -> Option<IDWriteRenderingParams> {
        // SAFETY: plain factory call.
        let p = unsafe {
            self.factory.CreateCustomRenderingParams(
                TEXT_GAMMA,
                0.0,
                TEXT_GRAYSCALE_CONTRAST,
                0.0,
                DWRITE_PIXEL_GEOMETRY_FLAT,
                DWRITE_RENDERING_MODE1_NATURAL_SYMMETRIC,
                DWRITE_GRID_FIT_MODE_DEFAULT,
            )
        };
        p.ok().map(Into::into)
    }

    /// Switches to a look's faces, loading only those font files.
    pub fn set_families(&self, families: Families) {
        if *self.families.borrow() != families {
            match fonts::collection(&self.factory, &[&families.ui, &families.num]) {
                Ok(next) => {
                    let old = std::mem::replace(&mut *self.collection.borrow_mut(), next);
                    fonts::release(&self.factory, &old.1);
                }
                Err(e) => tracing::error!(error = %e, "fonts not loaded; keeping the previous ones"),
            }
            *self.families.borrow_mut() = families;
            self.formats.borrow_mut().clear();
            self.layouts.borrow_mut().clear();
            self.metrics.borrow_mut().clear();
        }
    }

    /// True when `family` is one of the embedded faces.
    pub fn has_family(&self, family: &str) -> bool {
        let mut index = 0;
        let mut exists = windows::core::BOOL(0);
        // SAFETY: valid string and out pointers.
        unsafe {
            self.collection
                .borrow()
                .0
                .FindFamilyName(&HSTRING::from(family), &mut index, &mut exists)
        }
        .is_ok()
            && exists.as_bool()
    }

    fn family(&self, role: FontRole) -> String {
        let f = self.families.borrow();
        match role {
            FontRole::Ui => f.ui.clone(),
            FontRole::Num => f.num.clone(),
        }
    }

    fn format(&self, family: &str, size: f32, weight: u16) -> Result<IDWriteTextFormat> {
        let key = (family.to_string(), size.to_bits(), weight);
        if let Some(f) = self.formats.borrow().get(&key) {
            return Ok(f.clone());
        }
        // SAFETY: plain factory calls with valid strings.
        let format = unsafe {
            let format = self
                .factory
                .CreateTextFormat(
                    &HSTRING::from(family),
                    &self.collection.borrow().0,
                    DWRITE_FONT_WEIGHT(i32::from(weight)),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    &HSTRING::from("en-us"),
                )
                .ctx("text format")?;
            format
                .SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)
                .ctx("word wrapping")?;
            let sign = self.factory.CreateEllipsisTrimmingSign(&format).ctx("ellipsis")?;
            let trimming = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                delimiter: 0,
                delimiterCount: 0,
            };
            format.SetTrimming(&trimming, &sign).ctx("trimming")?;
            format
        };
        self.formats.borrow_mut().insert(key, format.clone());
        Ok(format)
    }

    fn face_metrics(&self, family: &str, weight: u16) -> FaceMetrics {
        let key = (family.to_string(), weight);
        if let Some(m) = self.metrics.borrow().get(&key) {
            return *m;
        }
        let read = || -> windows::core::Result<FaceMetrics> {
            let mut index = 0;
            let mut exists = windows::core::BOOL(0);
            // SAFETY: plain DirectWrite queries with valid out pointers.
            unsafe {
                self.collection
                    .borrow()
                    .0
                    .FindFamilyName(&HSTRING::from(family), &mut index, &mut exists)?;
                let fam = self
                    .collection
                    .borrow()
                    .0
                    .GetFontFamily(if exists.as_bool() { index } else { 0 })?;
                let font = fam.GetFirstMatchingFont(
                    DWRITE_FONT_WEIGHT(i32::from(weight)),
                    DWRITE_FONT_STRETCH_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                )?;
                let mut m = DWRITE_FONT_METRICS::default();
                font.GetMetrics(&mut m);
                let em = f32::from(m.designUnitsPerEm.max(1));
                Ok(FaceMetrics {
                    ascent: f32::from(m.ascent) / em,
                    descent: f32::from(m.descent) / em,
                })
            }
        };
        // Typical Latin proportions if the face cannot be read.
        let m = read().unwrap_or(FaceMetrics {
            ascent: 0.9,
            descent: 0.22,
        });
        self.metrics.borrow_mut().insert(key, m);
        m
    }

    /// A DirectWrite layout for `spec`, cached; `brushes` gives each span's drawing effect.
    pub(crate) fn layout(
        &self,
        spec: &TextSpec,
        brushes: Option<&dyn Fn(usize) -> Option<windows::core::IUnknown>>,
    ) -> Result<(IDWriteTextLayout, TextMetrics)> {
        let cached = self.layouts.borrow_mut().get(spec).cloned();
        let (layout, metrics) = match cached {
            Some(hit) => hit,
            None => {
                let built = self.build(spec)?;
                self.layouts.borrow_mut().insert(spec.clone(), built.clone());
                built
            }
        };
        if let Some(brushes) = brushes {
            let mut start = 0u32;
            for (i, span) in spec.spans.iter().enumerate() {
                let len = u32::try_from(span.text.encode_utf16().count()).unwrap_or_default();
                if let Some(effect) = brushes(i) {
                    let range = DWRITE_TEXT_RANGE {
                        startPosition: start,
                        length: len,
                    };
                    // SAFETY: the range lies within the layout's text.
                    unsafe { layout.SetDrawingEffect(&effect, range) }.ctx("drawing effect")?;
                }
                start += len;
            }
        }
        Ok((layout, metrics))
    }

    fn build(&self, spec: &TextSpec) -> Result<(IDWriteTextLayout, TextMetrics)> {
        let family = self.family(spec.font);
        let base_weight = spec.spans.first().map_or(400, |s| s.weight);
        let format = self.format(&family, spec.size, base_weight)?;
        let text: Vec<u16> = spec.plain().encode_utf16().collect();
        let max_w = spec.max_width.unwrap_or(f32::MAX / 4.0);
        let fm = self.face_metrics(&family, base_weight);
        let line = spec.size * CSS_LINE_HEIGHT;
        let content = (fm.ascent + fm.descent) * spec.size;
        let baseline = (line - content) / 2.0 + fm.ascent * spec.size;
        // SAFETY: `text` and `format` outlive the calls; ranges are within the text.
        unsafe {
            let layout = self
                .factory
                .CreateTextLayout(&text, &format, max_w, line)
                .ctx("text layout")?;
            layout
                .SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, line, baseline)
                .ctx("line spacing")?;
            let typography = self.factory.CreateTypography().ctx("typography")?;
            typography
                .AddFontFeature(DWRITE_FONT_FEATURE {
                    nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES,
                    parameter: 1,
                })
                .ctx("tabular figures")?;
            let all = DWRITE_TEXT_RANGE {
                startPosition: 0,
                length: u32::try_from(text.len()).unwrap_or(u32::MAX),
            };
            layout.SetTypography(&typography, all).ctx("set typography")?;
            let mut start = 0u32;
            for span in &spec.spans {
                let len = u32::try_from(span.text.encode_utf16().count()).unwrap_or_default();
                let range = DWRITE_TEXT_RANGE {
                    startPosition: start,
                    length: len,
                };
                if span.weight != base_weight {
                    layout
                        .SetFontWeight(DWRITE_FONT_WEIGHT(i32::from(span.weight)), range)
                        .ctx("weight")?;
                }
                if span.underline {
                    layout.SetUnderline(true, range).ctx("underline")?;
                }
                if span.italic {
                    layout.SetFontStyle(DWRITE_FONT_STYLE_ITALIC, range).ctx("italic")?;
                }
                if span.spacing != 0.0 && len > 0 {
                    layout
                        .cast::<IDWriteTextLayout1>()
                        .ctx("text layout 1")?
                        .SetCharacterSpacing(0.0, span.spacing, 0.0, range)
                        .ctx("letter spacing")?;
                }
                if span.after > 0.0 && len > 0 {
                    let last = DWRITE_TEXT_RANGE {
                        startPosition: start + len - 1,
                        length: 1,
                    };
                    layout
                        .cast::<IDWriteTextLayout1>()
                        .ctx("text layout 1")?
                        .SetCharacterSpacing(0.0, span.spacing + span.after, 0.0, last)
                        .ctx("character spacing")?;
                }
                start += len;
            }
            let mut m = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut m).ctx("text metrics")?;
            let width = m.widthIncludingTrailingWhitespace.min(max_w);
            Ok((
                layout,
                TextMetrics {
                    width,
                    height: line,
                    baseline,
                },
            ))
        }
    }
}

impl TextMeasure for TextEngine {
    fn measure(&self, spec: &TextSpec) -> TextMetrics {
        match self.layout(spec, None) {
            Ok((_, m)) => m,
            Err(e) => {
                tracing::error!(error = %e, "text measure failed");
                TextMetrics {
                    width: 0.0,
                    height: spec.size * CSS_LINE_HEIGHT,
                    baseline: spec.size,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: a full cache never grows past its limit; recently used entries
    //! survive a flood while the oldest go first; re-inserting a present key evicts
    //! nothing; a limit of zero still keeps one entry.
    use super::*;
    use crate::node::{Role, Span};

    #[test]
    fn a_full_cache_drops_the_least_recently_used_first() {
        let mut cache = Recent::new(8);
        for k in 0..8 {
            cache.insert(k, k * 10);
        }
        // 0 and 1 are used again, so 2 and 3 are now the oldest.
        assert_eq!(cache.get(&0), Some(&0));
        assert_eq!(cache.get(&1), Some(&10));
        cache.insert(8, 80);
        assert!(cache.entries.len() <= 8);
        assert_eq!(cache.get(&0), Some(&0));
        assert_eq!(cache.get(&1), Some(&10));
        assert_eq!(cache.get(&2), None);
        assert_eq!(cache.get(&3), None);
        assert_eq!(cache.get(&8), Some(&80));
        // A key already present is replaced without evicting anything.
        let before = cache.entries.len();
        cache.insert(8, 81);
        assert_eq!((cache.entries.len(), cache.get(&8)), (before, Some(&81)));
    }

    #[test]
    fn a_hot_entry_outlives_a_flood_many_times_the_limit() {
        let mut cache = Recent::new(16);
        cache.insert(u32::MAX, ());
        for k in 0..1000 {
            cache.insert(k, ());
            assert!(cache.entries.len() <= 16);
            if k % 4 == 0 {
                assert!(cache.get(&u32::MAX).is_some(), "the hot entry went at {k}");
            }
        }
        let mut tiny = Recent::new(0);
        tiny.insert(1, ());
        tiny.insert(2, ());
        assert!(tiny.get(&1).is_none() && tiny.get(&2).is_some());
    }

    fn spec(text: &str) -> TextSpec {
        TextSpec {
            spans: vec![Span {
                text: text.into(),
                color: Role::Fg,
                weight: 400,
                after: 0.0,
                underline: false,
                italic: false,
                spacing: 0.0,
            }],
            font: FontRole::Ui,
            size: 13.0,
            max_width: None,
        }
    }

    /// Found in a code review (PLAN M19 P10): the cache used to empty completely at its
    /// limit, so a launcher search of hundreds of rows threw away every bar text.
    #[test]
    fn bar_texts_keep_their_layouts_through_a_launcher_sized_search() {
        let engine = TextEngine::new().expect("DirectWrite");
        let clock = spec("20:31");
        let (first, _) = engine.layout(&clock, None).unwrap();
        for row in 0..LAYOUT_CACHE_LIMIT * 2 {
            engine.layout(&spec(&format!("result {row}")), None).unwrap();
            // The bar redraws now and then while the rows come in.
            if row % 64 == 0 {
                engine.layout(&clock, None).unwrap();
            }
        }
        let (again, _) = engine.layout(&clock, None).unwrap();
        assert!(first == again, "the clock's layout was built again");
    }
}
