// VoxLink 文本注入模块
// 职责：无感文本注入 + 剪贴板备份与复原
//
// 技术指标：
// - 写入前暂存并备份剪贴板原生快照
// - 覆写语音文本至系统剪贴板
// - macOS: 模拟 Cmd+V (CGEvent 构造)
// - Windows: 模拟 Ctrl+V (物理扫描码链)
// - 异步等待阻尼时间 t_delay = 100ms，然后将原备份数据静默写回

use anyhow::{Context, Result};
use std::time::Duration;

/// 粘贴操作阻尼时间（毫秒）
const PASTE_DELAY_MS: u64 = 100;

/// 剪贴板快照：支持文本和富文本格式
#[derive(Debug, Clone)]
pub struct ClipboardSnapshot {
    /// 纯文本内容
    pub text: Option<String>,
    /// 富文本（RTF/HTML）内容的字节表示
    pub rich_text: Option<Vec<u8>>,
    /// 图片数据的 PNG 字节表示
    pub image: Option<Vec<u8>>,
}

impl ClipboardSnapshot {
    /// 创建空快照
    pub fn empty() -> Self {
        Self {
            text: None,
            rich_text: None,
            image: None,
        }
    }
}

/// 将文本注入到当前光标位置
/// 流程：
/// 1. 备份当前剪贴板内容
/// 2. 将语音文本写入剪贴板
/// 3. 模拟粘贴快捷键
/// 4. 等待阻尼时间
/// 5. 恢复原始剪贴板内容
pub async fn inject_text(text: &str) -> Result<()> {
    if text.is_empty() {
        log::warn!("[VoxLink] 注入文本为空，跳过");
        return Ok(());
    }

    log::info!("[VoxLink] 开始注入文本: {} 字符", text.len());

    // 1. 备份剪贴板
    let snapshot = backup_clipboard().await
        .unwrap_or_else(|e| {
            log::warn!("[VoxLink] 剪贴板备份失败: {}，继续注入", e);
            ClipboardSnapshot::empty()
        });

    // 2. 写入文本到剪贴板
    write_text_to_clipboard(text).await
        .context("写入剪贴板失败")?;

    // 3. 模拟粘贴快捷键
    simulate_paste().await
        .context("模拟粘贴失败")?;

    // 4. 等待阻尼时间（确保宿主应用完成粘贴操作）
    tokio::time::sleep(Duration::from_millis(PASTE_DELAY_MS)).await;

    // 5. 恢复原始剪贴板内容
    if let Err(e) = restore_clipboard(&snapshot).await {
        log::warn!("[VoxLink] 剪贴板恢复失败: {}", e);
    }

    log::info!("[VoxLink] 文本注入完成");
    Ok(())
}

// ============================================================================
// 剪贴板操作
// ============================================================================

/// 备份当前剪贴板内容
async fn backup_clipboard() -> Result<ClipboardSnapshot> {
    let mut snapshot = ClipboardSnapshot::empty();

    // 尝试读取纯文本
    if let Ok(text) = read_clipboard_text().await {
        snapshot.text = Some(text);
    }

    // 尝试读取图片（如果需要）
    if let Ok(image) = read_clipboard_image().await {
        snapshot.image = Some(image);
    }

    log::info!("[VoxLink] 剪贴板备份: text={}, image={}",
        snapshot.text.is_some(),
        snapshot.image.is_some());

    Ok(snapshot)
}

/// 恢复剪贴板内容
async fn restore_clipboard(snapshot: &ClipboardSnapshot) -> Result<()> {
    log::info!("[VoxLink] 开始恢复剪贴板...");

    // 清除剪贴板
    clear_clipboard().await?;

    // 恢复文本
    if let Some(ref text) = snapshot.text {
        write_text_to_clipboard(text).await?;
    }

    // 恢复图片
    if let Some(ref image) = snapshot.image {
        write_image_to_clipboard(image).await?;
    }

    log::info!("[VoxLink] 剪贴板恢复完成");
    Ok(())
}

/// 读取剪贴板文本
async fn read_clipboard_text() -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let output = Command::new("pbpaste")
            .output()
            .context("执行 pbpaste 失败")?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            anyhow::bail!("pbpaste 返回非零状态码");
        }
    }

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::Ole::OleGetClipboard;
        use windows::Win32::System::DataExchange::{
            OpenClipboard, GetClipboardData, CloseClipboard,
            CF_UNICODETEXT, CF_TEXT,
        };
        use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock, GlobalSize};
        use windows::Win32::Foundation::HGLOBAL;

        unsafe {
            if !OpenClipboard(None).as_bool() {
                anyhow::bail!("无法打开剪贴板");
            }

            let result = {
                let handle = GetClipboardData(CF_UNICODETEXT.0 as u32);
                if handle.0.is_null() {
                    CloseClipboard()?;
                    anyhow::bail!("剪贴板无文本数据");
                }

                let hglobal = HGLOBAL(handle.0);
                let size = GlobalSize(hglobal) as usize;
                let ptr = GlobalLock(hglobal) as *const u16;

                if ptr.is_null() {
                    GlobalUnlock(hglobal);
                    CloseClipboard()?;
                    anyhow::bail!("无法锁定剪贴板内存");
                }

                let text = String::from_utf16_lossy(
                    std::slice::from_raw_parts(ptr, size / 2)
                );

                GlobalUnlock(hglobal);
                CloseClipboard()?;

                Ok(text)
            };

            result
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Ok(String::new())
    }
}

/// 写入文本到剪贴板
async fn write_text_to_clipboard(text: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let mut child = Command::new("pbcopy")
            .stdin(Stdio::piped())
            .spawn()
            .context("启动 pbcopy 失败")?;

        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(text.as_bytes())
                .context("写入 pbcopy 失败")?;
        }

        let status = child.wait().context("等待 pbcopy 完成失败")?;
        if !status.success() {
            anyhow::bail!("pbcopy 返回非零状态码");
        }
    }

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::Ole::OleSetClipboard;
        use windows::Win32::System::DataExchange::{
            OpenClipboard, EmptyClipboard, SetClipboardData, CloseClipboard,
            CF_UNICODETEXT,
        };
        use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

        unsafe {
            if !OpenClipboard(None).as_bool() {
                anyhow::bail!("无法打开剪贴板");
            }

            if !EmptyClipboard().as_bool() {
                CloseClipboard()?;
                anyhow::bail!("无法清空剪贴板");
            }

            let wide_text: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let byte_size = wide_text.len() * 2;

            let hglobal = GlobalAlloc(GMEM_MOVEABLE, byte_size)?;
            if hglobal.0.is_null() {
                CloseClipboard()?;
                anyhow::bail!("无法分配全局内存");
            }

            let dst = GlobalLock(hglobal) as *mut u16;
            if dst.is_null() {
                GlobalUnlock(hglobal);
                CloseClipboard()?;
                anyhow::bail!("无法锁定全局内存");
            }

            std::ptr::copy_nonoverlapping(wide_text.as_ptr(), dst, wide_text.len());
            GlobalUnlock(hglobal);

            if SetClipboardData(CF_UNICODETEXT.0 as u32, Some(hglobal)).is_err() {
                CloseClipboard()?;
                anyhow::bail!("设置剪贴板数据失败");
            }

            CloseClipboard()?;
        }
    }

    log::info!("[VoxLink] 已写入剪贴板: {} 字符", text.len());
    Ok(())
}

/// 读取剪贴板图片
async fn read_clipboard_image() -> Result<Vec<u8>> {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let output = Command::new("osascript")
            .arg("-e")
            .arg("try
                set the clipboard to (the clipboard as «class PNGf»)
                return the clipboard
            on error
                return \"\"
            end try")
            .output()
            .context("执行 osascript 失败")?;

        if output.status.success() && !output.stdout.is_empty() {
            Ok(output.stdout)
        } else {
            anyhow::bail!("剪贴板无图片数据");
        }
    }

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::DataExchange::{
            OpenClipboard, GetClipboardData, CloseClipboard, CF_DIB,
            CF_BITMAP,
        };
        use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock, GlobalSize};

        unsafe {
            if !OpenClipboard(None).as_bool() {
                anyhow::bail!("无法打开剪贴板");
            }

            let handle = GetClipboardData(CF_DIB.0 as u32);
            if handle.0.is_null() {
                CloseClipboard()?;
                anyhow::bail!("剪贴板无图片数据");
            }

            let size = GlobalSize(handle) as usize;
            let ptr = GlobalLock(handle) as *const u8;
            let data = std::slice::from_raw_parts(ptr, size).to_vec();
            GlobalUnlock(handle);
            CloseClipboard()?;

            Ok(data)
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        anyhow::bail!("当前平台不支持");
    }
}

/// 写入图片到剪贴板
async fn write_image_to_clipboard(_data: &[u8]) -> Result<()> {
    // 在实际项目中实现完整的图片剪贴板写入
    // 这里保留接口，优先级较低
    log::info!("[VoxLink] 图片剪贴板恢复（当前仅支持文本）");
    Ok(())
}

/// 清除剪贴板
async fn clear_clipboard() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let status = Command::new("pbcopy")
            .arg("/dev/null")
            .status()
            .context("清除剪贴板失败")?;

        if !status.success() {
            anyhow::bail!("pbcopy 清除失败");
        }
    }

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::DataExchange::{
            OpenClipboard, EmptyClipboard, CloseClipboard,
        };

        unsafe {
            if !OpenClipboard(None).as_bool() {
                anyhow::bail!("无法打开剪贴板");
            }
            EmptyClipboard()?;
            CloseClipboard()?;
        }
    }

    Ok(())
}

// ============================================================================
// 粘贴快捷键模拟
// ============================================================================

/// 模拟粘贴快捷键
async fn simulate_paste() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        simulate_paste_macos()
    }

    #[cfg(target_os = "windows")]
    {
        simulate_paste_windows()
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        anyhow::bail!("当前平台不支持键盘模拟");
    }
}

#[cfg(target_os = "macos")]
fn simulate_paste_macos() -> Result<()> {
    use core_foundation::base::TCFType;
    use core_graphics::event::{CGEvent, CGEventTapLocation, CGKeyCode};
    use core_graphics::event_source::CGEventSource;

    // 使用 CGEvent 构造 Cmd+V 按键事件
    let source = CGEventSource::new(
        core_graphics::event_source::CGEventSourceStateID::CombinedSessionState,
    );

    let cmd_key: CGKeyCode = 0x37; // Left Command
    let v_key: CGKeyCode = 0x09;   // V key

    // CGEvent::new_keyboard_event 返回 Result<CGEvent, ()>，
    // () 不实现 std::error::Error，不能用 .context()，用 map_err 替代
    let make_event = |src: &CGEventSource, key: CGKeyCode, down: bool| -> Result<CGEvent> {
        CGEvent::new_keyboard_event(src.clone(), key, down)
            .map_err(|_| anyhow::anyhow!("创建键盘事件失败"))
    };

    unsafe {
        // 按下 Command 键
        let cmd_down = make_event(&source, cmd_key, true)?;
        cmd_down.set_flags(core_graphics::event::CGEventFlags::CGEventFlagCommand);
        cmd_down.post(CGEventTapLocation::HID);

        // 按下 V 键
        let v_down = make_event(&source, v_key, true)?;
        v_down.set_flags(core_graphics::event::CGEventFlags::CGEventFlagCommand);
        v_down.post(CGEventTapLocation::HID);

        // 释放 V 键
        let v_up = make_event(&source, v_key, false)?;
        v_up.set_flags(core_graphics::event::CGEventFlags::CGEventFlagCommand);
        v_up.post(CGEventTapLocation::HID);

        // 释放 Command 键
        let cmd_up = make_event(&source, cmd_key, false)?;
        cmd_up.post(CGEventTapLocation::HID);
    }

    log::info!("[VoxLink] macOS Cmd+V 粘贴模拟完成");
    Ok(())
}

#[cfg(target_os = "windows")]
fn simulate_paste_windows() -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT,
        KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE,
        VK_CONTROL, VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::MapVirtualKeyW;
    use windows::Win32::UI::Input::KeyboardAndMouse::MAPVK_VK_TO_VSC;

    unsafe {
        // 获取 Ctrl 和 V 的扫描码
        let ctrl_scan = MapVirtualKeyW(VK_CONTROL.0 as u32, MAPVK_VK_TO_VSC) as u16;
        let v_scan = MapVirtualKeyW(VK_V.0 as u32, MAPVK_VK_TO_VSC) as u16;

        // 构造输入事件序列
        let mut inputs = vec![
            // 按下 Ctrl
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: ctrl_scan,
                        dwFlags: KEYEVENTF_SCANCODE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            // 按下 V
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: v_scan,
                        dwFlags: KEYEVENTF_SCANCODE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            // 释放 V
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: v_scan,
                        dwFlags: KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            // 释放 Ctrl
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: ctrl_scan,
                        dwFlags: KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        if sent != inputs.len() as u32 {
            log::warn!("[VoxLink] SendInput 只发送了 {}/{} 个事件", sent, inputs.len());
        }

        log::info!("[VoxLink] Windows Ctrl+V 粘贴模拟完成");
    }

    Ok(())
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_snapshot_empty() {
        let snapshot = ClipboardSnapshot::empty();
        assert!(snapshot.text.is_none());
        assert!(snapshot.image.is_none());
    }

    #[test]
    fn test_inject_empty_text() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(inject_text(""));
        assert!(result.is_ok());
    }
}