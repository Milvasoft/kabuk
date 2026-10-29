//! Screen readers on the bar thread (DECISIONS D97): after each drawing, a bar, the
//! launcher or a popup hands its window the parts a screen reader names, from
//! [`crate::bar::access`] with what the bar knows of each module.

use kabuk_ui::geom::{Point, Scale};
use kabuk_ui::node::Layout;
use kabuk_win::uia::{Container, Part};

use super::view::{hit, BarData};
use super::Shared;
use crate::bar::access::{self, Said};

impl Shared {
    /// The parts of a bar showing `data`, drawn as `layout` at `origin`.
    pub(super) fn bar_parts(&self, layout: &Layout, origin: Point, scale: Scale, data: &BarData) -> Vec<Part> {
        access::bar_parts(layout, origin, scale, |d| {
            let said = self.module_said(d.hit.module, d.hit.value, data);
            access::bar_name(&self.i18n, (d.hit.module, d.hit.value), &d.text, said)
        })
    }

    /// Figure `which` of the bar's system figures, written as the bar writes it.
    fn figure(&self, which: u32, data: &BarData) -> Option<String> {
        let i18n = &self.i18n;
        match which {
            hit::STAT_CPU => data
                .cpu
                .map(|c| i18n.tr_args("bar-percent", &[("value", c.to_string().into())])),
            hit::STAT_RAM => data
                .ram_gb
                .map(|r| i18n.tr_args("bar-ram", &[("value", i18n.decimal(r, 2).into())])),
            hit::STAT_GPU => data
                .gpu_c
                .map(|g| i18n.tr_args("bar-temp", &[("value", g.to_string().into())])),
            hit::STAT_NET => data
                .net_mbs
                .map(|n| i18n.tr_args("bar-net-rate", &[("value", i18n.decimal(n, 2).into())])),
            _ => None,
        }
    }

    /// What the bar knows of module `module` (`value`) besides the words it draws.
    fn module_said(&self, module: &str, value: u32, data: &BarData) -> Said {
        let i = usize::try_from(value).unwrap_or(usize::MAX);
        let own = match module {
            hit::VOLUME => data.volume.as_ref().map(|v| {
                self.i18n
                    .tr_args("bar-percent", &[("value", v.percent.to_string().into())])
            }),
            hit::STATS => self.figure(value, data),
            hit::APP => self.apps.list.get(i).map(|a| self.app_name(a)),
            hit::TRAY => self.tray.visible().nth(i).map(|icon| icon.tip.clone()),
            hit::LAYOUT => Some(
                self.i18n
                    .tr(if data.floating_layout { "t-float-on" } else { "t-tiled" }),
            ),
            // The language's whole name once its panel listed them, else its short code
            // (the tooltip asks Windows for every language: not on each drawing).
            hit::KEYBOARD => {
                let now = kabuk_win::input_lang::current();
                self.input_langs
                    .as_ref()
                    .and_then(|all| all.iter().find(|l| Some(l.hkl) == now))
                    .map(|l| l.name.clone())
                    .or_else(|| data.keyboard.clone())
            }
            _ => None,
        };
        // A figure's tooltip repeats its label; the keyboard's asks Windows for every
        // language (not on each drawing).
        let tip = if module == hit::KEYBOARD || module == hit::STATS {
            None
        } else {
            self.module_tip(module, value)
        };
        Said { tip, own }
    }
}

/// Tells screen readers what popup `shown` shows: a `container` named `name` holding the
/// regions of `module` of `layout` (drawn at `scale`) as `role`s, each named by `name_of`
/// (its value and its words; `None` leaves it out), the one valued `selected` holding
/// the keyboard.
pub(super) fn tell_popup(
    shown: &super::motion_host::Shown,
    (container, name): (Container, &str),
    (layout, scale): (&Layout, Scale),
    (module, role): (&str, kabuk_win::uia::Role),
    selected: Option<u32>,
    name_of: impl Fn(u32, &str) -> Option<String>,
) {
    let parts = access::popup_parts(layout, scale, (module, role), selected, name_of);
    shown.window().set_accessible(container, name, parts);
}

/// [`tell_popup`] for a menu named `name`: its entries of `module` are menu items named
/// by their words.
pub(super) fn tell_menu(
    shown: &super::motion_host::Shown,
    name: &str,
    drawn: (&Layout, Scale),
    module: &str,
    selected: Option<u32>,
) {
    tell_popup(
        shown,
        (Container::Menu, name),
        drawn,
        (module, kabuk_win::uia::Role::MenuItem),
        selected,
        access::words,
    );
}

/// [`tell_popup`] for a panel named `name`: its entries of `module` are buttons named by
/// `name_of`.
pub(super) fn tell_panel(
    shown: &super::motion_host::Shown,
    name: &str,
    drawn: (&Layout, Scale),
    module: &str,
    selected: Option<u32>,
    name_of: impl Fn(u32, &str) -> Option<String>,
) {
    tell_popup(
        shown,
        (Container::Panel, name),
        drawn,
        (module, kabuk_win::uia::Role::Button),
        selected,
        name_of,
    );
}
