//! Screen readers (DECISIONS D97): Kabuk draws everything itself, so Narrator saw
//! nothing of it. A window given parts ([`crate::msgloop::Window::set_accessible`]) answers
//! UI Automation's `WM_GETOBJECT` with a small provider: the window is the root fragment
//! and each part (a button, a list item, a menu item) its child, with a name, a control
//! type, a place on the screen and the Invoke pattern, which makes the window the same
//! click the mouse would make on the part (where it is when the window takes it). A part
//! that is a level (a slider: the volume, a brightness) has the RangeValue pattern
//! instead: its value, ends and step, and SetValue, which asks the window to set it
//! ([`set_value_message`]), as dragging it would. A notice is said aloud ([`announce`]).
//!
//! Nothing runs unless a client asks: the owner hands over its parts after each drawing
//! (a list swapped under a lock), and the provider reads them when a client calls. The
//! provider's objects hold only that list and the window's handle, so whichever thread
//! UI Automation calls them on, they never reach the owner's state.
//!
//! [`read`], [`invoke`], [`range`], [`set_range`] and [`listen`] are the client side, as
//! a screen reader sees a window (tests).

use std::io;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use windows::core::{implement, IUnknownImpl, Interface, BSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::SAFEARRAY;
use windows::Win32::System::Ole::{SafeArrayCreateVector, SafeArrayPutElement};
use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_BOOL, VT_I4};
use windows::Win32::UI::Accessibility::*;

use crate::msgloop::{msg, post_message, Msg, PxRect};
use crate::window::Hwnd;

/// What a part is to a screen reader (its UI Automation control type).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// A bar's module, app or tray icon; a panel's entry.
    Button,
    /// A launcher result.
    ListItem,
    /// A menu's entry.
    MenuItem,
    /// A level (the volume, a brightness), with its [`Range`].
    Slider,
    /// Words to read, not to click (a notice).
    Text,
}

impl Role {
    const ALL: [Role; 5] = [Role::Button, Role::ListItem, Role::MenuItem, Role::Slider, Role::Text];

    fn control_type(self) -> UIA_CONTROLTYPE_ID {
        match self {
            Role::Button => UIA_ButtonControlTypeId,
            Role::ListItem => UIA_ListItemControlTypeId,
            Role::MenuItem => UIA_MenuItemControlTypeId,
            Role::Slider => UIA_SliderControlTypeId,
            Role::Text => UIA_TextControlTypeId,
        }
    }

    fn of(control: UIA_CONTROLTYPE_ID) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.control_type() == control)
    }
}

/// A level a part sets, as UI Automation's RangeValue pattern offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub value: i32,
    pub min: i32,
    pub max: i32,
    /// What one key moves it (a wheel notch's step). Kabuk's popups take no Page keys,
    /// so a large change is this one too.
    pub step: i32,
}

/// What the window itself is to a screen reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    /// A bar: a tool bar of buttons.
    Bar,
    /// A menu of menu items.
    Menu,
    /// A list of results (the launcher).
    List,
    /// A panel of buttons.
    Panel,
}

impl Container {
    fn control_type(self) -> UIA_CONTROLTYPE_ID {
        match self {
            Container::Bar => UIA_ToolBarControlTypeId,
            Container::Menu => UIA_MenuControlTypeId,
            Container::List => UIA_ListControlTypeId,
            Container::Panel => UIA_PaneControlTypeId,
        }
    }
}

/// One part of a window for screen readers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// Who it is across redraws (its kind and value, [`part_id`]): a reader's object for
    /// it stays it when the parts before it come or go.
    pub id: u64,
    /// What a screen reader says, in the user's language.
    pub name: String,
    /// Where an Invoke clicks it (client pixels): a point a click reaches it at, which
    /// is not its middle when another part lies over that.
    pub click: (i32, i32),
    pub role: Role,
    /// Where it is in the window's client area, in physical pixels.
    pub rect: PxRect,
    /// The keyboard is on it (a menu's or the launcher's selection).
    pub focused: bool,
    /// A level's value and ends: offered as a range, not to be invoked (a click in its
    /// middle would set it to half).
    pub range: Option<Range>,
}

/// What the provider's objects read: the window (none once it is gone), its name and type,
/// and its parts as last drawn.
struct Inner {
    hwnd: Option<Hwnd>,
    container: Container,
    name: String,
    parts: Vec<Part>,
}

type Shared = Arc<Mutex<Inner>>;

fn lock(shared: &Shared) -> MutexGuard<'_, Inner> {
    // A panic while it was held left a whole list; it is still good to read.
    shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn gone() -> windows::core::Error {
    windows::core::Error::from_hresult(windows::core::HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32))
}

/// "Nothing" for an out parameter: success with a null pointer.
fn nothing<T>() -> windows::core::Result<T> {
    Err(windows::core::Error::empty())
}

fn int_variant(v: i32) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_I4,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { lVal: v },
            }),
        },
    }
}

fn bool_variant(v: bool) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_BOOL,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 {
                    boolVal: windows::Win32::Foundation::VARIANT_BOOL::from(v),
                },
            }),
        },
    }
}

/// The screen place of the window's client origin.
fn client_origin(hwnd: Hwnd) -> Option<(i32, i32)> {
    let mut p = POINT::default();
    // SAFETY: `p` is a valid in/out point; a destroyed window makes the call fail.
    unsafe { ClientToScreen(hwnd.raw(), &mut p) }
        .as_bool()
        .then_some((p.x, p.y))
}

/// The provider of a window: its root fragment.
#[implement(
    IRawElementProviderSimple,
    IRawElementProviderFragment,
    IRawElementProviderFragmentRoot
)]
struct Root {
    shared: Shared,
}

/// A part's id from its kind (e.g. the module it is) and its value.
pub fn part_id(kind: &str, value: u32) -> u64 {
    // FNV-1a: stable across runs, unlike the standard hasher.
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0100_0000_01b3;
    kind.bytes()
        .chain(value.to_le_bytes())
        .fold(OFFSET, |h, b| (h ^ u64::from(b)).wrapping_mul(PRIME))
}

/// A part of a window, found by its id (found by a test: by its place in the list, a
/// reader's object pointed at another part once the figures came after the first bars,
/// and invoking the clock clicked a figure).
#[implement(
    IRawElementProviderSimple,
    IRawElementProviderFragment,
    IInvokeProvider,
    IRangeValueProvider
)]
struct Item {
    shared: Shared,
    root: IRawElementProviderFragmentRoot,
    id: u64,
}

fn item(shared: &Shared, root: &IRawElementProviderFragmentRoot, index: usize) -> IRawElementProviderFragment {
    let id = lock(shared).parts.get(index).map_or(0, |p| p.id);
    Item {
        shared: shared.clone(),
        root: root.clone(),
        id,
    }
    .into()
}

impl IRawElementProviderSimple_Impl for Root_Impl {
    fn ProviderOptions(&self) -> windows::core::Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider)
    }

    fn GetPatternProvider(&self, _: UIA_PATTERN_ID) -> windows::core::Result<windows::core::IUnknown> {
        nothing()
    }

    fn GetPropertyValue(&self, id: UIA_PROPERTY_ID) -> windows::core::Result<VARIANT> {
        let inner = lock(&self.shared);
        Ok(if id == UIA_NamePropertyId && !inner.name.is_empty() {
            VARIANT::from(inner.name.as_str())
        } else if id == UIA_ControlTypePropertyId {
            int_variant(inner.container.control_type().0)
        } else {
            VARIANT::default()
        })
    }

    fn HostRawElementProvider(&self) -> windows::core::Result<IRawElementProviderSimple> {
        let hwnd = lock(&self.shared).hwnd.ok_or_else(gone)?;
        // SAFETY: plain call for a window of this process.
        unsafe { UiaHostProviderFromHwnd(hwnd.raw()) }
    }
}

impl IRawElementProviderFragment_Impl for Root_Impl {
    fn Navigate(&self, direction: NavigateDirection) -> windows::core::Result<IRawElementProviderFragment> {
        let count = lock(&self.shared).parts.len();
        let index = if count == 0 {
            None
        } else if direction == NavigateDirection_FirstChild {
            Some(0)
        } else if direction == NavigateDirection_LastChild {
            Some(count - 1)
        } else {
            // Its parent and siblings are the window's, which UI Automation knows.
            None
        };
        match index {
            Some(i) => Ok(item(&self.shared, &self.to_interface(), i)),
            None => nothing(),
        }
    }

    fn GetRuntimeId(&self) -> windows::core::Result<*mut SAFEARRAY> {
        // The window's own, from its host provider.
        Ok(std::ptr::null_mut())
    }

    fn BoundingRectangle(&self) -> windows::core::Result<UiaRect> {
        // The window's own, from its host provider.
        Ok(UiaRect::default())
    }

    fn GetEmbeddedFragmentRoots(&self) -> windows::core::Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn SetFocus(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn FragmentRoot(&self) -> windows::core::Result<IRawElementProviderFragmentRoot> {
        Ok(self.to_interface())
    }
}

impl IRawElementProviderFragmentRoot_Impl for Root_Impl {
    fn ElementProviderFromPoint(&self, x: f64, y: f64) -> windows::core::Result<IRawElementProviderFragment> {
        let inner = lock(&self.shared);
        let hwnd = inner.hwnd.ok_or_else(gone)?;
        let (ox, oy) = client_origin(hwnd).ok_or_else(gone)?;
        let (px, py) = (x.floor() as i32 - ox, y.floor() as i32 - oy);
        let found = inner.parts.iter().rposition(|p| {
            (p.rect.x..p.rect.x + p.rect.w).contains(&px) && (p.rect.y..p.rect.y + p.rect.h).contains(&py)
        });
        drop(inner);
        Ok(match found {
            Some(i) => item(&self.shared, &self.to_interface(), i),
            None => self.to_interface(),
        })
    }

    fn GetFocus(&self) -> windows::core::Result<IRawElementProviderFragment> {
        let focused = lock(&self.shared).parts.iter().position(|p| p.focused);
        match focused {
            Some(i) => Ok(item(&self.shared, &self.to_interface(), i)),
            None => nothing(),
        }
    }
}

impl Item_Impl {
    /// The part as it is now; an error once it is gone (the window closed, the list
    /// became shorter).
    fn part(&self) -> windows::core::Result<(Hwnd, Part)> {
        let inner = lock(&self.shared);
        let hwnd = inner.hwnd.ok_or_else(gone)?;
        let part = inner.parts.iter().find(|p| p.id == self.id).cloned().ok_or_else(gone)?;
        Ok((hwnd, part))
    }

    /// Its place among the window's parts now.
    fn index(&self) -> Option<usize> {
        lock(&self.shared).parts.iter().position(|p| p.id == self.id)
    }
}

impl IRawElementProviderSimple_Impl for Item_Impl {
    fn ProviderOptions(&self) -> windows::core::Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider)
    }

    fn GetPatternProvider(&self, id: UIA_PATTERN_ID) -> windows::core::Result<windows::core::IUnknown> {
        let (_, part) = self.part()?;
        // A level is set, not invoked; words to read are neither.
        if id == UIA_RangeValuePatternId && part.range.is_some() {
            let range: IRangeValueProvider = self.to_interface();
            return Ok(range.into());
        }
        if id == UIA_InvokePatternId && part.range.is_none() && part.role != Role::Text {
            let invoke: IInvokeProvider = self.to_interface();
            return Ok(invoke.into());
        }
        nothing()
    }

    fn GetPropertyValue(&self, id: UIA_PROPERTY_ID) -> windows::core::Result<VARIANT> {
        let (_, part) = self.part()?;
        Ok(if id == UIA_NamePropertyId {
            VARIANT::from(part.name.as_str())
        } else if id == UIA_ControlTypePropertyId {
            int_variant(part.role.control_type().0)
        } else if id == UIA_HasKeyboardFocusPropertyId || id == UIA_IsKeyboardFocusablePropertyId {
            bool_variant(part.focused)
        } else if id == UIA_IsEnabledPropertyId {
            bool_variant(true)
        } else {
            VARIANT::default()
        })
    }

    fn HostRawElementProvider(&self) -> windows::core::Result<IRawElementProviderSimple> {
        nothing()
    }
}

impl IRawElementProviderFragment_Impl for Item_Impl {
    fn Navigate(&self, direction: NavigateDirection) -> windows::core::Result<IRawElementProviderFragment> {
        if direction == NavigateDirection_Parent {
            return self.root.cast();
        }
        let count = lock(&self.shared).parts.len();
        let Some(at) = self.index() else { return nothing() };
        let index = if direction == NavigateDirection_NextSibling {
            Some(at + 1).filter(|i| *i < count)
        } else if direction == NavigateDirection_PreviousSibling {
            at.checked_sub(1).filter(|i| *i < count)
        } else {
            None
        };
        match index {
            Some(i) => Ok(item(&self.shared, &self.root, i)),
            None => nothing(),
        }
    }

    fn GetRuntimeId(&self) -> windows::core::Result<*mut SAFEARRAY> {
        let id = [
            UiaAppendRuntimeId as i32,
            // The id's low bits: stable while the part is there.
            (self.id & 0x7fff_ffff) as i32,
        ];
        // SAFETY: a new one-dimensional array of 32-bit integers, filled within its
        // bounds; UI Automation takes it over.
        unsafe {
            let array = SafeArrayCreateVector(VT_I4, 0, id.len() as u32);
            if array.is_null() {
                return Err(windows::core::Error::from_hresult(
                    windows::Win32::Foundation::E_OUTOFMEMORY,
                ));
            }
            for (i, v) in id.iter().enumerate() {
                let at = i as i32;
                SafeArrayPutElement(array, &at, (v as *const i32).cast())?;
            }
            Ok(array)
        }
    }

    fn BoundingRectangle(&self) -> windows::core::Result<UiaRect> {
        let (hwnd, part) = self.part()?;
        let (ox, oy) = client_origin(hwnd).ok_or_else(gone)?;
        Ok(UiaRect {
            left: f64::from(ox + part.rect.x),
            top: f64::from(oy + part.rect.y),
            width: f64::from(part.rect.w),
            height: f64::from(part.rect.h),
        })
    }

    fn GetEmbeddedFragmentRoots(&self) -> windows::core::Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn SetFocus(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn FragmentRoot(&self) -> windows::core::Result<IRawElementProviderFragmentRoot> {
        Ok(self.root.clone())
    }
}

impl IInvokeProvider_Impl for Item_Impl {
    /// The part's id, posted (Invoke must not wait on what the click does); the window
    /// clicks it where it is when it takes the message ([`clicks_of`]).
    fn Invoke(&self) -> windows::core::Result<()> {
        let (hwnd, part) = self.part()?;
        let invoke = Msg {
            id: invoke_message(),
            wparam: part.id as usize,
            lparam: 0,
        };
        post_message(hwnd, invoke).map_err(|_| gone())
    }
}

impl Item_Impl {
    /// The part's range now; an error for a part that is no level.
    fn range(&self) -> windows::core::Result<Range> {
        let (_, part) = self.part()?;
        part.range
            .ok_or_else(|| windows::core::Error::from_hresult(windows::core::HRESULT(UIA_E_INVALIDOPERATION as i32)))
    }
}

impl IRangeValueProvider_Impl for Item_Impl {
    /// The level asked for, posted to the window with the part's id (SetValue must not
    /// wait on it); the window sets it as a drag to it would ([`set_value_message`]).
    fn SetValue(&self, value: f64) -> windows::core::Result<()> {
        let (hwnd, part) = self.part()?;
        let Some(r) = part.range else {
            return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                UIA_E_INVALIDOPERATION as i32,
            )));
        };
        let Some(level) = level_asked(r, value) else {
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_INVALIDARG,
            ));
        };
        let set = Msg {
            id: set_value_message(),
            wparam: part.id as usize,
            lparam: level as isize,
        };
        post_message(hwnd, set).map_err(|_| gone())
    }

    fn Value(&self) -> windows::core::Result<f64> {
        Ok(f64::from(self.range()?.value))
    }

    fn IsReadOnly(&self) -> windows::core::Result<windows::core::BOOL> {
        self.range().map(|_| false.into())
    }

    fn Maximum(&self) -> windows::core::Result<f64> {
        Ok(f64::from(self.range()?.max))
    }

    fn Minimum(&self) -> windows::core::Result<f64> {
        Ok(f64::from(self.range()?.min))
    }

    fn LargeChange(&self) -> windows::core::Result<f64> {
        Ok(f64::from(self.range()?.step))
    }

    fn SmallChange(&self) -> windows::core::Result<f64> {
        Ok(f64::from(self.range()?.step))
    }
}

/// The level a SetValue of `value` asks of a part with range `r`, rounded to a whole
/// number (the levels are whole percents); `None` past its ends (UI Automation's rule:
/// refused, not clamped).
fn level_asked(r: Range, value: f64) -> Option<i32> {
    if !value.is_finite() || value < f64::from(r.min) || value > f64::from(r.max) {
        return None;
    }
    Some(value.round() as i32)
}

/// The message a screen reader's SetValue on a level posts its window: the part's id in
/// `wparam` and the level asked for (within the part's [`Range`]) in `lparam`. The window's
/// handler gets it as it is and sets the level the part is when it takes it, as a drag
/// to that value would.
pub fn set_value_message() -> u32 {
    static ID: OnceLock<u32> = OnceLock::new();
    *ID.get_or_init(|| crate::system::register_message("Kabuk.UiaSetValue"))
}

/// The message [`Item`]'s Invoke posts its window, the part's id in `wparam`. By the
/// part's place when Invoke was called, a click could land on another part: found by the
/// bar's screen reader test, where the figures came in between and the clock's click
/// opened a figure.
pub(crate) fn invoke_message() -> u32 {
    static ID: OnceLock<u32> = OnceLock::new();
    *ID.get_or_init(|| crate::system::register_message("Kabuk.UiaInvoke"))
}

/// The mouse messages of a click in the middle of part `id` of `parts` (left down, then
/// up), or none when it is gone.
pub(crate) fn clicks_of(parts: &[Part], id: u64) -> Vec<Msg> {
    let Some(p) = parts.iter().find(|p| p.id == id) else {
        return Vec::new();
    };
    let (x, y) = p.click;
    let lparam = ((y & 0xffff) << 16 | (x & 0xffff)) as isize;
    [(msg::LBUTTONDOWN, msg::MK_LBUTTON), (msg::LBUTTONUP, 0)]
        .into_iter()
        .map(|(id, wparam)| Msg { id, wparam, lparam })
        .collect()
}

/// A window's provider, kept by its [`crate::msgloop::Window`].
pub(crate) struct Accessible {
    shared: Shared,
    root: IRawElementProviderSimple,
}

impl Accessible {
    pub(crate) fn new(hwnd: Hwnd, container: Container, name: &str) -> Accessible {
        let shared = Arc::new(Mutex::new(Inner {
            hwnd: Some(hwnd),
            container,
            name: name.to_string(),
            parts: Vec::new(),
        }));
        let root: IRawElementProviderSimple = Root { shared: shared.clone() }.into();
        Accessible { shared, root }
    }

    /// Takes the window's parts as drawn now. Gives what changed that a listening screen
    /// reader is told of ([`tell`]): the part the keyboard moved to, the levels whose value
    /// moved (told once the caller no longer holds this, as UI Automation reads the parts
    /// back meanwhile).
    pub(crate) fn set(&self, container: Container, name: &str, parts: Vec<Part>) -> Vec<Told> {
        let focused = parts.iter().position(|p| p.focused);
        let (moved, levels) = {
            let mut inner = lock(&self.shared);
            let was = inner.parts.iter().find(|p| p.focused).map(|p| &p.name);
            let moved = focused.is_some_and(|i| was != Some(&parts[i].name));
            let levels = moved_levels(&inner.parts, &parts);
            inner.container = container;
            if inner.name != name {
                inner.name = name.to_string();
            }
            inner.parts = parts;
            (moved, levels)
        };
        // SAFETY: plain query.
        if (!moved && levels.is_empty()) || !unsafe { UiaClientsAreListening() }.as_bool() {
            return Vec::new();
        }
        let Ok(root) = self.root.cast::<IRawElementProviderFragmentRoot>() else {
            return Vec::new();
        };
        let provider = |i: usize| item(&self.shared, &root, i).cast::<IRawElementProviderSimple>().ok();
        let mut told: Vec<Told> = levels
            .into_iter()
            .filter_map(|(i, was, now)| Some(Told::Level(provider(i)?, was, now)))
            .collect();
        if let Some(part) = focused.filter(|_| moved).and_then(provider) {
            told.push(Told::Focus(part));
        }
        told
    }

    /// The click to make for an Invoke of part `id` ([`invoke_message`]), on its place now.
    pub(crate) fn clicks(&self, id: u64) -> Vec<Msg> {
        clicks_of(&lock(&self.shared).parts, id)
    }

    /// The window's root provider.
    pub(crate) fn root(&self) -> IRawElementProviderSimple {
        self.root.clone()
    }

    /// The window is going: its parts are gone for every client still holding them.
    pub(crate) fn release(&self, hwnd: HWND) {
        lock(&self.shared).hwnd = None;
        // SAFETY: releases what UI Automation keeps for this window, as its documentation
        // asks when the window is destroyed; then disconnects our root.
        unsafe {
            UiaReturnRawElementProvider(hwnd, WPARAM(0), LPARAM(0), None);
            let _ = UiaDisconnectProvider(&self.root);
        }
    }
}

/// The answer to a `WM_GETOBJECT` with `root`, when it asks for UI Automation's root.
pub(crate) fn answer(root: &IRawElementProviderSimple, hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    // The object id is a 32-bit value, whether or not it came sign-extended.
    if lparam.0 as i32 != UiaRootObjectId {
        return None;
    }
    // SAFETY: the arguments of the `WM_GETOBJECT` being handled, and our provider.
    Some(unsafe { UiaReturnRawElementProvider(hwnd, wparam, lparam, root) })
}

/// A change a listening screen reader is told of after a drawing.
pub(crate) enum Told {
    /// The keyboard is on this part now.
    Focus(IRawElementProviderSimple),
    /// This level's value went from the first to the second.
    Level(IRawElementProviderSimple, i32, i32),
}

/// The levels of `now` whose value differs from the same part's in `was`: their place in
/// `now`, the value before and the value now.
fn moved_levels(was: &[Part], now: &[Part]) -> Vec<(usize, i32, i32)> {
    now.iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let after = p.range?.value;
            let before = was.iter().find(|w| w.id == p.id)?.range?.value;
            (before != after).then_some((i, before, after))
        })
        .collect()
}

/// Tells screen readers of `told`.
pub(crate) fn tell(told: &Told) {
    // SAFETY: providers of one of our windows; UI Automation calls them back for the event.
    let r = unsafe {
        match told {
            Told::Focus(part) => UiaRaiseAutomationEvent(part, UIA_AutomationFocusChangedEventId),
            Told::Level(part, was, now) => UiaRaiseAutomationPropertyChangedEvent(
                part,
                UIA_RangeValueValuePropertyId,
                &VARIANT::from(f64::from(*was)),
                &VARIANT::from(f64::from(*now)),
            ),
        }
    };
    if let Err(e) = r {
        tracing::debug!(error = %e, "change not told");
    }
}

/// Says `text` aloud through a listening screen reader, as a notice of the window whose
/// root provider is `root` (Narrator reads it whatever it was reading).
pub(crate) fn announce(root: &IRawElementProviderSimple, text: &str) {
    // SAFETY: plain query.
    if !unsafe { UiaClientsAreListening() }.as_bool() {
        return;
    }
    // SAFETY: our window's provider; the strings live through the call.
    let r = unsafe {
        UiaRaiseNotificationEvent(
            root,
            NotificationKind_Other,
            NotificationProcessing_ImportantAll,
            &BSTR::from(text),
            &BSTR::from(NOTICE_ACTIVITY),
        )
    };
    if let Err(e) = r {
        tracing::debug!(error = %e, "notice not said");
    }
}

/// What Kabuk's notices are to a screen reader's settings (its "activity").
const NOTICE_ACTIVITY: &str = "Kabuk.Notice";

/// A part as a UI Automation client reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    pub name: String,
    /// `None` for a control type Kabuk does not give.
    pub role: Option<Role>,
    /// On the screen, in physical pixels.
    pub rect: kabuk_core::state::Rect,
}

fn automation() -> io::Result<IUIAutomation> {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    crate::system::com_init_mta();
    // SAFETY: plain creation of the UI Automation client object.
    unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.map_err(io::Error::other)
}

fn children(window: Hwnd) -> io::Result<Vec<IUIAutomationElement>> {
    let uia = automation()?;
    // SAFETY: plain UI Automation client calls; the window may be of any process.
    unsafe {
        let element = uia.ElementFromHandle(window.raw()).map_err(io::Error::other)?;
        let all = uia.CreateTrueCondition().map_err(io::Error::other)?;
        let found = element.FindAll(TreeScope_Children, &all).map_err(io::Error::other)?;
        let n = found.Length().map_err(io::Error::other)?;
        (0..n).map(|i| found.GetElement(i).map_err(io::Error::other)).collect()
    }
}

/// What a screen reader finds in `window`: its children with their names, roles and
/// places. Not to be called on the thread that owns the window (UI Automation would wait
/// on that thread for the answer).
pub fn read(window: Hwnd) -> io::Result<Vec<Seen>> {
    children(window)?
        .iter()
        .map(|e| {
            // SAFETY: plain UI Automation client calls on an element just found.
            unsafe {
                let name: BSTR = e.CurrentName().map_err(io::Error::other)?;
                let role = Role::of(e.CurrentControlType().map_err(io::Error::other)?);
                let r = e.CurrentBoundingRectangle().map_err(io::Error::other)?;
                Ok(Seen {
                    name: name.to_string(),
                    role,
                    rect: kabuk_core::state::Rect {
                        left: r.left,
                        top: r.top,
                        right: r.right,
                        bottom: r.bottom,
                    },
                })
            }
        })
        .collect()
}

/// The child of `window` named `name`.
fn child(window: Hwnd, name: &str) -> io::Result<IUIAutomationElement> {
    for e in children(window)? {
        // SAFETY: plain UI Automation client call on an element just found.
        if unsafe { e.CurrentName() }.map_err(io::Error::other)? == name {
            return Ok(e);
        }
    }
    Err(io::Error::new(io::ErrorKind::NotFound, name.to_string()))
}

/// Invokes the child of `window` named `name`, as a screen reader's user would.
pub fn invoke(window: Hwnd, name: &str) -> io::Result<()> {
    let e = child(window, name)?;
    // SAFETY: plain UI Automation client calls on an element just found.
    unsafe {
        let pattern: IUIAutomationInvokePattern =
            e.GetCurrentPatternAs(UIA_InvokePatternId).map_err(io::Error::other)?;
        pattern.Invoke().map_err(io::Error::other)
    }
}

fn range_pattern(window: Hwnd, name: &str) -> io::Result<IUIAutomationRangeValuePattern> {
    let e = child(window, name)?;
    // SAFETY: plain UI Automation client call on an element just found.
    unsafe { e.GetCurrentPatternAs(UIA_RangeValuePatternId) }.map_err(io::Error::other)
}

/// The level of the child of `window` named `name`, as a screen reader reads a slider.
pub fn range(window: Hwnd, name: &str) -> io::Result<Range> {
    let pattern = range_pattern(window, name)?;
    let whole = |v: windows::core::Result<f64>| v.map(|v| v.round() as i32).map_err(io::Error::other);
    // SAFETY: plain UI Automation client calls on a pattern just found.
    unsafe {
        Ok(Range {
            value: whole(pattern.CurrentValue())?,
            min: whole(pattern.CurrentMinimum())?,
            max: whole(pattern.CurrentMaximum())?,
            step: whole(pattern.CurrentSmallChange())?,
        })
    }
}

/// A screen reader listening to a window's notices ([`listen`]) while it is kept.
pub struct Listening {
    uia: IUIAutomation5,
    element: IUIAutomationElement,
    handler: IUIAutomationNotificationEventHandler,
    /// The words of each notice heard.
    pub heard: std::sync::mpsc::Receiver<String>,
}

#[implement(IUIAutomationNotificationEventHandler)]
struct Ear(Mutex<std::sync::mpsc::Sender<String>>);

impl IUIAutomationNotificationEventHandler_Impl for Ear_Impl {
    fn HandleNotificationEvent(
        &self,
        _: windows::core::Ref<IUIAutomationElement>,
        _: NotificationKind,
        _: NotificationProcessing,
        words: &BSTR,
        _: &BSTR,
    ) -> windows::core::Result<()> {
        if let Ok(tx) = self.0.lock() {
            let _ = tx.send(words.to_string());
        }
        Ok(())
    }
}

/// Listens to the notices `window` says ([`announce`]), as a screen reader does.
pub fn listen(window: Hwnd) -> io::Result<Listening> {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    crate::system::com_init_mta();
    // SAFETY: plain creation of the UI Automation client object; the newer class gives
    // the notices' events (Windows 10 1709 on).
    let uia: IUIAutomation5 =
        unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }.map_err(io::Error::other)?;
    let (tx, heard) = std::sync::mpsc::channel();
    let handler: IUIAutomationNotificationEventHandler = Ear(Mutex::new(tx)).into();
    // SAFETY: plain UI Automation client calls; the handler lives in `Listening`.
    let element = unsafe {
        let element = uia.ElementFromHandle(window.raw()).map_err(io::Error::other)?;
        uia.AddNotificationEventHandler(&element, TreeScope_Subtree, None, &handler)
            .map_err(io::Error::other)?;
        element
    };
    Ok(Listening {
        uia,
        element,
        handler,
        heard,
    })
}

impl Drop for Listening {
    fn drop(&mut self) {
        // SAFETY: removes the handler added in `listen`.
        let _ = unsafe { self.uia.RemoveNotificationEventHandler(&self.element, &self.handler) };
    }
}

/// Sets the level of the child of `window` named `name` to `value`, as a screen reader's
/// user would move a slider.
pub fn set_range(window: Hwnd, name: &str, value: f64) -> io::Result<()> {
    let pattern = range_pattern(window, name)?;
    // SAFETY: plain UI Automation client call on a pattern just found.
    unsafe { pattern.SetValue(value) }.map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    //! Edge cases: a window with parts answers a UI Automation client from another thread
    //! with their names, roles and screen places; Invoke posts the window a click in the
    //! part's middle; an unknown name is not invoked; parts handed over again are read as
    //! they are now (a shorter list, a new name); a window given no parts has no children.
    //! A level: read as a slider with its value, ends and step; SetValue within its ends
    //! (its ends included, a fraction rounded) posts the window the level asked for, past
    //! its ends or not a number it is refused; it cannot be invoked, nor a button set;
    //! drawn again with another value it is read so; words to read are neither invoked
    //! nor set; only a level whose value moved is told.
    use super::*;
    use crate::msgloop::{post_quit, run_until, Wake, Window};
    use crate::sync::NamedEvent;
    use std::sync::mpsc;
    use std::time::Duration;

    const CLASS: &str = "Kabuk.UiaTest";
    const QUIT: u32 = msg::APP + 1;
    const WAIT: Duration = Duration::from_secs(10);
    /// What the window says aloud when it takes its second parts.
    const SAID: &str = "Kabuk started again";
    /// A client and a provider in one process take turns: two at once made UI Automation
    /// fail one of them with `E_FAIL` (a screen reader is another process).
    static TURN: Mutex<()> = Mutex::new(());
    /// Far off every monitor, and never shown.
    const AT: PxRect = PxRect {
        x: -30000,
        y: -30000,
        w: 300,
        h: 40,
    };

    fn part(name: &str, role: Role, x: i32) -> Part {
        Part {
            id: part_id(name, 0),
            name: name.into(),
            role,
            rect: PxRect { x, y: 4, w: 40, h: 30 },
            click: (x + 20, 19),
            focused: false,
            range: None,
        }
    }

    const VOLUME: Range = Range {
        value: 40,
        min: 0,
        max: 100,
        step: 2,
    };

    fn slider(name: &str, x: i32, value: i32) -> Part {
        Part {
            range: Some(Range { value, ..VOLUME }),
            ..part(name, Role::Slider, x)
        }
    }

    struct TestWindow {
        hwnd: Hwnd,
        clicks: mpsc::Receiver<(u32, i32, i32)>,
        /// Levels asked for: the part's id and the level.
        levels: mpsc::Receiver<(u64, isize)>,
        swap: NamedEvent,
        swapped: mpsc::Receiver<()>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl TestWindow {
        /// A hidden window on its own thread with `first` as its parts, which takes `then`
        /// when [`TestWindow::swap`] asks; clicks it gets come back on a channel.
        fn new(tag: &str, first: Vec<Part>, then: Vec<Part>) -> TestWindow {
            let name = format!("Kabuk.UiaTest.{}.{tag}", std::process::id());
            let swap = NamedEvent::create_or_open(&name).unwrap();
            let (tx_hwnd, rx_hwnd) = mpsc::channel();
            let (tx_click, clicks) = mpsc::channel();
            let (tx_level, levels) = mpsc::channel();
            let (tx_swapped, swapped) = mpsc::channel();
            let thread = std::thread::spawn(move || {
                crate::system::com_init();
                let event = NamedEvent::create_or_open(&name).unwrap();
                let w = Window::shell_surface(CLASS, "", AT, move |m| {
                    match m.id {
                        msg::LBUTTONDOWN | msg::LBUTTONUP => {
                            let (x, y) = msg::point(m.lparam);
                            let _ = tx_click.send((m.id, x, y));
                        }
                        id if id == set_value_message() => {
                            let _ = tx_level.send((m.wparam as u64, m.lparam));
                        }
                        QUIT => post_quit(0),
                        _ => return None,
                    }
                    Some(0)
                })
                .unwrap();
                w.set_accessible(Container::Bar, "Test bar", first);
                tx_hwnd.send(w.hwnd()).unwrap();
                while let Wake::Signalled(_) = run_until(&[&event], Some(WAIT)) {
                    event.reset().unwrap();
                    w.set_accessible(Container::Bar, "Test bar", then.clone());
                    w.announce(SAID);
                    tx_swapped.send(()).unwrap();
                }
            });
            TestWindow {
                hwnd: rx_hwnd.recv().unwrap(),
                clicks,
                levels,
                swap,
                swapped,
                thread: Some(thread),
            }
        }

        fn swap(&self) {
            self.swap.set().unwrap();
            self.swapped.recv_timeout(WAIT).unwrap();
        }
    }

    impl Drop for TestWindow {
        fn drop(&mut self) {
            let _ = post_message(
                self.hwnd,
                Msg {
                    id: QUIT,
                    wparam: 0,
                    lparam: 0,
                },
            );
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    fn seen(parts: &[Part]) -> Vec<Seen> {
        parts
            .iter()
            .map(|p| Seen {
                name: p.name.clone(),
                role: Some(p.role),
                rect: kabuk_core::state::Rect {
                    left: AT.x + p.rect.x,
                    top: AT.y + p.rect.y,
                    right: AT.x + p.rect.x + p.rect.w,
                    bottom: AT.y + p.rect.y + p.rect.h,
                },
            })
            .collect()
    }

    #[test]
    fn a_client_reads_the_parts_and_invokes_one() {
        let parts = vec![
            part("Workspace 1", Role::Button, 10),
            part("Saat: 20:31", Role::Button, 60),
            part("Firefox", Role::ListItem, 110),
            part("Kilitle", Role::MenuItem, 160),
        ];
        let then = vec![part("Çalışma alanı 2", Role::Button, 10)];
        let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let w = TestWindow::new("read", parts.clone(), then.clone());
        assert_eq!(read(w.hwnd).unwrap(), seen(&parts));
        invoke(w.hwnd, "Saat: 20:31").unwrap();
        // The click a mouse would make in its middle: down, then up.
        assert_eq!(w.clicks.recv_timeout(WAIT).unwrap(), (msg::LBUTTONDOWN, 80, 19));
        assert_eq!(w.clicks.recv_timeout(WAIT).unwrap(), (msg::LBUTTONUP, 80, 19));
        assert_eq!(
            invoke(w.hwnd, "Nothing like it").map_err(|e| e.kind()),
            Err(io::ErrorKind::NotFound)
        );
        // Drawn again with fewer parts: read as they are now.
        w.swap();
        assert_eq!(read(w.hwnd).unwrap(), seen(&then));
    }

    /// Found by the bar's screen reader test: the figures came after the first bars and a
    /// reader's object for the clock, held from before, clicked a figure.
    #[test]
    fn a_part_held_by_a_reader_stays_itself_when_parts_come_before_it() {
        let first = vec![
            part("Workspace 1", Role::Button, 10),
            part("Saat: 20:31", Role::Button, 60),
        ];
        let mut clock = part("Saat: 20:31", Role::Button, 160);
        clock.id = first[1].id;
        let then = vec![
            part("Workspace 1", Role::Button, 10),
            part("CPU 4%", Role::Button, 60),
            part("GPU 2%", Role::Button, 110),
            clock,
        ];
        let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let w = TestWindow::new("held", first, then);
        let held = children(w.hwnd).unwrap().remove(1);
        w.swap();
        // SAFETY: plain UI Automation client calls on an element found above.
        unsafe {
            let pattern: IUIAutomationInvokePattern = held.GetCurrentPatternAs(UIA_InvokePatternId).unwrap();
            pattern.Invoke().unwrap();
        }
        assert_eq!(w.clicks.recv_timeout(WAIT).unwrap(), (msg::LBUTTONDOWN, 180, 19));
    }

    #[test]
    fn an_invoke_clicks_the_middle_of_the_part_where_it_is_now() {
        let mut moved = part("Saat: 20:31", Role::Button, 160);
        moved.id = 7;
        let ids = |m: &[Msg]| {
            m.iter()
                .map(|m| (m.id, m.wparam, msg::point(m.lparam)))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(&clicks_of(&[part("CPU 4%", Role::Button, 10), moved], 7)),
            [
                (msg::LBUTTONDOWN, msg::MK_LBUTTON, (180, 19)),
                (msg::LBUTTONUP, 0, (180, 19))
            ]
        );
        assert!(clicks_of(&[part("CPU 4%", Role::Button, 10)], 7).is_empty());
    }

    #[test]
    fn the_same_kind_and_value_give_the_same_id() {
        assert_eq!(part_id("clock", 0), part_id("clock", 0));
        assert_ne!(part_id("clock", 0), part_id("clock", 1));
        assert_ne!(part_id("clock", 0), part_id("stats", 0));
    }

    #[test]
    fn a_window_given_no_parts_has_no_children() {
        let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let w = TestWindow::new("empty", Vec::new(), Vec::new());
        assert_eq!(read(w.hwnd).unwrap(), Vec::new());
    }

    #[test]
    fn the_roles_round_trip_through_their_control_types() {
        for r in Role::ALL {
            assert_eq!(Role::of(r.control_type()), Some(r));
        }
        assert_eq!(Role::of(UIA_PaneControlTypeId), None);
    }

    /// PLAN M19 U17: a level was not offered at all, as Invoke would have set it to half.
    /// A screen reader reads it as a slider and sets it within its ends; the window is
    /// asked for the level, and read as it draws it next.
    #[test]
    fn a_level_is_read_as_a_range_and_set_through_its_window() {
        let first = vec![part("Mute", Role::Button, 10), slider("Volume", 60, 40)];
        let then = vec![part("Mute", Role::Button, 10), slider("Volume", 60, 64)];
        let id = first[1].id;
        let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let w = TestWindow::new("range", first.clone(), then);
        assert_eq!(read(w.hwnd).unwrap(), seen(&first));
        assert_eq!(read(w.hwnd).unwrap()[1].role, Some(Role::Slider));
        assert_eq!(range(w.hwnd, "Volume").unwrap(), VOLUME);
        set_range(w.hwnd, "Volume", 64.0).unwrap();
        assert_eq!(w.levels.recv_timeout(WAIT).unwrap(), (id, 64));
        // Its ends are within; a fraction is rounded.
        set_range(w.hwnd, "Volume", 0.0).unwrap();
        assert_eq!(w.levels.recv_timeout(WAIT).unwrap(), (id, 0));
        set_range(w.hwnd, "Volume", 99.6).unwrap();
        assert_eq!(w.levels.recv_timeout(WAIT).unwrap(), (id, 100));
        // Past its ends: refused, and nothing asked of the window.
        assert!(set_range(w.hwnd, "Volume", 101.0).is_err());
        assert!(set_range(w.hwnd, "Volume", -1.0).is_err());
        // A level is not invoked (a click in its middle would set it to half), and a
        // button is no level.
        assert!(invoke(w.hwnd, "Volume").is_err());
        assert!(range(w.hwnd, "Mute").is_err());
        assert!(w.clicks.try_recv().is_err());
        assert!(w.levels.try_recv().is_err());
        // The window drew it at the level asked: read so.
        w.swap();
        assert_eq!(range(w.hwnd, "Volume").unwrap().value, 64);
    }

    #[test]
    fn words_to_read_are_neither_invoked_nor_set() {
        let parts = vec![part("Kabuk started again", Role::Text, 10)];
        let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let w = TestWindow::new("text", parts.clone(), parts.clone());
        assert_eq!(read(w.hwnd).unwrap(), seen(&parts));
        assert!(invoke(w.hwnd, "Kabuk started again").is_err());
        assert!(range(w.hwnd, "Kabuk started again").is_err());
        assert!(w.clicks.try_recv().is_err());
    }

    /// PLAN M19 U17: Kabuk's notices were drawn only. A screen reader listening to a
    /// window hears what it says.
    #[test]
    fn a_screen_reader_hears_what_a_window_says() {
        let parts = vec![part(SAID, Role::Text, 10)];
        let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let w = TestWindow::new("said", parts.clone(), parts);
        let ear = listen(w.hwnd).unwrap();
        w.swap();
        assert_eq!(ear.heard.recv_timeout(WAIT).unwrap(), SAID);
    }

    #[test]
    fn a_level_asked_is_kept_within_its_ends() {
        assert_eq!(level_asked(VOLUME, 50.0), Some(50));
        assert_eq!(level_asked(VOLUME, 100.0), Some(100));
        assert_eq!(level_asked(VOLUME, 0.4), Some(0));
        assert_eq!(level_asked(VOLUME, 100.1), None);
        assert_eq!(level_asked(VOLUME, f64::NAN), None);
        assert_eq!(level_asked(VOLUME, f64::INFINITY), None);
    }

    #[test]
    fn only_a_level_whose_value_moved_is_told() {
        let was = vec![
            part("Mute", Role::Button, 10),
            slider("Volume", 60, 40),
            slider("Bright", 110, 70),
        ];
        let mut now = vec![
            slider("Bright", 110, 70),
            slider("Volume", 60, 42),
            part("Mute", Role::Button, 10),
        ];
        // Its place in the new list, and the values before and now.
        assert_eq!(moved_levels(&was, &now), vec![(1, 40, 42)]);
        now.push(slider("New", 160, 5));
        assert_eq!(moved_levels(&was, &now), vec![(1, 40, 42)], "a new level did not move");
        assert!(moved_levels(&was, &was).is_empty());
    }
}
