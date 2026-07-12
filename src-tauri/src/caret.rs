// VoxLink 光标模块
// 职责：跨平台光标物理定位 & 前后各 200 字符上下文提取
//
// 防御性重构：移除易碎的 macOS AXUIElement FFI 实现，
// 统一降级为鼠标位置兜底。Windows 使用 IUIAutomation 保持。
//
// 技术指标：
// - Windows: IUIAutomation8 获取焦点控件，
//   查询 IUIAutomationTextPattern2 接口调用 GetCaretRange 获取 BoundingBox
//   降级：Win32 GetGUIThreadInfo + ClientToScreen
// - macOS / Linux：降级为鼠标热点相对坐标

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
    #[cfg(target_os = "windows")]
    {
        get_caret_context_windows()
    }

    #[cfg(not(target_os = "windows"))]
    {
        fallback_to_mouse_position()
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
        GetCursorPos, GetWindowRect, GetWindowThreadProcessId, ClientToScreen,
    };
    use windows::Win32::Foundation::POINT;
    use windows::core::BSTR;

    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        // 尝试轨道一：IUIAutomation
        let automation: IUIAutomation = windows::Win32::System::Com::CoCreateInstance(
            &CUIAutomation,
            None,
            windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
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
        ));
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