# obui

[English](README.md) · **简体中文**

用 Rust 写的一组 Win32 界面零件，[ObCommander](https://obcommander.com) 和
[ObCleaner](https://github.com/obroom/obcleaner) 共用。纯 Win32 加 GDI（只有抗锯齿圆角用 Direct2D），
不依赖任何界面框架。

库里不出界面文案、不读配置、不跑消息循环：文字、设置和消息循环都由宿主程序自己管。

## 模块

| 模块 | 提供什么 |
|------|---------|
| `win` | Win32 小工具：宽字符串、字体（系统消息字体、Segoe Fluent Icons，Win10 退回 MDL2）、配色计算、图标字转位图，以及暗色模式：暗色标题栏、暗色弹出菜单、暗色滚动条 / 输入框 / 下拉框 / 按钮，还有 `dress` 一次把整个对话框换成暗色 |
| `card` | Win11 设置风格的自绘卡片界面：左侧导航、圆角卡片、按钮、开关、复选框、数字加减框、滚动条、焦点框。排版产出一张显示列表（`Frame`），`render` 双缓冲画出来 |
| `taskdlg` | `TaskDialog` 的包装：`dialog` 支持自定义按钮和可点链接；`msgbox` 可以直接替换 `MessageBoxW`。两者的标题栏都带程序图标 |
| `search_edit` | 单行搜索框里的 ✕ 清除按钮，跟 HTML 的 `<input type="search">` 一样 |
| `cue` | 空输入框里的灰色占位提示，获得焦点后也不消失，跟 HTML 的 `placeholder` 一样 |

## 用法

```toml
[dependencies]
obui = { git = "https://github.com/obroom/obui" }
```

```rust
use obui::{cue, search_edit, taskdlg, win};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONWARNING, MB_YESNO};
use windows::core::w;

/// 建好带搜索框的窗口后调一次
fn setup(dialog: HWND, search: HWND, dark: bool) {
    search_edit::attach(search); // 框内的 ✕ 按钮
    cue::set(search, "搜索"); // 占位提示，有焦点也显示
    // 标题栏挂程序图标；暗色模式下标题栏和所有子控件一起变暗
    win::dress(dialog, dark, win::parse_color("#202020").unwrap(), win::parse_color("#F0F0F0").unwrap());
}

fn confirm_delete(owner: HWND) -> bool {
    // 参数和返回值跟 MessageBoxW 一样
    unsafe { taskdlg::msgbox(Some(owner), w!("确定永久删除吗？"), w!("我的程序"), MB_YESNO | MB_ICONWARNING) == IDYES }
}
```

卡片界面的用法：每次重画时排出一张 `card::Frame`（`nav_item`、`button`、`toggle` 等），交给
`card::render` 画出来，鼠标点击和悬停用 `card::hit_at` 在同一张 Frame 上查。所有尺寸都按字高算，
所有颜色都来自 `card::Pal`，所以 DPI 和主题变了会自动跟上。

## 说明

- 程序图标从资源号 1（`win::ICON_ID`）加载，在你的 `.rc` 文件里把图标放在这个号上。
- 是不是暗色只看底色（`win::is_dark`），没有单独的开关。
- `win::set_app_dark_mode`（暗色弹出菜单）按序号 135 / 136 调用 `uxtheme.dll`，这是没有文档的接口。
  资源管理器和很多程序用的都是它，Windows 10 1809 之前的系统会直接跳过。`DarkMode_*` 主题类名也没有
  文档，但系统认不出时会退回默认样式。

## 环境要求

- Windows 10 及以上（暗色模式需要 1809 及以上）。
- Rust 1.88 及以上（edition 2024，用到了 let chains）。

## 许可证

[MIT](LICENSE)
