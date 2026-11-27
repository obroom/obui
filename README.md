# obui

**English** · [简体中文](README.zh-CN.md)

Small Win32 UI parts in Rust, shared by [ObCommander](https://obcommander.com) and
[ObCleaner](https://github.com/obroom/obcleaner). Plain Win32 and GDI (Direct2D only for anti-aliased
rounded corners), no UI framework.

The library has no UI text of its own, reads no config and runs no message loop: strings, settings
and the message loop belong to the host app.

## Modules

| Module | What it gives you |
|--------|-------------------|
| `win` | Win32 helpers: wide strings, fonts (system message font, Segoe Fluent Icons with MDL2 fallback), colour maths, glyph-to-bitmap rendering, and dark mode: dark title bar, dark popup menus, dark scrollbars / edits / combo boxes / buttons, and `dress` to dark-theme a whole dialog in one call |
| `card` | Owner-drawn card UI in the style of the Windows 11 Settings app: left navigation, rounded cards, buttons, toggles, checkboxes, steppers, scrollbar, focus ring. Layout produces a display list (`Frame`) and `render` paints it double-buffered |
| `taskdlg` | `TaskDialog` wrappers: `dialog` with custom buttons and clickable links, and `msgbox`, a drop-in `MessageBoxW` replacement |
| `search_edit` | A ✕ clear button inside a single-line search box, like HTML `<input type="search">` |
| `cue` | Grey placeholder text in an empty edit box that stays while it has focus, like HTML `placeholder` |

## Usage

```toml
[dependencies]
obui = { git = "https://github.com/obroom/obui" }
```

```rust
use obui::{cue, search_edit, taskdlg, win};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONWARNING, MB_YESNO};
use windows::core::w;

/// Call once after creating a window that has a search box.
fn setup(dialog: HWND, search: HWND, dark: bool) {
    search_edit::attach(search); // ✕ button inside the box
    cue::set(search, "Search"); // placeholder, stays while focused
    // In dark mode, darken the title bar and every child control
    win::dress(dialog, dark, win::parse_color("#202020").unwrap(), win::parse_color("#F0F0F0").unwrap());
}

fn confirm_delete(owner: HWND) -> bool {
    // Same arguments and return values as MessageBoxW
    unsafe { taskdlg::msgbox(Some(owner), w!("Delete permanently?"), w!("My App"), MB_YESNO | MB_ICONWARNING) == IDYES }
}
```

The card UI is used by drawing a `card::Frame` on every repaint (`nav_item`, `button`, `toggle`, …),
painting it with `card::render`, and hit-testing mouse input against the same frame with
`card::hit_at`. Every size is derived from the font height and every colour from a `card::Pal`, so it
follows DPI and theme changes.

## Notes

- The app icon is loaded from resource ID 1 (`win::ICON_ID`); put your icon there in your `.rc` file.
  Like system dialogs, owned popups get no title bar icon; only unowned windows, which show up in the
  taskbar and Alt+Tab on their own, get one.
- Dark mode is decided by the background colour alone (`win::is_dark`); there is no separate switch.
- `win::set_app_dark_mode` (dark popup menus) calls `uxtheme.dll` by ordinal 135 / 136, which is
  undocumented. It is the same call Explorer and many other apps use, and it is skipped before
  Windows 10 1809. The `DarkMode_*` theme class names are undocumented too, but they fall back to the
  default look when unknown.

## Requirements

- Windows 10 or later (dark mode needs 1809 or later).
- Rust 1.88+ (edition 2024, let chains).

## License

[MIT](LICENSE)
