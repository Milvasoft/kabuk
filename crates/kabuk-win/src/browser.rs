//! What a window of another program is showing, read from outside it: a browser's
//! address through UI Automation (as a screen reader reads it), a File Explorer window's
//! folder through Windows' list of shell windows. Nothing is loaded into the program.

use std::io;

use windows::core::{Interface, BSTR};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, IUIAutomation2, IUIAutomationElement, IUIAutomationTreeWalker, IUIAutomationValuePattern,
    UIA_EditControlTypeId, UIA_ValuePatternId,
};
use windows::Win32::UI::Shell::{IShellWindows, IWebBrowserApp, ShellWindows};

use crate::window::Hwnd;

/// How long a program may take to answer: a hung browser must not hold the saving up.
const CONNECT_MS: u32 = 1_000;
const ANSWER_MS: u32 = 3_000;
/// How many parts of a window are looked at for its address bar, which comes early.
const NODES: u32 = 300;

/// What the address bar of browser window `window` shows, as it shows it (`localhost:6179`,
/// without a scheme, is usual). The first edit field of a browser's window is its address
/// bar: the toolbar comes before the page. `None` for a window without one (a web app's).
pub fn address_bar(window: Hwnd) -> Option<String> {
    let found = first_edit(window).map_err(|e| tracing::debug!(error = %e, "address bar not read"));
    let element = found.ok().flatten()?;
    // SAFETY: plain UI Automation client calls on an element just found.
    unsafe {
        let value: IUIAutomationValuePattern = element.GetCurrentPatternAs(UIA_ValuePatternId).ok()?;
        let text: BSTR = value.CurrentValue().ok()?;
        Some(text.to_string())
    }
}

fn first_edit(window: Hwnd) -> io::Result<Option<IUIAutomationElement>> {
    crate::system::com_init_mta();
    // SAFETY: plain creation of the UI Automation client object, the kind with time limits.
    let uia: IUIAutomation2 =
        unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }.map_err(io::Error::other)?;
    // SAFETY: plain UI Automation client calls; the window may be of any process.
    unsafe {
        uia.SetConnectionTimeout(CONNECT_MS).map_err(io::Error::other)?;
        uia.SetTransactionTimeout(ANSWER_MS).map_err(io::Error::other)?;
        let root = uia.ElementFromHandle(window.raw()).map_err(io::Error::other)?;
        // Walked in the raw view: a search of the window does not reach Chrome's address bar.
        let walker = uia.RawViewWalker().map_err(io::Error::other)?;
        let mut left = NODES;
        Ok(find_edit(&walker, &root, &mut left))
    }
}

/// The first edit field under `at`, depth first, looking at no more than `left` parts.
fn find_edit(
    walker: &IUIAutomationTreeWalker,
    at: &IUIAutomationElement,
    left: &mut u32,
) -> Option<IUIAutomationElement> {
    // SAFETY: plain UI Automation client calls on elements just found.
    unsafe {
        let mut child = walker.GetFirstChildElement(at).ok();
        while let Some(c) = child {
            *left = left.checked_sub(1)?;
            if c.CurrentControlType().is_ok_and(|t| t == UIA_EditControlTypeId) {
                return Some(c);
            }
            if let Some(found) = find_edit(walker, &c, left) {
                return Some(found);
            }
            if *left == 0 {
                return None;
            }
            child = walker.GetNextSiblingElement(&c).ok();
        }
    }
    None
}

/// The folder File Explorer window `window` shows, as a `file:` address; `None` for one
/// showing a place that is no folder (Home, This PC).
pub fn explorer_folder(window: Hwnd) -> Option<String> {
    crate::system::com_init_mta();
    // SAFETY: plain COM calls on Windows' own list of shell windows, each object used
    // only while it is held.
    unsafe {
        let shell: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER).ok()?;
        for i in 0..shell.Count().ok()? {
            let Ok(item) = shell.Item(&VARIANT::from(i)) else {
                continue;
            };
            let Ok(app) = item.cast::<IWebBrowserApp>() else {
                continue;
            };
            if app.HWND().ok().map(|h| h.0 as u64) != Some(window.0) {
                continue;
            }
            let url = app.LocationURL().ok()?.to_string();
            return (!url.is_empty()).then_some(url);
        }
    }
    None
}
