//! 图形与输入相关的共享类型定义。

/// BGRA8888 帧缓冲格式。
pub const FRAMEBUFFER_FORMAT_BGRA8888: u32 = 1;

/// 用户态可见的帧缓冲信息。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct FramebufferInfo {
    /// 实际显示宽度（像素）。
    pub width: u32,
    /// 实际显示高度（像素）。
    pub height: u32,
    /// 每行像素数。
    pub stride: u32,
    /// 像素格式，当前固定为 `FRAMEBUFFER_FORMAT_BGRA8888`。
    pub format: u32,
}

/// VirtIO 输入事件类型：按键。
pub const INPUT_EVENT_KEY: u16 = 0x01;

/// 按键释放。
pub const INPUT_VALUE_RELEASE: u32 = 0;
/// 按键按下。
pub const INPUT_VALUE_PRESS: u32 = 1;
/// 按键重复。
pub const INPUT_VALUE_REPEAT: u32 = 2;

/// 用户态可见的输入事件结构。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct InputEvent {
    /// 事件类型，当前主要使用 `INPUT_EVENT_KEY`。
    pub event_type: u16,
    /// Linux evdev 风格的按键码。
    pub code: u16,
    /// 事件值：0 释放，1 按下，2 重复。
    pub value: u32,
}

/// Doom 风格控制会用到的一组常见按键码。
pub mod key {
    /// Escape。
    pub const ESC: u16 = 1;
    /// Enter。
    pub const ENTER: u16 = 28;
    /// Space。
    pub const SPACE: u16 = 57;
    /// Left arrow。
    pub const LEFT: u16 = 105;
    /// Right arrow。
    pub const RIGHT: u16 = 106;
    /// Up arrow。
    pub const UP: u16 = 103;
    /// Down arrow。
    pub const DOWN: u16 = 108;
    /// A。
    pub const A: u16 = 30;
    /// D。
    pub const D: u16 = 32;
    /// E。
    pub const E: u16 = 18;
    /// Q。
    pub const Q: u16 = 16;
    /// S。
    pub const S: u16 = 31;
    /// W。
    pub const W: u16 = 17;
}
