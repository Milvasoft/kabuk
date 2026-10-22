//! The bar's network indicator (DECISIONS D88): the link watched from Windows' own
//! notifications while the module is on, drawn when it changes.

use kabuk_win::msgloop::Window;

use super::{Shared, MSG_NETWORK};

impl Shared {
    /// Starts or stops watching the network with the module, and reads the link.
    pub(super) fn watch_network(&mut self) {
        let on = self.config().bar.modules.network;
        if !on {
            if self.network_watch.take().is_some() {
                tracing::info!("network no longer watched");
            }
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
        }
        self.read_network();
    }

    /// Reads the link again; true when it changed.
    fn read_network(&mut self) -> bool {
        let link = kabuk_win::network::link(self.network_watch.as_ref().and_then(|w| w.wlan()));
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
        if self.read_network() {
            self.render_all(false);
        }
        self.network_panel_notice(what);
    }
}
