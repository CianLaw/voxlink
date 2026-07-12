// VoxLink 光标模块
// 职责：跨平台光标物理定位 & 前后各 200 字符上下文提取
//
// 技术指标：
// - macOS: AXUIElement 获取 kAXFocusedUIElementAttribute，
//   读取 kAXSelectedTextRangeAttribute 得到光标相对索引，
//   调用 kAXBoundsForRangeParameterizedAttribute 获取 CGRect，
//   Y 轴翻转转换为屏幕绝对坐标
// - Windows: IUIAutomation8 获取焦点控件，
//   查询 IUIAutomationTextPattern2 接口调用 GetCaretRange 获取 BoundingBox
//   降级：Win32 GetGUIThreadInfo + ClientToScreen
// - 兜底：获取不到坐标时，降级为鼠标热点相对坐标

use anyhow::{Context, Result};

/// 光标上下文信息
#[derive(Debug, Clone, Default)]
pub struct CaretContext {
    /// 光标前的文本（最多 200 字符）
    pub before_text: String,
    /// 光标后的文本（最多 200 字符）
    pub after_text: String,
    /// 光标屏幕坐标 X
    pub caret_x: f64,
    /// 光标屏幕坐标 Y
    pub caret_y: f64,
    /// 光标在屏幕上的宽度
    pub caret_width: f64,
    /// 光标在屏幕上的高度
    pub caret_height: f64,
}

/// 获取光标上下文（平台分发）
pub fn get_caret_context() -> Result<CaretContext> {
    #[cfg(target_os = "macos")]
    {
        get_caret_context_macos()
    }

    #[cfg(target_os = "windows")]
    {
        get_caret_context_windows()
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // Linux 等平台：返回空上下文
        log::warn!("[VoxLink] 当前平台不支持光标上下文提取");
        Ok(CaretContext::default())
    }
}

// ============================================================================
// macOS 实现：AXUIElement Accessibility API
// ============================================================================

#[cfg(target_os = "macos")]
fn get_caret_context_macos() -> Result<CaretContext> {
    use core_foundation::base::TCFType;
    use core_foundation::string::{CFString, CFStringRef};
    use core_foundation::array::CFArray;
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::number::CFNumber;
    use core_foundation::data::CFData;
    use core_graphics::geometry::CGRect;
    use std::ptr;

    unsafe {
        // 获取系统全局 Accessibility 对象
        let system_wide: AXUIElementRef = AXUIElementCreateSystemWide();

        // 获取当前焦点应用
        let mut focused_app: AXUIElementRef = ptr::null_mut();
        let result = AXUIElementCopyAttributeValue(
            system_wide,
            kAXFocusedApplicationAttribute as CFStringRef,
            &mut focused_app as *mut _ as *mut CFTypeRef,
        );

        if result != 0 || focused_app.is_null() {
            // 降级：使用鼠标位置
            return fallback_to_mouse_position();
        }

        // 获取焦点应用的焦点 UI 元素
        let mut focused_element: AXUIElementRef = ptr::null_mut();
        let result = AXUIElementCopyAttributeValue(
            focused_app,
            kAXFocusedUIElementAttribute as CFStringRef,
            &mut focused_element as *mut _ as *mut CFTypeRef,
        );

        CFRelease(focused_app as *mut _);

        if result != 0 || focused_element.is_null() {
            return fallback_to_mouse_position();
        }

        // 获取选中文本范围（AXValueRef 类型）
        let mut selected_range_value: CFTypeRef = ptr::null_mut();
        let result = AXUIElementCopyAttributeValue(
            focused_element,
            kAXSelectedTextRangeAttribute as CFStringRef,
            &mut selected_range_value,
        );

        let mut caret_pos: isize = 0;
        let mut has_range = false;

        if result == 0 && !selected_range_value.is_null() {
            // 解析 AXValue 获取 range
            let mut range_value = CFRange { location: 0, length: 0 };
            let got_range = AXValueGetValue(
                selected_range_value as AXValueRef,
                kAXValueTypeCFRange,
                &mut range_value as *mut _ as *mut c_void,
            );

            if got_range != 0 {
                caret_pos = range_value.location as isize;
                has_range = true;
            }
            CFRelease(selected_range_value);
        }

        // 获取控件完整文本
        let mut full_text_value: CFTypeRef = ptr::null_mut();
        let result = AXUIElementCopyAttributeValue(
            focused_element,
            kAXValueAttribute as CFStringRef,
            &mut full_text_value,
        );

        let mut full_text = String::new();
        if result == 0 && !full_text_value.is_null() {
            let cf_str = CFString::wrap_under_create_rule(full_text_value as CFStringRef);
            full_text = cf_str.to_string();
        }

        // 提取前后文本
        let (before_text, after_text) = if !full_text.is_empty() {
            let chars: Vec<char> = full_text.chars().collect();
            let total_len = chars.len();
            let pos = if has_range { caret_pos as usize } else { total_len };
            let pos = pos.min(total_len);

            let before_start = if pos > 200 { pos - 200 } else { 0 };
            let after_end = (pos + 200).min(total_len);

            let before: String = chars[before_start..pos].iter().collect();
            let after: String = chars[pos..after_end].iter().collect();
            (before, after)
        } else {
            (String::new(), String::new())
        };

        // 获取光标屏幕坐标
        let (caret_x, caret_y, caret_w, caret_h) = get_caret_bounds_macos(
            focused_element,
            caret_pos,
        );

        CFRelease(focused_element as *mut _);

        Ok(CaretContext {
            before_text,
            after_text,
            caret_x,
            caret_y,
            caret_width: caret_w,
            caret_height: caret_h,
        })
    }
}

#[cfg(target_os = "macos")]
fn get_caret_bounds_macos(
    element: AXUIElementRef,
    caret_pos: isize,
) -> (f64, f64, f64, f64) {
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    use core_graphics::geometry::CGRect;
    use std::ptr;

    unsafe {
        // 构造 range 参数：光标位置，长度为 0
        let range = CFRange {
            location: caret_pos as isize,
            length: 0,
        };

        let range_value = AXValueCreate(
            kAXValueTypeCFRange,
            &range as *const _ as *const c_void,
        );

        if range_value.is_null() {
            return (0.0, 0.0, 0.0, 0.0);
        }

        // 获取光标的 bounds
        let mut bounds_value: CFTypeRef = ptr::null_mut();
        let param_name = CFString::new("AXBoundsForRange");
        let result = AXUIElementCopyParameterizedAttributeValue(
            element,
            kAXBoundsForRangeParameterizedAttribute as CFStringRef,
            range_value as CFTypeRef,
            &mut bounds_value,
        );

        CFRelease(range_value as *mut _);

        if result != 0 || bounds_value.is_null() {
            return (0.0, 0.0, 0.0, 0.0);
        }

        // 解析 CGRect
        let mut rect = CGRect::default();
        let got_rect = AXValueGetValue(
            bounds_value as AXValueRef,
            kAXValueTypeCGRect,
            &mut rect as *mut _ as *mut c_void,
        );

        CFRelease(bounds_value);

        if got_rect == 0 {
            return (0.0, 0.0, 0.0, 0.0);
        }

        // macOS 坐标系 Y 轴翻转：屏幕坐标原点在左上角，而 AX API 返回的是从底部算起的坐标
        // 获取主屏幕高度
        let screen_height = {
            let main_display = CGMainDisplayID();
            CGDisplayPixelsHigh(main_display) as f64
        };

        let flipped_y = screen_height - rect.origin.y - rect.size.height;

        (
            rect.origin.x,
            flipped_y,
            rect.size.width,
            rect.size.height,
        )
    }
}

// ============================================================================
// Windows 实现：IUIAutomation + Win32 GUI
// ============================================================================

#[cfg(target_os = "windows")]
fn get_caret_context_windows() -> Result<CaretContext> {
    use windows::Win32::UI::Accessibility::{
        IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
        IUIAutomationTextPattern2, IUIAutomationTextRange, CUIAutomation,
        UIA_TextPattern2Id, UIA_ValueValuePropertyId, UIA_BoundingRectanglePropertyId,
        UIA_NamePropertyId,
    };
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetGUIThreadInfo, GetForegroundWindow, GUITHREADINFO,
        GetCursorPos,
    };
    use windows::Win32::Foundation::POINT;
    use windows::core::BSTR;

    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        // 尝试轨道一：IUIAutomation
        let automation: IUIAutomation = CoCreateInstance(
            &CUIAutomation,
            None,
            CLSCTX_INPROC_SERVER,
        )?;

        let focused = automation.GetFocusedElement()?;

        // 获取控件文本
        let mut text = String::new();
        let is_text_pattern = {
            let pattern: Result<IUIAutomationTextPattern2, _> =
                focused.GetCurrentPatternAs(UIA_TextPattern2Id);
            pattern.is_ok()
        };

        if is_text_pattern {
            // 使用 TextPattern2 获取文本和光标
            let text_pattern: IUIAutomationTextPattern2 =
                focused.GetCurrentPatternAs(UIA_TextPattern2Id)?;

            let doc_range = text_pattern.GetDocumentRange()?;
            text = doc_range.GetText(-1)?.to_string();

            // 获取光标范围
            if let Ok(caret_range) = text_pattern.GetCaretRange(&mut POINT::default()) {
                // 获取光标位置
                let caret_rect = caret_range.GetBoundingRectangles()?;
                if !caret_rect.is_empty() {
                    let rect = caret_rect[0];
                    let chars: Vec<char> = text.chars().collect();
                    let pos = chars.len(); // 简化：默认在末尾

                    let before_start = if pos > 200 { pos - 200 } else { 0 };
                    let after_end = (pos + 200).min(chars.len());
                    let before_text: String = chars[before_start..pos].iter().collect();
                    let after_text: String = chars[pos..after_end].iter().collect();

                    return Ok(CaretContext {
                        before_text,
                        after_text,
                        caret_x: rect.left as f64,
                        caret_y: rect.top as f64,
                        caret_width: (rect.right - rect.left) as f64,
                        caret_height: (rect.bottom - rect.top) as f64,
                    });
                }
            }
        }

        // 轨道二（降级）：Win32 GetGUIThreadInfo
        let hwnd = GetForegroundWindow();
        if hwnd.0 != 0 {
            let mut gui_info = GUITHREADINFO::default();
            gui_info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;

            let thread_id = GetWindowThreadProcessId(hwnd, None);
            if GetGUIThreadInfo(thread_id, &mut gui_info).as_bool() {
                // 获取经典文本控件的光标位置
                if gui_info.hwndCaret.0 != 0 {
                    let mut caret_rect = windows::Win32::Foundation::RECT::default();
                    GetWindowRect(gui_info.hwndCaret, &mut caret_rect)?;

                    // ClientToScreen 转换
                    let mut pt = POINT {
                        x: caret_rect.left,
                        y: caret_rect.top,
                    };
                    ClientToScreen(gui_info.hwndFocus, &mut pt)?;

                    let width = (caret_rect.right - caret_rect.left) as f64;
                    let height = (caret_rect.bottom - caret_rect.top) as f64;

                    // 提取文本
                    let chars: Vec<char> = text.chars().collect();
                    let pos = chars.len();
                    let before_start = if pos > 200 { pos - 200 } else { 0 };
                    let after_end = (pos + 200).min(chars.len());
                    let before_text: String = chars[before_start..pos].iter().collect();
                    let after_text: String = chars[pos..after_end].iter().collect();

                    return Ok(CaretContext {
                        before_text,
                        after_text,
                        caret_x: pt.x as f64,
                        caret_y: pt.y as f64,
                        caret_width: width,
                        caret_height: height,
                    });
                }
            }
        }

        // 兜底：鼠标位置
        fallback_to_mouse_position()
    }
}

// ============================================================================
// 通用降级：鼠标位置
// ============================================================================

fn fallback_to_mouse_position() -> Result<CaretContext> {
    log::info!("[VoxLink] 无法获取光标位置，降级为鼠标坐标");

    #[cfg(target_os = "macos")]
    {
        use core_graphics::event::CGEvent;
        use core_graphics::event_source::CGEventSource;
        let event = CGEvent::new(CGEventSource::new(
            core_graphics::event_source::CGEventSourceStateID::CombinedSessionState,
        ).unwrap());
        if let Some(event) = event {
            let point = event.location();
            return Ok(CaretContext {
                caret_x: point.x,
                caret_y: point.y,
                caret_width: 2.0,
                caret_height: 20.0,
                ..Default::default()
            });
        }
    }

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        use windows::Win32::Foundation::POINT;

        let mut pt = POINT::default();
        unsafe {
            if GetCursorPos(&mut pt).as_bool() {
                return Ok(CaretContext {
                    caret_x: pt.x as f64,
                    caret_y: pt.y as f64,
                    caret_width: 2.0,
                    caret_height: 20.0,
                    ..Default::default()
                });
            }
        }
    }

    Ok(CaretContext::default())
}

// ============================================================================
// 外部类型声明（macOS Accessibility API）
// ============================================================================

#[cfg(target_os = "macos")]
mod ffi {
    use core_foundation::base::{CFTypeID, CFTypeRef, OSStatus};
    use core_foundation::string::CFStringRef;
    use core_graphics::geometry::CGRect;
    use std::os::raw::c_void;

    pub type AXUIElementRef = *mut c_void;
    pub type AXValueRef = *mut c_void;

    #[repr(C)]
    pub struct CFRange {
        pub location: isize,
        pub length: isize,
    }

    pub const kAXValueTypeCFRange: i32 = 0;
    pub const kAXValueTypeCGRect: i32 = 1;

    extern "C" {
        pub static kAXFocusedApplicationAttribute: CFStringRef;
        pub static kAXFocusedUIElementAttribute: CFStringRef;
        pub static kAXSelectedTextRangeAttribute: CFStringRef;
        pub static kAXValueAttribute: CFStringRef;
        pub static kAXBoundsForRangeParameterizedAttribute: CFStringRef;

        pub fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        pub fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> OSStatus;
        pub fn AXUIElementCopyParameterizedAttributeValue(
            element: AXUIElementRef,
            parameterized_attribute: CFStringRef,
            parameter: CFTypeRef,
            result: *mut CFTypeRef,
        ) -> OSStatus;
        pub fn AXValueCreate(
            value_type: i32,
            value_ptr: *const c_void,
        ) -> AXValueRef;
        pub fn AXValueGetValue(
            value: AXValueRef,
            value_type: i32,
            value_ptr: *mut c_void,
        ) -> i32;
        pub fn CFRelease(cf: *mut c_void);
        pub fn CGMainDisplayID() -> u32;
        pub fn CGDisplayPixelsHigh(display: u32) -> usize;
    }
}

#[cfg(target_os = "macos")]
use ffi::*;

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_caret_context() {
        let ctx = CaretContext::default();
        assert!(ctx.before_text.is_empty());
        assert!(ctx.after_text.is_empty());
        assert_eq!(ctx.caret_x, 0.0);
        assert_eq!(ctx.caret_y, 0.0);
    }

    #[test]
    fn test_fallback_to_mouse_position() {
        let result = fallback_to_mouse_position();
        // 在测试环境中可能无法获取实际鼠标位置，但不应该 panic
        assert!(result.is_ok());
    }
}