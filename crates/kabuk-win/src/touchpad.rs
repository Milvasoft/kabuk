//! Precision touchpads through raw input (DECISIONS D68): a window gets every touchpad
//! report as `WM_INPUT`, in the background too, and the HID parser reads its fingers
//! (`kabuk_core::gesture` makes swipes of them). Nothing is hooked or injected; the
//! touchpad keeps working for Windows and every program as before.

use std::collections::HashMap;

use kabuk_core::gesture::{Contact, Reported};
use windows::Win32::Devices::HumanInterfaceDevice::{
    HidP_GetCaps, HidP_GetUsageValue, HidP_GetUsages, HidP_GetValueCaps, HidP_Input, HIDP_CAPS, HIDP_VALUE_CAPS,
    PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE,
    RAWINPUTHEADER, RIDEV_INPUTSINK, RIDEV_REMOVE, RIDI_PREPARSEDDATA, RID_INPUT, RIM_TYPEHID,
};

use crate::sync::io_error;
use crate::window::Hwnd;

/// HID usages: the digitizer page and its touch pad, contact count, contact id and tip
/// switch; the generic desktop page's X and Y.
const DIGITIZER: u16 = 0x0D;
const TOUCH_PAD: u16 = 0x05;
const CONTACT_COUNT: u16 = 0x54;
const CONTACT_ID: u16 = 0x51;
const TIP_SWITCH: u16 = 0x42;
const GENERIC: u16 = 0x01;
const X: u16 = 0x30;
const Y: u16 = 0x31;
/// `HIDP_STATUS_SUCCESS`.
const HID_OK: i32 = 0x0011_0000;
/// Most buttons a finger's collection reports (the tip switch, confidence, …).
const BUTTONS: usize = 16;

/// `window` gets `WM_INPUT` for every precision touchpad report, even in the background
/// (`true`), or no more (`false`). Without a touchpad nothing comes.
pub fn watch(window: Hwnd, on: bool) -> std::io::Result<()> {
    let device = RAWINPUTDEVICE {
        usUsagePage: DIGITIZER,
        usUsage: TOUCH_PAD,
        dwFlags: if on { RIDEV_INPUTSINK } else { RIDEV_REMOVE },
        hwndTarget: if on { window.raw() } else { HWND::default() },
    };
    // SAFETY: a valid array of one, its element size passed.
    unsafe {
        RegisterRawInputDevices(
            &[device],
            u32::try_from(std::mem::size_of::<RAWINPUTDEVICE>()).unwrap_or_default(),
        )
    }
    .map_err(io_error)
}

/// A value's logical range in a report (smallest, largest).
type Range = (i32, i32);

/// A finger's place in a touchpad's reports: its link collection and the logical range
/// of its X and Y.
#[derive(Clone, Copy, Debug)]
struct Finger {
    link: u16,
    x: Range,
    y: Range,
}

/// A touchpad's HID parser data and its fingers, read once.
struct Device {
    /// `HIDP_PREPARSED_DATA`, kept 8-byte aligned.
    preparsed: Vec<u64>,
    fingers: Vec<Finger>,
}

impl Device {
    fn parser(&self) -> PHIDP_PREPARSED_DATA {
        PHIDP_PREPARSED_DATA(self.preparsed.as_ptr() as isize)
    }
}

/// Reads touchpad reports; keeps each device's parser data.
#[derive(Default)]
pub struct Reader {
    devices: HashMap<isize, Option<Device>>,
}

impl Reader {
    /// The touchpad reports of one `WM_INPUT` (its `lparam`): each report's contact count
    /// (0 when it continues a frame) and its fingers. Empty for other input.
    pub fn read(&mut self, lparam: isize) -> Vec<(u32, Vec<Reported>)> {
        let Some(mut buffer) = raw_input(lparam) else {
            return Vec::new();
        };
        let (kind, handle, size, count) = {
            // SAFETY: the buffer holds a whole RAWINPUT as `GetRawInputData` wrote it, and
            // is 8-byte aligned (a Vec<u64>); the reference ends in this block.
            let raw = unsafe { &*(buffer.as_ptr() as *const RAWINPUT) };
            // SAFETY: the union's HID member is only used for RIM_TYPEHID (checked below).
            let hid = unsafe { raw.data.hid };
            (
                raw.header.dwType,
                raw.header.hDevice,
                hid.dwSizeHid as usize,
                hid.dwCount as usize,
            )
        };
        if kind != RIM_TYPEHID.0 {
            return Vec::new();
        }
        let device = self.devices.entry(handle.0 as isize).or_insert_with(|| device(handle));
        let Some(device) = device.as_ref() else {
            return Vec::new();
        };
        // `RAWHID::bRawData` follows its two counts.
        let start = std::mem::offset_of!(RAWINPUT, data) + std::mem::size_of::<u32>() * 2;
        let bytes_len = buffer.len() * std::mem::size_of::<u64>();
        // SAFETY: a byte view of the whole buffer, the only reference to it from here on.
        let bytes = unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr().cast::<u8>(), bytes_len) };
        let mut out = Vec::new();
        for i in 0..count {
            let from = start + i * size;
            let Some(report) = bytes.get_mut(from..from + size) else {
                break;
            };
            out.push(fingers(device, report));
        }
        out
    }
}

/// The `RAWINPUT` behind a `WM_INPUT`'s `lparam`, in an aligned buffer.
fn raw_input(lparam: isize) -> Option<Vec<u64>> {
    let handle = HRAWINPUT(lparam as *mut core::ffi::c_void);
    let header = u32::try_from(std::mem::size_of::<RAWINPUTHEADER>()).ok()?;
    let mut size = 0u32;
    // SAFETY: a size query with no buffer.
    unsafe { GetRawInputData(handle, RID_INPUT, None, &mut size, header) };
    if size == 0 {
        return None;
    }
    let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
    // SAFETY: the buffer is at least `size` bytes.
    let got = unsafe { GetRawInputData(handle, RID_INPUT, Some(buffer.as_mut_ptr().cast()), &mut size, header) };
    (got != u32::MAX && got > 0).then_some(buffer)
}

/// A touchpad's parser data and its fingers' collections; `None` for a device that is
/// not a touchpad or cannot be read.
fn device(handle: HANDLE) -> Option<Device> {
    let mut size = 0u32;
    // SAFETY: a size query with no buffer.
    unsafe { GetRawInputDeviceInfoW(Some(handle), RIDI_PREPARSEDDATA, None, &mut size) };
    if size == 0 {
        return None;
    }
    let mut preparsed = vec![0u64; (size as usize).div_ceil(8)];
    // SAFETY: the buffer is at least `size` bytes.
    let got = unsafe {
        GetRawInputDeviceInfoW(
            Some(handle),
            RIDI_PREPARSEDDATA,
            Some(preparsed.as_mut_ptr().cast()),
            &mut size,
        )
    };
    if got == u32::MAX || got == 0 {
        return None;
    }
    let mut device = Device {
        preparsed,
        fingers: Vec::new(),
    };
    let mut caps = HIDP_CAPS::default();
    // SAFETY: the parser data read above; `caps` is written.
    if unsafe { HidP_GetCaps(device.parser(), &mut caps) }.0 != HID_OK {
        return None;
    }
    let mut values = vec![HIDP_VALUE_CAPS::default(); usize::from(caps.NumberInputValueCaps)];
    let mut len = caps.NumberInputValueCaps;
    // SAFETY: room for `len` value caps.
    if unsafe { HidP_GetValueCaps(HidP_Input, values.as_mut_ptr(), &mut len, device.parser()) }.0 != HID_OK {
        return None;
    }
    values.truncate(usize::from(len));
    let range = |v: &HIDP_VALUE_CAPS| (v.LogicalMin, v.LogicalMax);
    let mut axes: HashMap<u16, (Option<Range>, Option<Range>)> = HashMap::new();
    for v in &values {
        if v.UsagePage != GENERIC || v.IsRange {
            continue;
        }
        // SAFETY: `NotRange` is the member in use when `IsRange` is false.
        let usage = unsafe { v.Anonymous.NotRange.Usage };
        let e = axes.entry(v.LinkCollection).or_default();
        match usage {
            X => e.0 = Some(range(v)),
            Y => e.1 = Some(range(v)),
            _ => {}
        }
    }
    let mut fingers: Vec<Finger> = axes
        .into_iter()
        .filter_map(|(link, (x, y))| Some(Finger { link, x: x?, y: y? }))
        .collect();
    fingers.sort_by_key(|f| f.link);
    device.fingers = fingers;
    (!device.fingers.is_empty()).then_some(device)
}

/// One report's contact count and fingers, their places 0-1 across the pad.
fn fingers(device: &Device, report: &mut [u8]) -> (u32, Vec<Reported>) {
    let parser = device.parser();
    let value = |page: u16, link: u16, usage: u16, report: &mut [u8]| {
        let mut v = 0u32;
        // SAFETY: the parser data and a report of its device.
        let status = unsafe { HidP_GetUsageValue(HidP_Input, page, Some(link), usage, &mut v, parser, report) };
        (status.0 == HID_OK).then_some(v)
    };
    let count = value(DIGITIZER, 0, CONTACT_COUNT, report).unwrap_or(0);
    let share = |v: u32, (min, max): Range| {
        let span = (max - min).max(1) as f32;
        ((v as f32 - min as f32) / span).clamp(0.0, 1.0)
    };
    let mut out = Vec::new();
    for f in &device.fingers {
        let (Some(x), Some(y)) = (value(GENERIC, f.link, X, report), value(GENERIC, f.link, Y, report)) else {
            continue;
        };
        let id = value(DIGITIZER, f.link, CONTACT_ID, report).unwrap_or(u32::from(f.link));
        let mut usages = [0u16; BUTTONS];
        let mut len = BUTTONS as u32;
        // SAFETY: room for `len` usages; the parser data and a report of its device.
        let status = unsafe {
            HidP_GetUsages(
                HidP_Input,
                DIGITIZER,
                Some(f.link),
                usages.as_mut_ptr(),
                &mut len,
                parser,
                report,
            )
        };
        let touching = status.0 == HID_OK && usages[..(len as usize).min(BUTTONS)].contains(&TIP_SWITCH);
        out.push(Reported {
            contact: Contact {
                id,
                x: share(x, f.x),
                y: share(y, f.y),
            },
            touching,
        });
    }
    (count, out)
}

#[cfg(test)]
mod tests {
    //! Without a touchpad (this machine and the VM have none, DECISIONS D68) only the
    //! watch can be tried: it registers and goes away; input of no touchpad reads as
    //! nothing.
    use super::*;
    use crate::msgloop::MessageWindow;

    #[test]
    fn the_watch_registers_and_goes_away() {
        let w = MessageWindow::new("Kabuk.Test.Touchpad", "touchpad", |_| None).unwrap();
        watch(w.hwnd(), true).unwrap();
        watch(w.hwnd(), false).unwrap();
    }

    #[test]
    fn a_handle_of_no_input_reads_as_nothing() {
        assert!(Reader::default().read(0).is_empty());
        assert!(Reader::default().read(0x1234).is_empty());
    }
}
