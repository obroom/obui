//! Grey placeholder text in an empty EDIT that stays while the box has focus and goes once something is typed,
//! like HTML `placeholder`.
//!
//! `EM_SETCUEBANNER` with "show while focused" is unreliable: on some systems (East Asian input methods among
//! them) the system hides the cue as soon as the box gets focus. So the hint is drawn here, and without flicker:
//! - while the box is empty its paint is ours: one blit of a cached picture (background + hint), no erase first;
//! - the EDIT also redraws its line straight to the screen on clicks, focus changes, keys and input-method changes.
//!   Around those messages its drawing is switched off (WM_SETREDRAW), and the picture is put back after, in case.
//!
//! Colours come from the parent's WM_CTLCOLOREDIT, so dark themes need nothing extra. Call [`set`] again to change
//! the text; an empty hint removes it.

use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Shell::{DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

const SUBCLASS_ID: usize = 0xC0E1;
const EM_GETRECT: u32 = 0x00B2;
const EM_SETSEL: u32 = 0x00B1;
const EM_REPLACESEL: u32 = 0x00C2;

/// What a picture was made for: (width, height, background, text colour)
type Key = (i32, i32, u32, u32);

struct Cue {
    hint: Vec<u16>,
    pic: RefCell<Option<(Key, HBITMAP)>>,
    /// How deep inside messages that switch the EDIT's drawing off we are
    depth: Cell<u32>,
}

impl Cue {
    fn free(&self) {
        if let Some((_, bmp)) = self.pic.borrow_mut().take() {
            unsafe {
                let _ = DeleteObject(bmp.into());
            }
        }
    }
}

/// Show `hint` in `edit` while it is empty (single-line EDIT; any number of calls)
pub fn set(edit: HWND, hint: &str) {
    if edit.is_invalid() {
        return;
    }
    let hint: Vec<u16> = hint.encode_utf16().collect();
    unsafe {
        let mut data = 0usize;
        if GetWindowSubclass(edit, Some(proc), SUBCLASS_ID, Some(&mut data)).as_bool() && data != 0 {
            let cue = &mut *(data as *mut Cue);
            cue.hint = hint;
            cue.free();
        } else {
            let cue = Box::into_raw(Box::new(Cue { hint, pic: RefCell::new(None), depth: Cell::new(0) }));
            let _ = SetWindowSubclass(edit, Some(proc), SUBCLASS_ID, cue as usize);
        }
        let _ = InvalidateRect(Some(edit), None, true);
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, data: usize) -> LRESULT {
    let cue = unsafe { &*(data as *const Cue) };
    let showing = || !cue.hint.is_empty() && unsafe { GetWindowTextLengthW(hwnd) } == 0;
    match msg {
        WM_ERASEBKGND if showing() => LRESULT(1),
        WM_PAINT if showing() => unsafe {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            blit(hwnd, cue, hdc);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        },
        // the message test goes first: showing() sends WM_GETTEXTLENGTH, which comes back through here
        _ if (draws_line(msg) || msg == WM_MOUSEMOVE && wp.0 & 0x13 != 0) && showing() => unsafe {
            // keep the EDIT from wiping its line while it stays empty; if this message fills it, it paints anew.
            // Only the outermost message switches drawing (WM_LBUTTONDOWN sends WM_SETFOCUS from inside)
            let outer = cue.depth.replace(cue.depth.get() + 1) == 0;
            if outer {
                let _ = DefSubclassProc(hwnd, WM_SETREDRAW, WPARAM(0), LPARAM(0));
            }
            let r = DefSubclassProc(hwnd, msg, wp, lp);
            cue.depth.set(cue.depth.get() - 1);
            if !outer {
                return r;
            }
            let _ = DefSubclassProc(hwnd, WM_SETREDRAW, WPARAM(1), LPARAM(0));
            if showing() {
                let hdc = GetDC(Some(hwnd));
                let _ = HideCaret(Some(hwnd));
                blit(hwnd, cue, hdc);
                let _ = ShowCaret(Some(hwnd));
                ReleaseDC(Some(hwnd), hdc);
            } else {
                let _ = InvalidateRect(Some(hwnd), None, true);
            }
            r
        },
        WM_SIZE | WM_SETFONT => {
            cue.free();
            unsafe { DefSubclassProc(hwnd, msg, wp, lp) }
        }
        WM_NCDESTROY => unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(proc), SUBCLASS_ID);
            drop(Box::from_raw(data as *mut Cue));
            DefSubclassProc(hwnd, msg, wp, lp)
        },
        _ => {
            let r = unsafe { DefSubclassProc(hwnd, msg, wp, lp) };
            // the last character just went (Backspace, Delete, cut, a cleared text): show the hint
            if draws_line(msg) && showing() {
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, true);
                }
            }
            r
        }
    }
}

/// Messages after which the EDIT may redraw its line itself (straight to the screen) or change its text
fn draws_line(msg: u32) -> bool {
    matches!(
        msg,
        WM_SETFOCUS
            | WM_KILLFOCUS
            | WM_LBUTTONDOWN
            | WM_LBUTTONUP
            | WM_LBUTTONDBLCLK
            | WM_RBUTTONDOWN
            | WM_RBUTTONUP
            | WM_CAPTURECHANGED
            | WM_TIMER
            | WM_ENABLE
            | WM_KEYDOWN
            | WM_KEYUP
            | WM_CHAR
            | WM_PASTE
            | WM_CUT
            | WM_CLEAR
            | WM_UNDO
            | WM_SETTEXT
            | EM_SETSEL
            | EM_REPLACESEL
            | WM_IME_SETCONTEXT
            | WM_IME_NOTIFY
            | WM_IME_STARTCOMPOSITION
            | WM_IME_COMPOSITION
            | WM_IME_ENDCOMPOSITION
            | WM_IME_CHAR
    )
}

/// Copy the picture (background + hint where typed text would start, in the text colour faded halfway into the
/// background) onto the box, making it first if the size or colours changed
fn blit(edit: HWND, cue: &Cue, hdc: HDC) {
    unsafe {
        let mut client = RECT::default();
        let _ = GetClientRect(edit, &mut client);
        if let Ok(parent) = GetParent(edit) {
            let _ = SendMessageW(parent, WM_CTLCOLOREDIT, Some(WPARAM(hdc.0 as usize)), Some(LPARAM(edit.0 as isize)));
        }
        let (bk, fg) = (GetBkColor(hdc), GetTextColor(hdc));
        let key = (client.right, client.bottom, bk.0, fg.0);
        let mem = CreateCompatibleDC(Some(hdc));
        let stale = cue.pic.borrow().as_ref().is_none_or(|(k, _)| *k != key);
        if stale {
            cue.free();
            let bmp = CreateCompatibleBitmap(hdc, client.right.max(1), client.bottom.max(1));
            let old = SelectObject(mem, bmp.into());
            let fill = CreateSolidBrush(bk);
            FillRect(mem, &client, fill);
            let _ = DeleteObject(fill.into());
            let mut rc = RECT::default();
            let _ = SendMessageW(edit, EM_GETRECT, None, Some(LPARAM(&mut rc as *mut RECT as isize)));
            let font = HFONT(SendMessageW(edit, WM_GETFONT, None, None).0 as *mut _);
            let old_font = SelectObject(mem, font.into());
            let _ = SetTextColor(mem, crate::search_edit::mix(fg, bk, 1, 2));
            let _ = SetBkMode(mem, TRANSPARENT);
            let mut text = cue.hint.clone();
            let _ = DrawTextW(mem, &mut text, &mut rc, DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS);
            SelectObject(mem, old_font);
            SelectObject(mem, old);
            *cue.pic.borrow_mut() = Some((key, bmp));
        }
        if let Some((_, bmp)) = &*cue.pic.borrow() {
            let old = SelectObject(mem, (*bmp).into());
            let _ = BitBlt(hdc, 0, 0, client.right, client.bottom, Some(mem), 0, 0, SRCCOPY);
            SelectObject(mem, old);
        }
        let _ = DeleteDC(mem);
    }
}
