//! Windows title bar dark mode configuration via Desktop Window Manager (DWM).
//!
//! Windows 10 (19041+) and Windows 11 require `DWMWA_USE_IMMERSIVE_DARK_MODE` (attribute 20)
//! to render the native title bar in dark mode with visible white caption buttons.
//! Without this, active windows render with a white title bar and white buttons (invisible).

#![allow(unsafe_code)]

const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const DWMWA_USE_IMMERSIVE_DARK_MODE_OLD: u32 = 19;
const DWMWA_CAPTION_COLOR: u32 = 35;
const DWMWA_TEXT_COLOR: u32 = 36;

const SWP_NOSIZE: u32 = 0x0001;
const SWP_NOMOVE: u32 = 0x0002;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_FRAMECHANGED: u32 = 0x0020;

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(hwnd: isize, dw_attribute: u32, pv_attribute: *const std::ffi::c_void, cb_attribute: u32) -> i32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn SetWindowPos(hwnd: isize, hwnd_insert_after: isize, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
}

/// Apply dark or light theme to the native Windows title bar.
pub fn apply(hwnd: isize, dark: bool) {
    if hwnd == 0 {
        return;
    }
    let value: i32 = if dark { 1 } else { 0 };
    unsafe {
        // Attribute 20: Windows 10 (2004+) and Windows 11
        let res = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, (&value as *const i32).cast(), std::mem::size_of::<i32>() as u32);
        if res != 0 {
            // Attribute 19: Windows 10 (1809 - 1909)
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE_OLD, (&value as *const i32).cast(), std::mem::size_of::<i32>() as u32);
        }

        if dark {
            // Dark title bar background (#202020) and crisp white text on Windows 11
            let caption_color: u32 = 0x00202020;
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR, (&caption_color as *const u32).cast(), std::mem::size_of::<u32>() as u32);
            let text_color: u32 = 0x00FFFFFF;
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_TEXT_COLOR, (&text_color as *const u32).cast(), std::mem::size_of::<u32>() as u32);
        } else {
            // Reset to default on light theme (0xFFFFFFFF = DWMWA_COLOR_DEFAULT)
            let default_color: u32 = 0xFFFFFFFF;
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR, (&default_color as *const u32).cast(), std::mem::size_of::<u32>() as u32);
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_TEXT_COLOR, (&default_color as *const u32).cast(), std::mem::size_of::<u32>() as u32);
        }

        // Flush non-client metrics and repaint title bar immediately
        SetWindowPos(hwnd, 0, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED);
    }
}
