//! obui: Obroom 几个 Windows 程序 (ObCommander、ObCleaner) 共用的界面零件, 纯 Win32。
//!
//! - [`win`] —— Win32 小工具 (宽字符串、字体、配色、暗色主题、程序图标、弹窗装扮)
//! - [`card`] —— 卡片式自绘界面 (ObCommander 设置窗口那个样子)
//! - [`taskdlg`] —— 带程序图标的 TaskDialog / MessageBox 替身
//! - [`search_edit`] —— 搜索框右侧的清除按钮 ✕
//! - [`cue`] —— 输入框的灰色占位提示 (有焦点也显示, 打字才消失)
//!
//! 规矩: 库里不出界面文案、不读配置、不跑主消息循环 (系统模态框自带的循环除外)。

pub mod card;
pub mod cue;
pub mod search_edit;
pub mod taskdlg;
pub mod win;
