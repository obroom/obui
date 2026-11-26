//! Win32 小工具: 宽字符串、消息、字体、配色、暗色主题、程序图标。所有 Win32 调用都在 UI 线程。

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

/// &str → 带尾 0 的 UTF-16。
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 带尾 0 的 u16 切片 → PCWSTR。调用方保证生命周期。
pub fn pcw(s: &[u16]) -> PCWSTR {
    PCWSTR(s.as_ptr())
}

pub fn send(hwnd: HWND, msg: u32, w: usize, l: isize) -> isize {
    unsafe { SendMessageW(hwnd, msg, Some(WPARAM(w)), Some(LPARAM(l))).0 }
}

/// 系统消息字体 (Segoe UI / 微软雅黑 9pt), 不用 DEFAULT_GUI_FONT 那个 Tahoma。
pub fn gui_font() -> HFONT {
    unsafe {
        let mut ncm = NONCLIENTMETRICSW {
            cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
            ..Default::default()
        };
        let ok = SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            ncm.cbSize,
            Some(&mut ncm as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        if ok.is_ok() {
            CreateFontIndirectW(&ncm.lfMessageFont)
        } else {
            HFONT(GetStockObject(DEFAULT_GUI_FONT).0)
        }
    }
}

/// config 里字号没写 (0) 时用多大: 10 pt。系统消息字体是 9 pt, 当文件管理器的正文偏小 (B哥 2026-11-16 看了 9 / 10 对比图定的);
/// 系统字体本身调得比 10 大 (辅助功能那种) 就跟系统, 不往小了改
pub const DEFAULT_PT: u32 = 10;

/// 系统消息字体的字号 (pt, 按主屏 DPI 折算回来)
pub fn sys_font_pt() -> u32 {
    unsafe {
        let mut ncm = NONCLIENTMETRICSW { cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
        let _ = SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, ncm.cbSize, Some(&mut ncm as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        let dpi = windows::Win32::UI::HiDpi::GetDpiForSystem().max(1) as i32;
        ((-ncm.lfMessageFont.lfHeight * 72 + dpi / 2) / dpi).max(1) as u32
    }
}

/// 字号 0 (没写) 实际用多大, 见 [`DEFAULT_PT`]
pub fn default_pt() -> u32 {
    sys_font_pt().max(DEFAULT_PT)
}

/// 列表 / 界面字体: 系统消息字体为底, name 非空换字体名; 字号 pt, 0 = [`default_pt`] (按主屏 DPI 折算)
pub fn make_font(name: &str, pt: u32) -> HFONT {
    let pt = if pt == 0 { default_pt() } else { pt };
    unsafe {
        let mut ncm = NONCLIENTMETRICSW { cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
        let _ = SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, ncm.cbSize, Some(&mut ncm as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        let mut lf = ncm.lfMessageFont;
        let dpi = windows::Win32::UI::HiDpi::GetDpiForSystem() as i32;
        lf.lfHeight = -(pt as i32 * dpi / 72);
        if !name.is_empty() {
            let w = wide(name);
            let n = w.len().min(lf.lfFaceName.len() - 1);
            lf.lfFaceName = [0; 32];
            lf.lfFaceName[..n].copy_from_slice(&w[..n]);
        }
        CreateFontIndirectW(&lf)
    }
}

/// 两色按 t 混合 (t=0 取 a, t=1 取 b)
pub fn blend(a: COLORREF, b: COLORREF, t: f32) -> COLORREF {
    let ch = |s: u32| {
        let (x, y) = ((a.0 >> s & 0xFF) as f32, (b.0 >> s & 0xFF) as f32);
        (((x + (y - x) * t) as u32) & 0xFF) << s
    };
    COLORREF(ch(0) | ch(8) | ch(16))
}

/// 用纯色填一块。自己建/删画刷 —— 这套三行的 CreateSolidBrush / FillRect / DeleteObject
/// 原来在各处抄了一遍又一遍, 暗色主题把这种填充又加了好些, 收成一个
pub fn fill(hdc: HDC, rc: &RECT, col: COLORREF) {
    unsafe {
        let br = CreateSolidBrush(col);
        FillRect(hdc, rc, br);
        let _ = DeleteObject(br.into());
    }
}

thread_local! {
    /// 回给 WM_CTLCOLOR* 的画刷缓存: (颜色, 画刷)
    static CTL_BRUSHES: std::cell::RefCell<Vec<(u32, isize)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// WM_CTLCOLOR* 必须回一个**之后还活着**的画刷 —— 系统拿它去擦控件背景, 当场删掉就是画到野指针。
/// 所以按颜色缓存, 一个颜色只建一次。用到的颜色就那么几种 (列表底 / chrome 底), 表不会长。
pub fn ctl_brush(col: COLORREF) -> HBRUSH {
    CTL_BRUSHES.with(|c| {
        let mut v = c.borrow_mut();
        if let Some((_, h)) = v.iter().find(|(k, _)| *k == col.0) {
            return HBRUSH(*h as *mut _);
        }
        let br = unsafe { CreateSolidBrush(col) };
        v.push((col.0, br.0 as isize));
        br
    })
}

/// COLORREF → "#RRGGBB"。COLORREF 内部是 BGR, 写进 CSS / config 要翻过来
pub fn hex(c: COLORREF) -> String {
    format!("#{:02X}{:02X}{:02X}", c.0 & 0xFF, c.0 >> 8 & 0xFF, c.0 >> 16 & 0xFF)
}

/// 感知亮度 0.0~1.0
pub fn luminance(c: COLORREF) -> f32 {
    let (r, g, b) = ((c.0 & 0xFF) as f32, (c.0 >> 8 & 0xFF) as f32, (c.0 >> 16 & 0xFF) as f32);
    (0.299 * r + 0.587 * g + 0.114 * b) / 255.0
}

/// 底色上该用黑字还是白字 (按感知亮度; 阈值 0.6 偏向白字, 深色底更常见)
pub fn contrast_text(bg: COLORREF) -> COLORREF {
    if luminance(bg) > 0.6 { COLORREF(0x00202020) } else { COLORREF(0x00FFFFFF) }
}

/// 这个底色算不算"暗色"。**整个暗色模式只有这一个判据** —— 不另存开关, 也不读系统设置:
/// 主题/配置给出底色, 是不是暗的由底色自己说了算。用户把 bg_color 调黑, 剩下的自动跟上。
pub fn is_dark(bg: COLORREF) -> bool {
    luminance(bg) < 0.5
}

/// 标题栏跟着变暗。**这个是有文档的** (DwmSetWindowAttribute + DWMWA_USE_IMMERSIVE_DARK_MODE),
/// 不是那堆未文档化的 uxtheme 序号。属性号在 Win10 1809~1903 是 19, 之后是 20 —— 两个都发一遍,
/// 不支持的那个返回错误, 忽略即可。
pub fn title_bar_dark(hwnd: HWND, dark: bool) {
    let on: i32 = dark as i32;
    for attr in [20u32, 19] {
        let _ = unsafe {
            windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                hwnd,
                windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE(attr as i32),
                &on as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            )
        };
    }
}

/// 强调色铺成一块底 (当前标签 / 当前盘符): 浅色下往 chrome 混淡 (t 越大越淡), 暗色下原样用。
/// 浅色主题的强调色是高饱和的 (#2F3E4E 石墨 / #0E9F6E 翡翠), 不冲淡太重; 暗色主题那套本来就是
/// 按深底挑的暗色 (#3B5A7A), 再往 #2B2B2B 混 80% 就跟底糊成一片 —— 实测当前标签 #2E343A, 肉眼认不出
pub fn wash(col: COLORREF, chrome: COLORREF, t: f32) -> COLORREF {
    if is_dark(chrome) { col } else { blend(col, chrome, t) }
}

/// 暗色下把系统画的**浅色按钮面**盖成 chrome 底, 并回一个该用的前景色;
/// 浅色下什么都不盖, 原样回系统的 COLOR_BTNTEXT —— 也就是说这函数在浅色下是恒等变换。
///
/// 给那些"系统画按钮面 + 我们在上面补个箭头/字形"的小按钮用 (标签栏右端的 ▾ ◂ ▸ +)。
/// Win32 的推按钮**没有暗色部件**, 只能这样盖掉。
///
/// 盖的时候把系统那圈圆角边框也补回来 (第一版只铺底, 四个按钮的边全没了, 符号悬在半空 —— B哥 报的),
/// 悬停 / 按下各亮一档: 系统画的那层反馈被我们盖掉了, 不补就是点下去毫无反应。
/// 主题按钮进出悬停、按下松开时系统自己会重画, 这里每次重画时问一下状态就够
pub fn dark_btn_face(hwnd: HWND, hdc: HDC, rc: &RECT, chrome: COLORREF, chrome_fg: COLORREF) -> COLORREF {
    const BM_GETSTATE: u32 = 0x00F2;
    const BST_PUSHED: u32 = 0x0004;
    const BST_HOT: u32 = 0x0200;
    if !is_dark(chrome) {
        return unsafe { COLORREF(GetSysColor(COLOR_BTNTEXT)) };
    }
    let st = unsafe { SendMessageW(hwnd, BM_GETSTATE, None, None) }.0 as u32;
    let face = if st & BST_PUSHED != 0 {
        blend(chrome, chrome_fg, 0.22)
    } else if st & BST_HOT != 0 {
        blend(chrome, chrome_fg, 0.12)
    } else {
        chrome
    };
    fill(hdc, rc, chrome);
    unsafe {
        let (br, pen) = (CreateSolidBrush(face), CreatePen(PS_SOLID, 1, blend(chrome, chrome_fg, 0.35)));
        let (ob, op) = (SelectObject(hdc, br.into()), SelectObject(hdc, pen.into()));
        // 四周让 1px (相邻按钮之间留出缝, 跟浅色下系统画的一样), 圆角随按钮高度走
        let r = ((rc.bottom - rc.top) / 5).max(3);
        let _ = RoundRect(hdc, rc.left + 1, rc.top + 1, rc.right - 1, rc.bottom - 1, r, r);
        SelectObject(hdc, ob);
        SelectObject(hdc, op);
        let _ = DeleteObject(br.into());
        let _ = DeleteObject(pen.into());
    }
    chrome_fg
}

/// 让**整个进程**进入暗色态。
///
/// **实测边界 (沙箱 A/B 过)**: 它对**菜单栏本身**没有影响 —— 菜单栏也不跟 DWM 的 immersive 属性走,
/// 标题栏变黑之后它照样是白的 (2026-09-23 shotnc 截图确认); 要暗只能接未文档化的 WM_UAHDRAWMENU 自绘。
/// 对**对话框里的按钮和下拉框**也没有影响 (Win32 推按钮根本没有暗色部件, 要暗只能自绘)。
/// 留着它是为了**点开后的弹出菜单** —— 那是 Win32 里唯一由它管、又没法用别的办法变暗的一块,
/// 而弹出菜单在隔离桌面里打不开 (要真实键盘输入), 沙箱验不了。
///
/// ⚠ **这是全项目唯一一处未文档化的脏活**: `uxtheme.dll` 的 **序号 135** (`SetPreferredAppMode`, 外加紧跟的 136 `FlushMenuThemes`),
/// 没有名字可导, 只能按序号取。资源管理器、记事本、以及一众第三方软件的暗色模式走的都是它,
/// 从 Win10 1809 (build 17763) 一路到现在签名没变过 —— 但它终究是序号, 微软要是重排了,
/// 取到的就是**另一个函数**, 调下去是 UB。
///
/// 所以加了两道闸:
///   1. build < 17763 直接不碰 (那之前压根没有暗色模式, 序号 135 是别的东西)
///   2. `GetProcAddress` 取不到就算了
///
/// 参数 2 = `ForceDark`, 0 = `Default`(还原)。只在启动时和主题切换时各调一次, 不在热路径上。
pub fn set_app_dark_mode(dark: bool) {
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    // build 号: RtlGetVersion 才拿得到真值 (GetVersionEx 被兼容性垫片骗)
    let build = unsafe {
        let mut vi = windows::Win32::System::SystemInformation::OSVERSIONINFOW {
            dwOSVersionInfoSize: std::mem::size_of::<windows::Win32::System::SystemInformation::OSVERSIONINFOW>() as u32,
            ..Default::default()
        };
        let ntdll = LoadLibraryW(windows::core::w!("ntdll.dll")).unwrap_or_default();
        match GetProcAddress(ntdll, windows::core::s!("RtlGetVersion")) {
            Some(f) => {
                let g: unsafe extern "system" fn(*mut _) -> i32 = std::mem::transmute(f);
                g(&mut vi as *mut _ as *mut _);
                vi.dwBuildNumber
            }
            None => 0,
        }
    };
    if build < 17763 {
        return;
    }
    unsafe {
        let Ok(ux) = LoadLibraryW(windows::core::w!("uxtheme.dll")) else { return };
        // MAKEINTRESOURCEA(135): 高位全 0 的 PCSTR 就是"按序号取"
        let Some(f) = GetProcAddress(ux, windows::core::PCSTR(135 as *const u8)) else { return };
        let set: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(f);
        set(if dark { 2 } else { 0 });
        // 序号 136 = FlushMenuThemes: 弹出菜单的主题系统缓存着, 运行中切主题不冲一下右键菜单还是旧配色 (B哥 报)。
        // 跟 135 同一批、同一道 build 闸, 无参无返回
        if let Some(f) = GetProcAddress(ux, windows::core::PCSTR(136 as *const u8)) {
            let flush: unsafe extern "system" fn() = std::mem::transmute(f);
            flush();
        }
    }
}

/// 换控件的视觉主题。ListView / TreeView 的**滚动条**归系统主题管, 我们设 LVM_SETBKCOLOR 也改不到它,
/// 只能靠换主题名: 暗色下是 "DarkMode_Explorer", 否则白滚动条直接贴在黑列表边上。
///
/// ⚠ "DarkMode_*" 这组主题名**没有文档**, 是资源管理器自己在用的。好处是纯数据 (msstyles 里的类名),
/// 不是未文档化的**导出序号**那种玩法 —— 系统认不出来就退回默认样式, 不报错不崩。
pub fn dark_theme(h: HWND, dark: bool) {
    let name = if dark { windows::core::w!("DarkMode_Explorer") } else { windows::core::w!("Explorer") };
    let _ = unsafe { windows::Win32::UI::Controls::SetWindowTheme(h, name, None) };
}

/// 同上但指定暗色主题名 (输入框/下拉框用 "DarkMode_CFD")。浅色时清回默认 ——
/// 这些控件本来就没被我们设过主题, 给它们套个非默认的会改掉现有的浅色外观
pub fn dark_theme_named(h: HWND, dark: bool, name: PCWSTR) {
    let _ = unsafe {
        if dark {
            windows::Win32::UI::Controls::SetWindowTheme(h, name, None)
        } else {
            windows::Win32::UI::Controls::SetWindowTheme(h, PCWSTR::null(), PCWSTR::null())
        }
    };
}

/// 同上, 给列头 (Header) 用。列头是系统主题画的, 我们只在它底下补了一条分界线,
/// 不换主题名的话暗色列表上会顶着一条浅灰表头。浅色时**清回默认** —— 现在就没给它设过主题,
/// 设成 "ItemsView" 会改掉已有的浅色外观
pub fn dark_theme_items(h: HWND, dark: bool) {
    let _ = unsafe {
        if dark {
            windows::Win32::UI::Controls::SetWindowTheme(h, windows::core::w!("DarkMode_ItemsView"), None)
        } else {
            windows::Win32::UI::Controls::SetWindowTheme(h, PCWSTR::null(), PCWSTR::null())
        }
    };
}

/// 图标字体: Segoe Fluent Icons (Win11) / Segoe MDL2 Assets (Win10), 系统自带, 字符即图标, 零资源。
/// 图标字符表见 https://learn.microsoft.com/windows/apps/design/style/segoe-fluent-icons-font
pub fn icon_font(px: i32) -> HFONT {
    icon_font_q(px, CLEARTYPE_QUALITY)
}

/// 字体的实测行高 (px)。界面各处的尺寸都按它算, 不写死像素 (进程是 DPI 感知的)
/// 一段文字用这个字体画出来有多宽 (px)。列宽写死像素在高 DPI / 换字体之后就不对了, 该量就量
pub fn text_width(font: HFONT, s: &str) -> i32 {
    unsafe {
        let hdc = CreateCompatibleDC(None);
        let old = SelectObject(hdc, font.into());
        let w: Vec<u16> = s.encode_utf16().collect();
        let mut size = windows::Win32::Foundation::SIZE::default();
        let _ = windows::Win32::Graphics::Gdi::GetTextExtentPoint32W(hdc, &w, &mut size);
        SelectObject(hdc, old);
        let _ = DeleteDC(hdc);
        size.cx
    }
}

pub fn text_height(font: HFONT) -> i32 {
    unsafe {
        let hdc = CreateCompatibleDC(None);
        let old = SelectObject(hdc, font.into());
        let mut tm = TEXTMETRICW::default();
        let _ = GetTextMetricsW(hdc, &mut tm);
        SelectObject(hdc, old);
        let _ = DeleteDC(hdc);
        tm.tmHeight.max(12)
    }
}

/// 图标字体字符 → size×size 的 32 位预乘 alpha 位图: 白字黑底画一遍, 覆盖率当 alpha 再乘上目标色。
/// 塞进 HIMAGELIST 给原生控件 (工具栏/盘符栏) 用, 比自绘更省事 —— 悬停/按下/灰化全交给 comctl32。
///
/// # Safety
/// 在 UI 线程调; 返回的位图归调用方 DeleteObject。
pub unsafe fn render_glyph(ch: u16, size: i32, font: HFONT, color: COLORREF) -> HBITMAP {
    unsafe { render_text(&[ch], size, size, font, color) }
}

/// 同上, 任意文字、任意宽高 (菜单项左侧的编号位图用它, 普通 UI 字体也行)
///
/// # Safety
/// 在 UI 线程调; 返回的位图归调用方 DeleteObject。
pub unsafe fn render_text(text: &[u16], w: i32, h: i32, font: HFONT, color: COLORREF) -> HBITMAP {
    unsafe {
        let size = w; // 下面按 w×h 建位图, 变量名沿用
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            bmiColors: [Default::default(); 1],
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let hdc = CreateCompatibleDC(None);
        let bmp = CreateDIBSection(Some(hdc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).expect("glyph dib");
        let old_bmp = SelectObject(hdc, bmp.into());
        let _ = PatBlt(hdc, 0, 0, w, h, BLACKNESS);
        let old_font = SelectObject(hdc, font.into());
        SetTextColor(hdc, COLORREF(0x00FFFFFF));
        SetBkMode(hdc, TRANSPARENT);
        let mut rc = RECT { left: 0, top: 0, right: w, bottom: h };
        let mut s: Vec<u16> = text.iter().copied().filter(|&c| c != 0).collect();
        DrawTextW(hdc, &mut s, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
        let _ = GdiFlush();
        SelectObject(hdc, old_font);
        SelectObject(hdc, old_bmp);
        let _ = DeleteDC(hdc);
        let (r, g, b) = (color.0 & 0xFF, (color.0 >> 8) & 0xFF, (color.0 >> 16) & 0xFF);
        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (w * h) as usize);
        for p in px {
            let a = (*p & 0xFF).max((*p >> 8) & 0xFF).max((*p >> 16) & 0xFF);
            *p = (a << 24) | ((r * a / 255) << 16) | ((g * a / 255) << 8) | (b * a / 255);
        }
        bmp
    }
}

/// render_text / render_glyph 出的 32 位 DIB 左右翻过来 (图标字体里只有一个方向的, 比如旋转), 原地改, 原样返回
///
/// # Safety
/// 在 UI 线程调; 返回的位图归调用方 DeleteObject。
pub unsafe fn mirror_h(bmp: HBITMAP) -> HBITMAP {
    unsafe {
        let mut bm = BITMAP::default();
        GetObjectW(bmp.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut core::ffi::c_void));
        if !bm.bmBits.is_null() && bm.bmBitsPixel == 32 {
            let w = bm.bmWidth as usize;
            let px = std::slice::from_raw_parts_mut(bm.bmBits as *mut u32, w * bm.bmHeight.unsigned_abs() as usize);
            for row in px.chunks_mut(w) {
                row.reverse();
            }
        }
        bmp
    }
}

/// IDEA 的「软换行」图标 (intellij-community platform/icons/src/actions/toggleSoftWrap.svg, Apache 2.0),
/// 图标字体里没有这个形状, 按原稿坐标直接算。px = 图标边长 (原稿 16 单位缩到这么大), 在 size 见方的位图里居中; 出跟 render_text 同格式的预乘位图
///
/// # Safety
/// 在 UI 线程调; 返回的位图归调用方 DeleteObject。
pub unsafe fn soft_wrap_icon(px: i32, size: i32, color: COLORREF) -> HBITMAP {
    let alpha = soft_wrap_alpha(px, size);
    unsafe {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            bmiColors: [Default::default(); 1],
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bmp = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).expect("icon dib");
        let (r, g, b) = (color.0 & 0xFF, (color.0 >> 8) & 0xFF, (color.0 >> 16) & 0xFF);
        let out = std::slice::from_raw_parts_mut(bits as *mut u32, alpha.len());
        for (p, &a) in out.iter_mut().zip(&alpha) {
            let a = a as u32;
            *p = (a << 24) | ((r * a / 255) << 16) | ((g * a / 255) << 8) | (b * a / 255);
        }
        bmp
    }
}

/// soft_wrap_icon 的覆盖率 (0..=255, 逐行, size × size): 每像素 4×4 个采样点落在形状里的比例, 边缘自然抗锯齿
fn soft_wrap_alpha(px: i32, size: i32) -> Vec<u8> {
    // 原稿 (16×16): 三行横条 (长 / 中 / 短 + 一小截), 中间那行右端弯成半圆折回来, 三角箭头朝左接在第三行
    let inside = |x: f32, y: f32| {
        let rect = |l: f32, t: f32, w: f32, h: f32| x >= l && x < l + w && y >= t && y < t + h;
        let d = ((x - 11.5).powi(2) + (y - 10.5).powi(2)).sqrt();
        rect(2.0, 2.0, 12.0, 2.0)
            || rect(2.0, 7.0, 9.5, 2.0)
            || rect(2.0, 12.0, 3.0, 2.0)
            || rect(10.0, 12.0, 1.5, 2.0)
            || (x <= 10.0 && (y - 13.0).abs() <= x - 7.0)
            || (x >= 11.5 && (1.5..=3.5).contains(&d))
    };
    let s = px as f32 / 16.0;
    let off = (size - px) as f32 / 2.0;
    let mut out = Vec::with_capacity((size * size) as usize);
    for y in 0..size {
        for x in 0..size {
            let n = (0..16)
                .filter(|k| {
                    let sx = x as f32 + (k % 4) as f32 * 0.25 + 0.125;
                    let sy = y as f32 + (k / 4) as f32 * 0.25 + 0.125;
                    inside((sx - off) / s, (sy - off) / s)
                })
                .count();
            out.push((n * 255 / 16) as u8);
        }
    }
    out
}

#[test]
fn soft_wrap_shape() {
    // 16px 不缩放: 一个像素 = 原稿一个单位
    let a = soft_wrap_alpha(16, 16);
    let at = |x: usize, y: usize| a[y * 16 + x];
    assert_eq!(at(7, 2), 255); // 第一行横条
    assert_eq!(at(7, 5), 0); // 两行之间空着
    assert_eq!(at(14, 10), 255); // 右边的弯
    assert_eq!(at(12, 10), 0); // 弯的里面是空的
    assert_eq!(at(9, 13), 255); // 箭头
    assert_eq!(at(6, 13), 0); // 箭头尖左边、短横条右边的空当
}

/// 同上, 可指定抗锯齿方式 (渲染成位图取 alpha 时要灰度抗锯齿, ClearType 的彩边会串进颜色)
pub fn icon_font_q(px: i32, quality: FONT_QUALITY) -> HFONT {
    unsafe {
        let mut lf = LOGFONTW { lfHeight: -px, lfCharSet: DEFAULT_CHARSET, lfQuality: quality, ..Default::default() };
        let name = wide("Segoe Fluent Icons");
        lf.lfFaceName[..name.len()].copy_from_slice(&name);
        let f = CreateFontIndirectW(&lf);
        // Win10 没有 Fluent Icons, 回退 MDL2 (两者图标码位基本兼容)
        let hdc = CreateCompatibleDC(None);
        let old = SelectObject(hdc, f.into());
        let mut got = [0u16; 32];
        let n = GetTextFaceW(hdc, Some(&mut got)) as usize;
        SelectObject(hdc, old);
        let _ = DeleteDC(hdc);
        let got = String::from_utf16_lossy(&got[..n.saturating_sub(1)]);
        if got.starts_with("Segoe Fluent") {
            return f;
        }
        let _ = DeleteObject(f.into());
        let name = wide("Segoe MDL2 Assets");
        lf.lfFaceName = [0; 32];
        lf.lfFaceName[..name.len()].copy_from_slice(&name);
        CreateFontIndirectW(&lf)
    }
}

/// "#RRGGBB" → COLORREF (0x00BBGGRR); 格式不对返回 None
pub fn parse_color(s: &str) -> Option<COLORREF> {
    let hex = s.trim().strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some(COLORREF((v >> 16) | (v & 0xFF00) | ((v & 0xFF) << 16)))
}

/// 把 UTF-16 文本拷进 Win32 给的 (pszText, cchTextMax) 缓冲, 截断并补 0。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // obcmd 里一堆调用点都当安全函数用, 指针由调用方保证
pub fn copy_text(dst: *mut u16, cap: i32, src: &[u16]) {
    if dst.is_null() || cap <= 0 {
        return;
    }
    let n = src.len().min(cap as usize - 1);
    unsafe {
        std::ptr::copy_nonoverlapping(src.as_ptr(), dst, n);
        *dst.add(n) = 0;
    }
}

pub fn client_rect(hwnd: HWND) -> RECT {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    rc
}

pub fn move_window(hwnd: HWND, x: i32, y: i32, w: i32, h: i32) {
    unsafe {
        let _ = MoveWindow(hwnd, x, y, w, h, true);
    }
}

pub fn show(hwnd: HWND, visible: bool) {
    unsafe {
        let _ = ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE });
    }
}

pub fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}

/// 重画但**不擦背景**。自绘控件 (标签栏/盘符栏/面包屑) 的 WM_PAINT 本来就把自己那块画满,
/// 先擦一遍背景只是白白多一次底色闪 —— 切换活动面板时左右两条标签栏一起"闪一下"就是这么来的。
pub fn invalidate_keep(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

pub fn loword(v: isize) -> i32 {
    (v & 0xFFFF) as i16 as i32
}

pub fn hiword(v: isize) -> i32 {
    ((v >> 16) & 0xFFFF) as i16 as i32
}

/// 程序图标资源号 (build.rs 写进 .rc 的 `1 ICON`)。用 obui 的程序都照这个号放图标
pub const ICON_ID: u16 = 1;

/// 程序图标 (exe 里的 ICON_ID), small = 标题栏 / 任务栏小图标的尺寸
pub fn app_icon(hinst: HINSTANCE, small: bool) -> HICON {
    let cx = unsafe { GetSystemMetrics(if small { SM_CXSMICON } else { SM_CXICON }) };
    unsafe { LoadImageW(Some(hinst), PCWSTR(ICON_ID as usize as *const u16), IMAGE_ICON, cx, cx, LR_DEFAULTCOLOR).map(|h| HICON(h.0)).unwrap_or_default() }
}

/// 给窗口挂上程序图标 (标题栏左上角 + Alt+Tab)。DLGMODALFRAME 的窗口光给窗口类图标标题栏不画, 得 WM_SETICON。
/// 两个尺寸各取一次留着 (LoadImageW 每次都新建句柄, 一个弹窗泄一对太难看)
pub fn set_window_icon(hwnd: HWND) {
    static ICONS: std::sync::OnceLock<(isize, isize)> = std::sync::OnceLock::new();
    let (small, big) = *ICONS.get_or_init(|| {
        let hinst = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }.unwrap_or_default().into();
        (app_icon(hinst, true).0 as isize, app_icon(hinst, false).0 as isize)
    });
    if small != 0 {
        send(hwnd, WM_SETICON, ICON_SMALL as usize, small);
        send(hwnd, WM_SETICON, ICON_BIG as usize, big);
    }
}

/// 自建窗口建好之后调一次: 程序图标 + 标题栏 + 窗口里每一个子控件都按 dark 装扮; bg / fg = 列表的底色和字色。
///
/// 为什么要按类名逐个换主题: 下拉框 / 复选框 / 按钮这些**有视觉主题的系统控件根本不吃
/// WM_CTLCOLOR\***, 主题背景直接盖过去。只能换成资源管理器自己那套暗色主题类名
/// (`DarkMode_CFD` 给输入框和下拉框, `DarkMode_Explorer` 给按钮和列表)。
/// 这组名字没有文档, 但认不出来时系统退回默认样式, 不报错不崩 (实测过)。
pub fn dress(hwnd: HWND, dark: bool, bg: COLORREF, fg: COLORREF) {
    use windows::Win32::UI::Controls::{LVM_GETHEADER, LVM_SETBKCOLOR, LVM_SETTEXTBKCOLOR, LVM_SETTEXTCOLOR};
    // 左上角的程序图标: 所有自建弹窗统一一个样子 (浅色也要, 所以放在下面那个 return 前面)
    set_window_icon(hwnd);
    // **浅色下一个字都不碰。** 这些窗口是开的时候建、关的时候销毁, 没有"上一次设过的暗色"要还原;
    // 而给本来没设过主题的控件去 SetWindowTheme(NULL) 是会出事的 —— 实测把批量改名框画残了
    if !dark {
        return;
    }
    title_bar_dark(hwnd, true);
    struct Ctx {
        bg: COLORREF,
        fg: COLORREF,
    }
    unsafe extern "system" fn child(h: HWND, lp: LPARAM) -> windows::core::BOOL {
        let cx = unsafe { &*(lp.0 as *const Ctx) };
        let mut buf = [0u16; 64];
        let n = unsafe { GetClassNameW(h, &mut buf) } as usize;
        let cls = String::from_utf16_lossy(&buf[..n]);
        match cls.to_ascii_lowercase().as_str() {
            // 输入框和下拉框: CFD = Common File Dialog, 资源管理器的文件对话框用的就是这套
            "edit" | "combobox" | "richedit50w" => dark_theme_named(h, true, windows::core::w!("DarkMode_CFD")),
            // 按钮 / 复选框 / 单选
            "button" => dark_theme(h, true),
            "syslistview32" | "systreeview32" => {
                send(h, LVM_SETBKCOLOR, 0, cx.bg.0 as isize);
                send(h, LVM_SETTEXTBKCOLOR, 0, cx.bg.0 as isize);
                send(h, LVM_SETTEXTCOLOR, 0, cx.fg.0 as isize);
                dark_theme(h, true);
                let hd = HWND(send(h, LVM_GETHEADER, 0, 0) as *mut _);
                if !hd.0.is_null() {
                    dark_theme_items(hd, true);
                }
            }
            _ => {}
        }
        true.into()
    }
    let cx = Ctx { bg, fg };
    let _ = unsafe { EnumChildWindows(Some(hwnd), Some(child), LPARAM(&cx as *const Ctx as isize)) };
}

/// Repaint a ListView header in dark mode: background, each column's text (right-aligned columns stay
/// right-aligned), separators and sort arrows. Needed because "DarkMode_ItemsView" darkens the header's
/// background but the text stays COLOR_WINDOWTEXT black — invisible. Call after the default paint
/// (WM_PAINT with GetDC, or WM_PRINTCLIENT's hdc so PrintWindow screenshots match the screen).
/// Same drawing as ObCommander's panel header.
pub fn paint_header_dark(hwnd: HWND, hdc: HDC, bg: COLORREF, fg: COLORREF) {
    use windows::Win32::UI::Controls::{HDF_RIGHT, HDF_SORTDOWN, HDF_SORTUP, HDI_FORMAT, HDI_TEXT, HDITEMW, HDM_GETITEMCOUNT, HDM_GETITEMRECT, HDM_GETITEMW};
    let rc = client_rect(hwnd);
    unsafe {
        fill(hdc, &rc, bg);
        let font = HFONT(send(hwnd, WM_GETFONT, 0, 0) as *mut _);
        let old = if font.is_invalid() { HGDIOBJ::default() } else { SelectObject(hdc, font.into()) };
        SetBkMode(hdc, TRANSPARENT);
        let sep = blend(bg, fg, 0.30);
        let n = send(hwnd, HDM_GETITEMCOUNT, 0, 0);
        for i in 0..n.max(0) {
            let mut ir = RECT::default();
            if send(hwnd, HDM_GETITEMRECT, i as usize, &mut ir as *mut _ as isize) == 0 {
                continue;
            }
            let mut buf = [0u16; 128];
            let mut it = HDITEMW { mask: HDI_TEXT | HDI_FORMAT, pszText: windows::core::PWSTR(buf.as_mut_ptr()), cchTextMax: buf.len() as i32, ..Default::default() };
            if send(hwnd, HDM_GETITEMW, i as usize, &mut it as *mut _ as isize) == 0 {
                continue;
            }
            let len = buf.iter().position(|&c| c == 0).unwrap_or(0);
            let right = it.fmt.0 & HDF_RIGHT.0 != 0;
            let mut tr = RECT { left: ir.left + 6, right: ir.right - 6, ..ir };
            SetTextColor(hdc, fg);
            let flags = DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX | if right { DT_RIGHT } else { DT_LEFT };
            DrawTextW(hdc, &mut buf[..len], &mut tr, flags);
            if it.fmt.0 & (HDF_SORTUP.0 | HDF_SORTDOWN.0) != 0 {
                let up = it.fmt.0 & HDF_SORTUP.0 != 0;
                let cx = (ir.left + ir.right) / 2;
                let y = ir.top + 2;
                let tri = if up { [POINT { x: cx - 3, y: y + 3 }, POINT { x: cx + 3, y: y + 3 }, POINT { x: cx, y }] } else { [POINT { x: cx - 3, y }, POINT { x: cx + 3, y }, POINT { x: cx, y: y + 3 }] };
                let (br, pen) = (CreateSolidBrush(fg), CreatePen(PS_SOLID, 1, fg));
                let (ob, op) = (SelectObject(hdc, br.into()), SelectObject(hdc, pen.into()));
                let _ = Polygon(hdc, &tri);
                SelectObject(hdc, ob);
                SelectObject(hdc, op);
                let _ = DeleteObject(br.into());
                let _ = DeleteObject(pen.into());
            }
            if i + 1 < n {
                fill(hdc, &RECT { left: ir.right - 1, top: ir.top + 3, right: ir.right, bottom: ir.bottom - 3 }, sep);
            }
        }
        if !old.is_invalid() {
            SelectObject(hdc, old);
        }
    }
}
