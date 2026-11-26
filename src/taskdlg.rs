//! 系统任务对话框 (TaskDialog) 的两个包装: 自定义按钮的 [`dialog`] 和 `MessageBoxW` 的替身 [`msgbox`]。
//! 都在标题栏挂程序图标 (所有弹窗统一带 logo), 正文里的 `<a href>` 用浏览器打开 (只放 http(s))。

use windows::Win32::Foundation::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::WindowsAndMessaging::{IDCANCEL, IDNO, IDOK, MB_DEFBUTTON2, MB_ICONWARNING, MB_SETFOREGROUND, MESSAGEBOX_RESULT, MESSAGEBOX_STYLE};
use windows::core::{HRESULT, PCWSTR};

use crate::win::{set_window_icon, wide};

/// 弹一个任务对话框 (系统自带的那种大标题+说明的框, 比 MessageBox 能放链接和自定义按钮)。
/// `buttons` 是额外按钮 (id, 文字), 永远再带一个"关闭"; 返回点中的按钮 id。
/// `links=true` 时正文里的 `<a href="...">x</a>` 可点 (回调里 ShellExecute)
pub fn dialog(owner: HWND, title: &str, main: &str, content: &str, buttons: &[(i32, &str)], links: bool) -> i32 {
    // 这些 Vec 必须活到 TaskDialogIndirect 返回 —— 结构体里存的是裸指针
    let (wt, wm, wc) = (wide(title), wide(main), wide(content));
    let btn_text: Vec<Vec<u16>> = buttons.iter().map(|(_, t)| wide(t)).collect();
    let btns: Vec<TASKDIALOG_BUTTON> = buttons
        .iter()
        .zip(&btn_text)
        .map(|((id, _), t)| TASKDIALOG_BUTTON { nButtonID: *id, pszButtonText: PCWSTR(t.as_ptr()) })
        .collect();

    // SIZE_TO_CONTENT: 框跟着内容变宽, 不够就裁。**必须有** —— 64 位的 SHA256 中间没有空格,
    // TaskDialog 断不了行, 默认宽度下直接被切掉半截。内容短的框不受影响, 这个标志只会让它变宽不会变窄
    let mut flags = TDF_ALLOW_DIALOG_CANCELLATION | TDF_POSITION_RELATIVE_TO_WINDOW | TDF_SIZE_TO_CONTENT;
    if links {
        flags |= TDF_ENABLE_HYPERLINKS;
    }
    let cfg = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: owner,
        dwFlags: flags,
        dwCommonButtons: TDCBF_CLOSE_BUTTON,
        pszWindowTitle: PCWSTR(wt.as_ptr()),
        pszMainInstruction: PCWSTR(wm.as_ptr()),
        pszContent: PCWSTR(wc.as_ptr()),
        cButtons: btns.len() as u32,
        pButtons: btns.as_ptr(),
        pfCallback: Some(callback),
        ..Default::default()
    };
    let mut pressed = 0i32;
    unsafe {
        let _ = TaskDialogIndirect(&cfg, Some(&mut pressed), None, None);
    }
    pressed
}

/// 系统 `MessageBoxW` 的替身: 参数、返回值一模一样 (IDOK / IDYES / IDNO / IDCANCEL), 但长得跟 [`dialog`] 一样, 标题栏带程序图标
/// (系统消息框挂不了)。认 MB_OK / OKCANCEL / YESNO / YESNOCANCEL、MB_DEFBUTTON2、MB_SETFOREGROUND;
/// 正文一般不放图标 (logo 只在标题栏左上角, 正文再来一个大的是重复); 例外是**危险操作的确认** ——
/// MB_ICONWARNING 又带选择 (是/否、确定/取消; 永久删除、覆盖、结束进程这些) 放系统的黄色警告三角, 让它跟普通提示区分开。
/// 只有「确定」的警告是普通提示, 不放。unsafe 跟原函数同理: text / caption 是裸指针
///
/// # Safety
/// text / caption 要是有效的以 0 结尾的宽字符串 (跟 MessageBoxW 一样)。
pub unsafe fn msgbox<P1, P2>(owner: Option<HWND>, text: P1, caption: P2, style: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT
where
    P1: windows::core::Param<PCWSTR>,
    P2: windows::core::Param<PCWSTR>,
{
    let (text, caption) = unsafe { (text.param().abi(), caption.param().abi()) };
    let (text, caption) = unsafe { (text.to_string().unwrap_or_default(), caption.to_string().unwrap_or_default()) };
    let (buttons, second) = match style.0 & 0xF {
        1 => (TDCBF_OK_BUTTON | TDCBF_CANCEL_BUTTON, IDCANCEL),                // MB_OKCANCEL
        3 => (TDCBF_YES_BUTTON | TDCBF_NO_BUTTON | TDCBF_CANCEL_BUTTON, IDNO), // MB_YESNOCANCEL
        4 => (TDCBF_YES_BUTTON | TDCBF_NO_BUTTON, IDNO),                       // MB_YESNO
        _ => (TDCBF_OK_BUTTON, IDOK),
    };
    let (wt, wc) = (wide(&caption), wide(&text));
    let flags = TDF_ALLOW_DIALOG_CANCELLATION | TDF_POSITION_RELATIVE_TO_WINDOW | TDF_SIZE_TO_CONTENT;
    let mut cfg = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: owner.unwrap_or_default(),
        dwFlags: flags,
        dwCommonButtons: buttons,
        pszWindowTitle: PCWSTR(wt.as_ptr()),
        pszContent: PCWSTR(wc.as_ptr()),
        nDefaultButton: if style.0 & 0xF00 == MB_DEFBUTTON2.0 { second.0 } else { 0 },
        pfCallback: Some(callback),
        lpCallbackData: (style.0 & MB_SETFOREGROUND.0 != 0) as isize, // callback 里 1 = 提到前台
        ..Default::default()
    };
    if style.0 & 0xF0 == MB_ICONWARNING.0 && buttons != TDCBF_OK_BUTTON {
        cfg.Anonymous1.pszMainIcon = TD_WARNING_ICON;
    }
    let mut pressed = 0i32;
    unsafe {
        let _ = TaskDialogIndirect(&cfg, Some(&mut pressed), None, None);
    }
    // 是/否 框原来没有取消: Esc / 右上角 ✕ 当"否"
    if style.0 & 0xF == 4 && pressed == IDCANCEL.0 {
        return IDNO;
    }
    MESSAGEBOX_RESULT(pressed)
}

/// 两件事: 建好时挂程序图标; 正文里的链接被点了就用浏览器打开
unsafe extern "system" fn callback(hwnd: HWND, msg: TASKDIALOG_NOTIFICATIONS, _wp: WPARAM, lp: LPARAM, data: isize) -> HRESULT {
    // 标题栏左上角也带程序图标, 跟自建弹窗一个样子 (TaskDialog 是系统画的, 不走 dress)
    if msg == TDN_CREATED {
        set_window_icon(hwnd);
        if data == 1 {
            let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(hwnd) };
        }
    }
    if msg == TDN_HYPERLINK_CLICKED && lp.0 != 0 {
        let url = unsafe { PCWSTR(lp.0 as *const u16).to_string() }.unwrap_or_default();
        if !url.is_empty() {
            open_url(&url);
        }
    }
    HRESULT(0) // S_OK
}

/// 用默认浏览器开链接。只放 http(s): ShellExecute 什么协议都认 (file:、ms-settings:、自己装的应用协议…),
/// 服务端下发的地址 / 正文里的链接一旦被改, 那就是个任意程序启动口
pub fn open_url(url: &str) {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        log::warn!("open_url: 拒绝非 http(s) 链接 {url}");
        return;
    }
    let u = wide(url);
    unsafe {
        windows::Win32::UI::Shell::ShellExecuteW(None, windows::core::w!("open"), PCWSTR(u.as_ptr()), None, None, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL);
    }
}
