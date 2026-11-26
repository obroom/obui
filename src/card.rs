//! 卡片式自绘界面 (Win11 设置那个样子): 左边导航, 右边一张张圆角卡片 `[标签 + 灰字说明 | 控件]`。
//! 从 ObCommander 的设置窗口抽出来, obcmd 和 ObCleaner 共用。
//!
//! 用法: 每次重画先排版出一张 [`Frame`] (圆角块 / 文字 / 图标 + 可点区域), 再交给 [`render`] 画:
//! 圆角块走 Direct2D (抗锯齿, GDI 的 RoundRect 边缘是锯齿), 文字用 GDI 画在上面, 整张先画进内存位图再贴 (不闪)。
//! 点击 / 悬停按上一次排版的可点区域查 ([`Frame::hit`])。
//! 尺寸全按字高 `th` 算, 不写死像素; 颜色全从 [`Pal`] 来, 换主题跟着变。库里不出界面文案。

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::{HIMAGELIST, ILD_TRANSPARENT, ImageList_Draw};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::win::*;

// ───────────── 配色 ─────────────

/// 卡片界面的几样颜色, 全从程序的配色推出来: 卡片取 (列表底, chrome) 里亮的那个, 窗口底取暗的那个
/// —— 浅色下是灰底白卡片, 深色下是黑底深灰卡片, 跟 Win11 设置一个样子
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pal {
    pub win: COLORREF,
    pub card: COLORREF,
    pub fg: COLORREF,
    pub dim: COLORREF,
    pub line: COLORREF,
    pub field: COLORREF,
    pub hover: COLORREF,
    pub press: COLORREF,
    pub accent: COLORREF,
}

impl Pal {
    /// bg / fg = 列表底和字, chrome / chrome_fg = 工具栏底和字, accent = 强调色 (开关、选中的竖条)
    pub fn new(bg: COLORREF, fg: COLORREF, chrome: COLORREF, chrome_fg: COLORREF, accent: COLORREF) -> Pal {
        let (win, card, fg) = if luminance(bg) >= luminance(chrome) { (chrome, bg, fg) } else { (bg, chrome, chrome_fg) };
        Pal {
            win,
            card,
            fg,
            dim: blend(fg, card, 0.45),
            line: blend(fg, card, 0.86),
            field: blend(fg, card, 0.96),
            hover: blend(fg, win, 0.93),
            press: blend(fg, win, 0.88),
            accent,
        }
    }

    /// 深浅两套内置配色 (没有自己主题系统的程序用): 浅 = Win11 浅灰底白卡片, 深 = 黑底深灰卡片
    pub fn system(dark: bool, accent: COLORREF) -> Pal {
        if dark {
            Pal::new(COLORREF(0x00202020), COLORREF(0x00F0F0F0), COLORREF(0x002B2B2B), COLORREF(0x00F0F0F0), accent)
        } else {
            Pal::new(COLORREF(0x00FFFFFF), COLORREF(0x001A1A1A), COLORREF(0x00F3F3F3), COLORREF(0x001A1A1A), accent)
        }
    }

    pub fn is_dark(&self) -> bool {
        is_dark(self.card)
    }
}

// ───────────── 字体 ─────────────

/// 系统消息字体换个字号 / 粗细。pt10 = 十分之一磅 (105 = 10.5pt)
pub fn msg_font(pt10: i32, weight: i32) -> HFONT {
    unsafe {
        let mut ncm = NONCLIENTMETRICSW { cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
        let _ = SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, ncm.cbSize, Some(&mut ncm as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        let mut lf = ncm.lfMessageFont;
        let dpi = windows::Win32::UI::HiDpi::GetDpiForSystem() as i32;
        lf.lfHeight = -(pt10 * dpi / 720);
        lf.lfWeight = weight;
        CreateFontIndirectW(&lf)
    }
}

/// 卡片界面用的一套字体。th / sth = 正文 / 说明的字高: 所有尺寸按它们算
pub struct Fonts {
    pub body: HFONT,
    pub small: HFONT,
    pub strong: HFONT,
    pub title: HFONT,
    pub icon: HFONT,
    pub icon_small: HFONT,
    pub th: i32,
    pub sth: i32,
}

impl Fonts {
    pub fn new() -> Fonts {
        let body = msg_font(105, 400);
        let th = text_height(body);
        let small = msg_font(90, 400);
        Fonts {
            body,
            small,
            strong: msg_font(105, 600),
            title: msg_font(200, 600),
            icon: icon_font(th * 4 / 5),
            icon_small: icon_font(th * 2 / 3),
            th,
            sth: text_height(small),
        }
    }
}

impl Default for Fonts {
    fn default() -> Self {
        Fonts::new()
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        for f in [self.body, self.small, self.strong, self.title, self.icon, self.icon_small] {
            unsafe {
                let _ = DeleteObject(f.into());
            }
        }
    }
}

// ───────────── 显示列表 ─────────────

pub enum Op {
    /// 圆角块: 填色 / 描边 (线宽)
    Box { rc: RECT, r: f32, fill: Option<COLORREF>, line: Option<(COLORREF, f32)> },
    Text { rc: RECT, s: String, font: HFONT, col: COLORREF, fmt: DRAW_TEXT_FORMAT },
    /// 图像列表里的一个小图标, 左上角在 (x, y)
    Icon { x: i32, y: i32, list: HIMAGELIST, idx: i32 },
}

/// 一次排版的结果: 要画的东西 + 能点的区域 (按排版顺序, Tab 也按这个顺序走)。
/// A = 程序自己的"点了做什么"; hot / down = 现在悬停 / 按住的那个 (画悬停底色用)
pub struct Frame<A> {
    pub ops: Vec<Op>,
    pub hits: Vec<(RECT, A)>,
    /// 内容总高 (滚动用)
    pub content_h: i32,
    /// 滚动条滑块 (能拖), 没有滚动条 = None
    pub thumb: Option<RECT>,
    pub hot: Option<A>,
    pub down: Option<A>,
}

impl<A: Copy + PartialEq> Frame<A> {
    pub fn new(hot: Option<A>, down: Option<A>) -> Self {
        Frame { ops: Vec::new(), hits: Vec::new(), content_h: 0, thumb: None, hot, down }
    }
    pub fn rbox(&mut self, rc: RECT, r: f32, fill: Option<COLORREF>, line: Option<COLORREF>) {
        self.ops.push(Op::Box { rc, r, fill, line: line.map(|c| (c, 1.0)) });
    }
    pub fn text(&mut self, rc: RECT, s: &str, font: HFONT, col: COLORREF, fmt: DRAW_TEXT_FORMAT) {
        if !s.is_empty() {
            self.ops.push(Op::Text { rc, s: s.to_string(), font, col, fmt });
        }
    }
    /// 图标字体 (Segoe Fluent Icons) 里的一个字, 在 rc 里居中
    pub fn glyph(&mut self, rc: RECT, ch: u16, font: HFONT, col: COLORREF) {
        self.text(rc, &String::from_utf16_lossy(&[ch]), font, col, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    }
    pub fn hit(&mut self, rc: RECT, a: A) {
        self.hits.push((rc, a));
    }
    pub fn is_hot(&self, a: A) -> bool {
        self.hot == Some(a)
    }
    /// 按住且鼠标还在它上面
    pub fn is_pressed(&self, a: A) -> bool {
        self.down == Some(a) && self.hot == Some(a)
    }
    /// 按状态挑底色: 按下 > 悬停 > 平时
    pub fn state_fill(&self, a: A, base: Option<COLORREF>, hover: COLORREF, press: COLORREF) -> Option<COLORREF> {
        if self.is_pressed(a) {
            Some(press)
        } else if self.is_hot(a) {
            Some(hover)
        } else {
            base
        }
    }
}

/// 在可点区域表里找 (x, y) 落在哪个上 (后排上去的盖在上面, 所以倒着找)
pub fn hit_at<A: Copy>(hits: &[(RECT, A)], x: i32, y: i32) -> Option<A> {
    hits.iter().rev().find(|(rc, _)| inside(rc, x, y)).map(|(_, a)| *a)
}

pub const LEFT_MID: DRAW_TEXT_FORMAT = DRAW_TEXT_FORMAT(DT_LEFT.0 | DT_VCENTER.0 | DT_SINGLELINE.0 | DT_NOPREFIX.0 | DT_END_ELLIPSIS.0);
pub const RIGHT_MID: DRAW_TEXT_FORMAT = DRAW_TEXT_FORMAT(DT_RIGHT.0 | DT_VCENTER.0 | DT_SINGLELINE.0 | DT_NOPREFIX.0);
pub const CENTER_MID: DRAW_TEXT_FORMAT = DRAW_TEXT_FORMAT(DT_CENTER.0 | DT_VCENTER.0 | DT_SINGLELINE.0 | DT_NOPREFIX.0 | DT_END_ELLIPSIS.0);
/// 多行正文 (自动折行)
pub const WRAP: DRAW_TEXT_FORMAT = DRAW_TEXT_FORMAT(DT_LEFT.0 | DT_WORDBREAK.0 | DT_NOPREFIX.0);

pub fn rect(l: i32, t: i32, r: i32, b: i32) -> RECT {
    RECT { left: l, top: t, right: r, bottom: b }
}

pub fn inside(rc: &RECT, x: i32, y: i32) -> bool {
    x >= rc.left && x < rc.right && y >= rc.top && y < rc.bottom
}

// ───────────── 控件 ─────────────

/// 左边导航的一项: 圆角底 (选中 / 悬停), 选中时左边一根强调色小竖条, 图标 + 名字
#[allow(clippy::too_many_arguments)]
pub fn nav_item<A: Copy + PartialEq>(f: &mut Frame<A>, pal: &Pal, fonts: &Fonts, rc: RECT, icon: u16, label: &str, col: COLORREF, a: A, selected: bool) {
    let th = fonts.th;
    let item_h = rc.bottom - rc.top;
    f.rbox(rc, th as f32 * 0.3, f.state_fill(a, selected.then_some(pal.hover), if selected { pal.press } else { pal.hover }, pal.press), None);
    if selected {
        let ph = th * 9 / 10;
        f.rbox(rect(rc.left + 1, rc.top + (item_h - ph) / 2, rc.left + 4, rc.top + (item_h + ph) / 2), 1.5, Some(pal.accent), None);
    }
    f.glyph(rect(rc.left + th / 2, rc.top, rc.left + th * 2, rc.bottom), icon, fonts.icon, col);
    f.text(rect(rc.left + th * 5 / 2, rc.top, rc.right - th / 3, rc.bottom), label, fonts.body, col, LEFT_MID);
    f.hit(rc, a);
}

/// 普通按钮 (描边圆角, 文字居中)。primary = 强调色底 (一页里最主要的那个动作)。enabled = false 画灰不能点
#[allow(clippy::too_many_arguments)]
pub fn button<A: Copy + PartialEq>(f: &mut Frame<A>, pal: &Pal, fonts: &Fonts, rc: RECT, label: &str, a: A, primary: bool, enabled: bool) {
    let th = fonts.th;
    if !enabled {
        f.rbox(rc, th as f32 * 0.25, Some(pal.card), Some(pal.line));
        f.text(rc, label, fonts.body, pal.line, CENTER_MID);
        return;
    }
    if primary {
        // blend 的 t = 往第二个颜色走多少: 悬停只淡一成 (Win11 强调按钮的做法), 按下两成; 字一直对强调色取反差
        let fill = if f.is_pressed(a) { blend(pal.accent, pal.card, 0.2) } else if f.is_hot(a) { blend(pal.accent, pal.card, 0.1) } else { pal.accent };
        f.rbox(rc, th as f32 * 0.25, Some(fill), None);
        f.text(rc, label, fonts.body, contrast_text(pal.accent), CENTER_MID);
    } else {
        f.rbox(rc, th as f32 * 0.25, f.state_fill(a, Some(pal.card), blend(pal.fg, pal.card, 0.93), blend(pal.fg, pal.card, 0.88)), Some(pal.line));
        f.text(rc, label, fonts.body, pal.fg, CENTER_MID);
    }
    f.hit(rc, a);
}

/// 按钮该多宽: 文字宽 + 左右各一个字高
pub fn button_w(fonts: &Fonts, label: &str) -> i32 {
    text_width(fonts.body, label) + fonts.th * 2
}

/// 输入框类控件 (下拉 / 选色 / 选字体) 的底: 平时 field 色, 悬停 / 按下加深
pub fn field<A: Copy + PartialEq>(f: &mut Frame<A>, pal: &Pal, th: i32, b: RECT, a: A) {
    let fill = if f.is_pressed(a) {
        blend(pal.fg, pal.card, 0.90)
    } else if f.is_hot(a) {
        blend(pal.fg, pal.card, 0.93)
    } else {
        pal.field
    };
    f.rbox(b, th as f32 * 0.25, Some(fill), Some(pal.line));
    f.hit(b, a);
}

/// 开关 (右边缘 right, 竖直居中 cy), 返回轨道的矩形。开 = 强调色底 + 圆点在右, 关 = 描边 + 灰圆点在左, 悬停圆点大一圈。
/// 不进可点区域表: 调用方连旁边的「开 / 关」字一起算一块
#[allow(clippy::too_many_arguments)]
pub fn toggle<A: Copy + PartialEq>(f: &mut Frame<A>, pal: &Pal, th: i32, right: i32, cy: i32, on: bool, a: A) -> RECT {
    let (tw, tht) = (th * 2, th * 21 / 20);
    let track = rect(right - tw, cy - tht / 2, right, cy + tht / 2);
    let hover = f.is_hot(a);
    let knob = tht * if hover { 7 } else { 6 } / 10;
    let dot = |f: &mut Frame<A>, kx: i32, col: COLORREF| f.rbox(rect(kx - knob / 2, cy - knob / 2, kx + knob / 2, cy + knob / 2), knob as f32 / 2.0, Some(col), None);
    if on {
        f.rbox(track, tht as f32 / 2.0, Some(pal.accent), None);
        dot(f, track.right - tht / 2, contrast_text(pal.accent));
    } else {
        f.ops.push(Op::Box { rc: track, r: tht as f32 / 2.0, fill: hover.then_some(pal.field), line: Some((pal.dim, 1.0)) });
        dot(f, track.left + tht / 2, pal.dim);
    }
    track
}

/// 勾选框 (方的, 左上角在 x, 竖直居中 cy), 返回框的矩形。勾上 = 强调色底 + 白勾; 不进可点区域表
pub fn checkbox<A: Copy + PartialEq>(f: &mut Frame<A>, pal: &Pal, fonts: &Fonts, x: i32, cy: i32, on: bool, a: A) -> RECT {
    let th = fonts.th;
    let sz = th * 9 / 10;
    let b = rect(x, cy - sz / 2, x + sz, cy + sz / 2);
    if on {
        f.rbox(b, th as f32 * 0.18, Some(if f.is_hot(a) { blend(pal.accent, pal.card, 0.1) } else { pal.accent }), None);
        f.glyph(b, 0xE73E, fonts.icon_small, contrast_text(pal.accent));
    } else {
        f.ops.push(Op::Box { rc: b, r: th as f32 * 0.18, fill: Some(if f.is_hot(a) { pal.field } else { pal.card }), line: Some((pal.dim, 1.0)) });
    }
    b
}

/// `− 数字 +` 框: 数字那格点了直接输入 (mid); minus / plus 是 None = 到头了, 画灰不能点。
/// 可点区域按 − 数字 + 的顺序进表 (Tab 也按这个顺序走)
#[allow(clippy::too_many_arguments)]
pub fn stepper<A: Copy + PartialEq>(f: &mut Frame<A>, pal: &Pal, fonts: &Fonts, b: RECT, text: &str, minus: Option<A>, plus: Option<A>, mid: A, editing: bool) {
    let th = fonts.th;
    f.rbox(b, th as f32 * 0.25, Some(pal.field), Some(pal.line));
    let bw = th * 3 / 2;
    let step = |f: &mut Frame<A>, a: Option<A>, rr: RECT, glyph: u16| {
        if let Some(a) = a {
            if f.is_hot(a) {
                let inner = rect(rr.left + 3, rr.top + 3, rr.right - 3, rr.bottom - 3);
                let fill = if f.down == Some(a) { pal.press } else { pal.hover };
                f.rbox(inner, th as f32 * 0.2, Some(fill), None);
            }
            f.hit(rr, a);
        }
        f.glyph(rr, glyph, fonts.icon_small, if a.is_some() { pal.fg } else { pal.line });
    };
    step(f, minus, rect(b.left, b.top, b.left + bw, b.bottom), 0xE738);
    let m = rect(b.left + bw, b.top, b.right - bw, b.bottom);
    if !editing {
        f.text(m, text, fonts.body, pal.fg, CENTER_MID);
    }
    f.hit(m, mid);
    step(f, plus, rect(b.right - bw, b.top, b.right, b.bottom), 0xE710);
}

/// 滚动条: 内容比窗口高才有, 细细一条贴右边, 悬停 / 拖动时 (wide) 加粗。滑块的可拖区域记进 f.thumb
pub fn scrollbar<A: Copy + PartialEq>(f: &mut Frame<A>, pal: &Pal, th: i32, cw: i32, ch: i32, scroll: i32, wide: bool) {
    if f.content_h <= ch {
        return;
    }
    let track = ch - th;
    let th_h = (track * ch / f.content_h).max(th * 2);
    let max = f.content_h - ch;
    let ty = th / 2 + (track - th_h) * scroll.clamp(0, max) / max.max(1);
    let bw = if wide { th / 3 } else { th / 6 };
    let rc = rect(cw - th / 2 - bw, ty, cw - th / 2, ty + th_h);
    f.rbox(rc, bw as f32 / 2.0, Some(blend(pal.fg, pal.win, 0.6)), None);
    f.thumb = Some(rect(cw - th, ty, cw, ty + th_h));
}

/// 键盘焦点框 (用过 Tab 才画, 鼠标点的不画, 跟系统控件一样)
pub fn focus_ring<A: Copy + PartialEq>(f: &mut Frame<A>, pal: &Pal, th: i32, focus: A) {
    if let Some((rc, _)) = f.hits.iter().find(|(_, a)| *a == focus) {
        let rc = rect(rc.left - 2, rc.top - 2, rc.right + 2, rc.bottom + 2);
        f.ops.push(Op::Box { rc, r: th as f32 * 0.3, fill: None, line: Some((pal.fg, 2.0)) });
    }
}

/// 弹出菜单: (id, 文字, 能不能点), id 0 = 分隔线; checked = 打勾的那个 id (0 = 不打)。
/// 弹在 anchor (屏幕坐标) 下面, right = 右对齐; 返回点中的 id (0 = 没点)
pub fn popup_menu(hwnd: HWND, anchor: RECT, right: bool, items: &[(usize, &str, bool)], checked: usize) -> usize {
    unsafe {
        let Ok(m) = CreatePopupMenu() else { return 0 };
        for &(id, text, on) in items {
            if id == 0 {
                let _ = AppendMenuW(m, MF_SEPARATOR, 0, None);
            } else {
                let mut flags = if on { MF_STRING } else { MF_STRING | MF_GRAYED };
                if id == checked {
                    flags |= MF_CHECKED;
                }
                let _ = AppendMenuW(m, flags, id, &windows::core::HSTRING::from(text));
            }
        }
        let align = if right { TPM_RIGHTALIGN } else { TPM_LEFTALIGN };
        let x = if right { anchor.right } else { anchor.left };
        let sel = TrackPopupMenuEx(m, (TPM_RETURNCMD | TPM_NONOTIFY | align | TPM_TOPALIGN | TPM_NOANIMATION).0, x, anchor.bottom + 2, hwnd, None).0 as usize;
        let _ = DestroyMenu(m);
        sel
    }
}

/// 客户区坐标的矩形 → 屏幕坐标 (弹菜单要)
pub fn to_screen(hwnd: HWND, rc: RECT) -> RECT {
    let mut p = POINT { x: rc.left, y: rc.top };
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut p);
    }
    rect(p.x, p.y, p.x + rc.right - rc.left, p.y + rc.bottom - rc.top)
}

// ───────────── 绘制 ─────────────

fn d2d_color(c: COLORREF) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: (c.0 & 0xFF) as f32 / 255.0, g: (c.0 >> 8 & 0xFF) as f32 / 255.0, b: (c.0 >> 16 & 0xFF) as f32 / 255.0, a: 1.0 }
}

fn render_target() -> Option<ID2D1DCRenderTarget> {
    unsafe {
        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()?;
        // dpi 96 = 一个单位就是一个像素 (排版全按像素算)。
        // SOFTWARE: 默认 (DEFAULT) 会建 D3D 设备, 把显卡驱动整个拉进进程 —— 实测专用内存 +54 MB、线程 +16,
        // 而这里只是往内存位图里画几十个圆角块, CPU 画绰绰有余 (软件版 +2 MB)
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
            dpiX: 96.0,
            dpiY: 96.0,
            ..Default::default()
        };
        factory.CreateDCRenderTarget(&props).ok()
    }
}

/// 把显示列表画到 hdc 的 rc 上: 先在内存位图里画好整张再贴上去 (不闪): 底色 bg → 圆角块 (D2D) → 图标 → 文字 (GDI)。
/// rt = 调用方留着的 D2D 渲染目标 (第一次传 None 进来, 这里建好放回去; 设备丢了会清成 None 下次重建)
pub fn render(hdc: HDC, rc: &RECT, ops: &[Op], bg: COLORREF, rt: &mut Option<ID2D1DCRenderTarget>) {
    let (cw, ch) = (rc.right - rc.left, rc.bottom - rc.top);
    if cw <= 0 || ch <= 0 {
        return;
    }
    unsafe {
        let mem = CreateCompatibleDC(Some(hdc));
        let bmp = CreateCompatibleBitmap(hdc, cw, ch);
        let old = SelectObject(mem, bmp.into());
        let local = rect(0, 0, cw, ch);
        fill(mem, &local, bg);
        if rt.is_none() {
            *rt = render_target();
        }
        if let Some(t) = rt.as_ref()
            && t.BindDC(mem, &local).is_ok()
        {
            t.BeginDraw();
            let mut brushes: Vec<(u32, ID2D1SolidColorBrush)> = Vec::new();
            let mut brush = |c: COLORREF| -> Option<ID2D1SolidColorBrush> {
                if let Some((_, b)) = brushes.iter().find(|(k, _)| *k == c.0) {
                    return Some(b.clone());
                }
                let b = t.CreateSolidColorBrush(&d2d_color(c), None).ok()?;
                brushes.push((c.0, b.clone()));
                Some(b)
            };
            for op in ops {
                if let Op::Box { rc, r, fill, line } = op {
                    let rr = |inset: f32| D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: rc.left as f32 + inset, top: rc.top as f32 + inset, right: rc.right as f32 - inset, bottom: rc.bottom as f32 - inset },
                        radiusX: *r,
                        radiusY: *r,
                    };
                    if let Some(b) = fill.and_then(&mut brush) {
                        t.FillRoundedRectangle(&rr(0.0), &b);
                    }
                    if let Some((c, w)) = line
                        && let Some(b) = brush(*c)
                    {
                        t.DrawRoundedRectangle(&rr(w / 2.0), &b, *w, None);
                    }
                }
            }
            if t.EndDraw(None, None).is_err() {
                *rt = None; // 设备丢了 (显卡驱动重置之类): 下次重建
            }
        }
        for op in ops {
            if let Op::Icon { x, y, list, idx } = op {
                let _ = ImageList_Draw(*list, *idx, mem, *x, *y, ILD_TRANSPARENT);
            }
        }
        SetBkMode(mem, TRANSPARENT);
        for op in ops {
            if let Op::Text { rc, s, font, col, fmt } = op {
                let old_f = SelectObject(mem, (*font).into());
                SetTextColor(mem, *col);
                let mut buf: Vec<u16> = s.encode_utf16().collect();
                let mut r = *rc;
                DrawTextW(mem, &mut buf, &mut r, *fmt);
                SelectObject(mem, old_f);
            }
        }
        let _ = BitBlt(hdc, rc.left, rc.top, cw, ch, Some(mem), 0, 0, SRCCOPY);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pal_picks_lighter_as_card() {
        // 浅色: 列表白底比工具栏亮 → 卡片白, 窗口灰
        let p = Pal::new(COLORREF(0xFFFFFF), COLORREF(0), COLORREF(0xF0F0F0), COLORREF(0), COLORREF(0x3C982B));
        assert_eq!((p.card, p.win), (COLORREF(0xFFFFFF), COLORREF(0xF0F0F0)));
        // 深色: 工具栏比列表亮 → 卡片取工具栏, 字也取工具栏的字
        let d = Pal::new(COLORREF(0x101010), COLORREF(0xEEEEEE), COLORREF(0x2B2B2B), COLORREF(0xDDDDDD), COLORREF(0x3C982B));
        assert_eq!((d.card, d.win, d.fg), (COLORREF(0x2B2B2B), COLORREF(0x101010), COLORREF(0xDDDDDD)));
        assert!(d.is_dark() && !p.is_dark());
    }

    #[test]
    fn hit_prefers_last_pushed() {
        let hits = [(rect(0, 0, 10, 10), 1), (rect(5, 5, 8, 8), 2)];
        assert_eq!(hit_at(&hits, 6, 6), Some(2));
        assert_eq!(hit_at(&hits, 1, 1), Some(1));
        assert_eq!(hit_at(&hits, 10, 10), None); // 右下边缘不算
    }
}
