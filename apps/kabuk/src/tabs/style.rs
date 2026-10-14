//! What a tabbed group's strip looks like. The design page has no tab strip (DECISIONS
//! D34): it takes the tray tooltip's sizes (D21) and the look's surface colours, and marks
//! the shown tab with the accent, as the looks mark state (SPEC §5). Pure.

use kabuk_ui::node::Role;

use crate::bar::tooltip::TIP;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabStyle {
    /// The strip's height and its tabs' text size (DIPs).
    pub height: f32,
    pub text: f32,
    /// Space inside a tab, and between tabs.
    pub pad_h: f32,
    pub gap: f32,
    pub radius: f32,
    pub border: f32,
    pub tab: Role,
    pub tab_line: Role,
    pub tab_text: Role,
    /// The shown tab: its underline and text.
    pub shown_mark: Role,
    pub shown_text: Role,
    /// The shown tab's underline thickness.
    pub mark: f32,
}

/// The strip in every look: one tooltip line tall (its text at the CSS line height plus
/// its padding, 12.5 × 1.55 + 2 × 6 ≈ 31.4, rounded up).
pub const TABS: TabStyle = TabStyle {
    height: 32.0,
    text: TIP.text,
    pad_h: TIP.pad_h,
    gap: TIP.border * 2.0,
    radius: TIP.radius,
    border: TIP.border,
    tab: Role::Surface,
    tab_line: Role::SurfaceLine,
    tab_text: Role::Dim,
    shown_mark: Role::Accent,
    shown_text: Role::Fg,
    mark: TIP.border * 2.0,
};

#[cfg(test)]
mod tests {
    //! Edge case: the strip holds a line of its text with the tooltip's padding.
    use super::*;

    #[test]
    fn a_line_of_text_fits_the_strip() {
        let line = TABS.text * kabuk_ui::render::CSS_LINE_HEIGHT;
        assert!(line + 2.0 * TIP.pad_v <= TABS.height, "{line}");
        assert!(TABS.height < 2.0 * line, "taller than a line needs");
    }
}
