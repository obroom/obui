//! 搜索框「框内清除按钮」—— 模拟 HTML `<input type="search">` 右侧的 ✕ (照搬 hotkit src/gui/search_edit.rs)。
//!
//! 对任意已创建好的单行搜索 EDIT 调 [`attach`] 即可。按钮做成 **EDIT 的
//! 子窗口**, 三件事全自动、宿主零改动:
//! - 跟随 EDIT 移动/缩放 (subclass 拦 WM_SIZE 重摆);
//! - EDIT 隐藏时按钮作为子窗口跟着不可见;
//! - 按钮的 WM_COMMAND / WM_DRAWITEM 都发给 EDIT、被 subclass 拦下,
//!   不占宿主窗口的控件 ID, 宿主消息路由一行不用改。
//!
//! 行为: 有内容才显示; 点击清空文字并把焦点还给 EDIT。清空走 WM_SETTEXT,
//! 单行 EDIT 会照常给宿主发 EN_CHANGE, 已有的过滤 / 搜索刷新逻辑自动跟上。
//! `EM_SETMARGINS` 常驻给文字让出右侧空间, 长文本不会钻到按钮底下,
//! 出现/消失时文字排版也不跳动。

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_SELECTED};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

/// 按钮在 EDIT 内部的控件 ID。EDIT 自己不会有别的子控件, 不存在碰撞。
const BTN_ID: i32 = 1;
const SUBCLASS_ID: usize = 0x5EA8;
/// 按钮右缘距 EDIT 客户区右缘。
const RIGHT_PAD: i32 = 3;
/// 按钮上下各留的空隙 (按钮为正方形, 边长 = 客户区高 - 2×V_INSET)。
const V_INSET: i32 = 3;

/// 给单行搜索 EDIT 挂上框内清除按钮。EDIT 创建后调一次即可。
pub fn attach(edit: HWND) {
    if edit.is_invalid() {
        return;
    }
    unsafe {
        let hinstance = GetModuleHandleW(None).unwrap_or_default();
        let _ = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            windows::core::w!("BUTTON"),
            None,
            WS_CHILD | WINDOW_STYLE(BS_OWNERDRAW as u32),
            0,
            0,
            1,
            1,
            Some(edit),
            Some(HMENU(BTN_ID as *mut _)),
            Some(hinstance.into()),
            None,
        );
        // WS_CLIPCHILDREN: EDIT 重画文字时不刷按钮那块, 避免闪烁/残影。
        let style = GetWindowLongPtrW(edit, GWL_STYLE);
        SetWindowLongPtrW(edit, GWL_STYLE, style | WS_CLIPCHILDREN.0 as isize);
        let _ = SetWindowSubclass(edit, Some(subclass_proc), SUBCLASS_ID, 0);
    }
    layout(edit);
    sync_visibility(edit);
}

fn btn(edit: HWND) -> Option<HWND> {
    let h = unsafe { GetDlgItem(Some(edit), BTN_ID).unwrap_or_default() };
    if h.is_invalid() { None } else { Some(h) }
}

/// 按 EDIT 当前客户区尺寸重摆按钮 + 刷新右 margin。attach 时和每次
/// WM_SIZE 后各调一次 (面板 layout 挪 EDIT 也会触发 WM_SIZE)。
fn layout(edit: HWND) {
    let Some(b) = btn(edit) else { return };
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(edit, &mut rc);
    }
    let h = rc.bottom - rc.top;
    if h <= 0 || rc.right <= 0 {
        return;
    }
    let side = (h - V_INSET * 2).max(12);
    unsafe {
        let _ = MoveWindow(b, rc.right - RIGHT_PAD - side, (h - side) / 2, side, side, true);
        const EM_SETMARGINS: u32 = 0x00D3;
        const EC_RIGHTMARGIN: usize = 2;
        let margin = ((side + RIGHT_PAD * 2) as isize) << 16; // 右 margin 在 HIWORD
        let _ = SendMessageW(edit, EM_SETMARGINS, Some(WPARAM(EC_RIGHTMARGIN)), Some(LPARAM(margin)));
    }
}

/// 有内容显示、空则隐藏。SW_SHOWNA 不抢焦点 (用户正在打字)。
fn sync_visibility(edit: HWND) {
    let Some(b) = btn(edit) else { return };
    let len = unsafe { GetWindowTextLengthW(edit) };
    unsafe {
        let _ = ShowWindow(b, if len > 0 { SW_SHOWNA } else { SW_HIDE });
    }
}

unsafe extern "system" fn subclass_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _ref_data: usize) -> LRESULT {
    match msg {
        WM_SIZE => {
            let r = unsafe { DefSubclassProc(hwnd, msg, wp, lp) };
            layout(hwnd);
            r
        }
        WM_DRAWITEM => {
            let dis = unsafe { &*(lp.0 as *const DRAWITEMSTRUCT) };
            if dis.CtlID as i32 == BTN_ID {
                draw_clear_button(dis);
                return LRESULT(1);
            }
            unsafe { DefSubclassProc(hwnd, msg, wp, lp) }
        }
        WM_COMMAND => {
            let id = (wp.0 & 0xFFFF) as i32;
            let code = ((wp.0 >> 16) & 0xFFFF) as u32;
            if id == BTN_ID && code == 0 {
                // BN_CLICKED。清空走 WM_SETTEXT → 单行 EDIT 自动向宿主发
                // EN_CHANGE, 过滤刷新 + 本组件的显隐同步都顺原链路走。
                unsafe {
                    let _ = SetWindowTextW(hwnd, windows::core::w!(""));
                    let _ = SetFocus(Some(hwnd));
                }
                return LRESULT(0);
            }
            unsafe { DefSubclassProc(hwnd, msg, wp, lp) }
        }
        // 所有可能改文字的消息: 先让 EDIT 处理, 再同步按钮显隐。
        WM_CHAR | WM_KEYUP | WM_PASTE | WM_CUT | WM_CLEAR | WM_UNDO | WM_SETTEXT | WM_IME_CHAR | WM_IME_COMPOSITION => {
            let r = unsafe { DefSubclassProc(hwnd, msg, wp, lp) };
            sync_visibility(hwnd);
            r
        }
        WM_DESTROY => unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
            DefSubclassProc(hwnd, msg, wp, lp)
        },
        _ => unsafe { DefSubclassProc(hwnd, msg, wp, lp) },
    }
}

/// `a` 向 `b` 靠 `num/den` 的线性混色 (COLORREF 是 0x00BBGGRR)。
pub(crate) fn mix(a: COLORREF, b: COLORREF, num: i32, den: i32) -> COLORREF {
    let ch = |sa: u32, sb: u32| -> u32 {
        let (va, vb) = ((a.0 >> sa) & 0xFF, (b.0 >> sb) & 0xFF);
        (va as i32 + (vb as i32 - va as i32) * num / den) as u32
    };
    COLORREF(ch(0, 0) | (ch(8, 8) << 8) | (ch(16, 16) << 16))
}

/// 自绘 ✕: 字形 0xE894 (Clear), 平时半淡、hover 画圆角高亮并
/// 转正文色 —— 与 HTML search ✕ 观感一致。
///
/// 配色不硬编码: 把 WM_CTLCOLOREDIT 原样发给宿主 (EDIT 的父窗口 = 面板), 面板
/// 会在我们的 HDC 上 SetBkColor/SetTextColor (暗色主题就是这么给输入框着色的);
/// 拿到底/字两色后按比例混色, 任何主题下自动协调。
fn draw_clear_button(dis: &DRAWITEMSTRUCT) {
    let hdc = dis.hDC;
    let rc = dis.rcItem;
    let is_hot = (dis.itemState.0 & 0x0040) != 0; // ODS_HOTLIGHT
    let is_sel = (dis.itemState.0 & ODS_SELECTED.0) != 0; // 按下 (0x10 是 ODS_FOCUS, 别混)
    unsafe {
        let edit = GetParent(dis.hwndItem).unwrap_or_default();
        let host = if edit.is_invalid() { HWND::default() } else { GetParent(edit).unwrap_or_default() };
        if !host.is_invalid() {
            let _ = SendMessageW(host, WM_CTLCOLOREDIT, Some(WPARAM(hdc.0 as usize)), Some(LPARAM(edit.0 as isize)));
        }
        let bk = GetBkColor(hdc);
        let txt = GetTextColor(hdc);

        let fill = if is_sel {
            mix(bk, txt, 1, 5)
        } else if is_hot {
            mix(bk, txt, 1, 9)
        } else {
            bk
        };
        let brush = CreateSolidBrush(fill);
        if is_hot || is_sel {
            let bg = CreateSolidBrush(bk);
            let _ = FillRect(hdc, &rc, bg);
            let _ = DeleteObject(bg.into());
            let old_brush = SelectObject(hdc, brush.into());
            let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
            let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, 6, 6);
            SelectObject(hdc, old_brush);
            SelectObject(hdc, old_pen);
        } else {
            let _ = FillRect(hdc, &rc, brush);
        }
        let _ = DeleteObject(brush.into());

        // hotkit 用缓存的 MDL2 字体 (字高 × 3/4); 这里按输入框当前字体现算 (字号能热重载), 画完就删
        let edit_font = HFONT(SendMessageW(edit, WM_GETFONT, None, None).0 as *mut _);
        let font = crate::win::icon_font(crate::win::text_height(edit_font) * 3 / 4);
        let old_font = SelectObject(hdc, font.into());
        let col = if is_hot || is_sel { txt } else { mix(txt, bk, 2, 5) };
        let _ = SetTextColor(hdc, col);
        let _ = SetBkMode(hdc, TRANSPARENT);
        let mut ch = [0xE894u16]; // Clear ✕
        let mut rc_mut = rc;
        let _ = DrawTextW(hdc, &mut ch, &mut rc_mut, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
        SelectObject(hdc, old_font);
        let _ = DeleteObject(font.into());
    }
}
