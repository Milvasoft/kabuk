//! The bar's network indicator (DECISIONS D88): the link watched from Windows' own
//! notifications while the module is on, drawn when it changes.

use kabuk_win::msgloop::Window;

use super::{Shared, MSG_ETHERNET, MSG_NETWORK};

impl Shared {
    /// Starts or stops watching the network with the module, and reads the link.
    pub(super) fn watch_network(&mut self) {
        let on = self.config().bar.modules.network;
        if !on {
            if self.network_watch.take().is_some() {
                tracing::info!("network no longer watched");
            }
            self.ethernet_watch = None;
            self.live.network = None;
            self.close_network_panel();
            return;
        }
        if self.network_watch.is_none() {
            let Some(control) = self.control.as_ref().map(Window::hwnd) else {
                return;
            };
            let watch = kabuk_win::network::Watch::start(control, MSG_NETWORK);
            tracing::info!(wifi = watch.wlan().is_some(), "network watched");
            self.network_watch = Some(watch);
            self.ethernet_watch = kabuk_win::network::EthernetWatch::start(control, MSG_ETHERNET)
                .map_err(|e| tracing::warn!(error = %e, "wired link not read"))
                .ok();
        }
        if let Some(e) = &self.ethernet_watch {
            e.ask();
        }
        self.read_network();
    }

    /// Reads the link again with the wired link as last read (off this thread, M19 P3);
    /// true when it changed. Nothing until the wired link has been read once.
    fn read_network(&mut self) -> bool {
        let Some(ethernet) = self.ethernet_watch.as_ref().and_then(|e| e.up()) else {
            return false;
        };
        let wlan = self.network_watch.as_ref().and_then(|w| w.wlan());
        let link = kabuk_win::network::link_with(ethernet, wlan);
        if self.live.network.as_ref() == Some(&link) {
            return false;
        }
        tracing::info!(?link, "network link");
        self.live.network = Some(link);
        true
    }

    /// Windows told of a change (`what`: [`kabuk_win::network::CHANGED`] or
    /// [`kabuk_win::network::SCANNED`]).
    pub(super) fn network_changed(&mut self, what: usize) {
        if self.network_watch.is_none() {
            return;
        }
        tracing::trace!(what, "network notice");
        if let Some(e) = &self.ethernet_watch {
            e.ask();
        }
        if self.read_network() {
            self.render_all(false);
        }
        self.network_panel_notice(what);
    }

    /// The wired link has been read: the link drawn again if it changed, and the open
    /// network panel with it.
    pub(super) fn ethernet_read(&mut self) {
        if self.read_network() {
            self.render_all(false);
        }
        self.network_panel_notice(kabuk_win::network::CHANGED);
    }
}
