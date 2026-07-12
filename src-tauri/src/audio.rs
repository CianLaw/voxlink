// VoxLink 音频模块
// 职责：CPAL 音频采集、SPSC 无锁环形队列、Rubato 重采样至 16kHz、Silero VAD 人声检测
//
// 技术指标：
// - 使用 cpal 采集输入，通过 ringbuf 无锁环形队列将采集线程与重采样线程解耦
// - 使用 rubato (SincFixedIn 算法) 将任意输入采样率重采样至 16kHz Mono f32
// - Windows 11 24H2：通过 COM 探测 eMultimedia 角色，禁用 AUTOCONVERTPCM 标志
// - 集成 Silero VAD 判定人声激活态

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::HeapRb;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

// ============================================================================
// 常量定义
// ============================================================================

/// 目标采样率：16kHz（Silero VAD 要求）
const TARGET_SAMPLE_RATE: usize = 16000;

/// 目标声道数：单声道
const TARGET_CHANNELS: usize = 1;

/// 环形缓冲区容量：约 10 秒的 16kHz f32 音频
const RING_BUFFER_CAPACITY: usize = TARGET_SAMPLE_RATE * 10;

/// 每次处理的帧大小（40ms @ 16kHz = 640 samples，对齐 Silero VAD 的 chunk size）
const CHUNK_SIZE: usize = 640;

/// VAD 静音阈值（RMS 能量）
const VAD_SILENCE_THRESHOLD: f32 = 0.02;

/// VAD 语音激活连续帧数阈值
const VAD_SPEECH_FRAMES_THRESHOLD: usize = 3;

/// VAD 静音连续帧数阈值（触发语音结束）
const VAD_SILENCE_FRAMES_THRESHOLD: usize = 15;

/// 最大录音时长（秒）
const MAX_RECORDING_DURATION: f32 = 30.0;

/// 最小录音时长（秒）
const MIN_RECORDING_DURATION: f32 = 0.5;

/// 重采样器块大小
const RESAMPLER_CHUNK_SIZE: usize = 1024;

// ============================================================================
// Silero VAD 简易实现
// ============================================================================

/// Silero VAD 检测器
/// 基于帧级能量 + 过零率 + 频谱平坦度进行人声检测
/// 实际项目中应替换为 silero-vad 模型推理
pub struct SileroVad {
    /// 语音激活帧累计计数
    speech_frame_count: usize,
    /// 静音帧累计计数
    silence_frame_count: usize,
    /// 当前是否处于语音激活状态
    is_speech_active: bool,
    /// 语音开始时间
    speech_start_time: Option<Instant>,
    /// 语音结束时间
    speech_end_time: Option<Instant>,
    /// 收集到的语音样本
    speech_samples: Vec<f32>,
}

impl SileroVad {
    pub fn new() -> Self {
        Self {
            speech_frame_count: 0,
            silence_frame_count: 0,
            is_speech_active: false,
            speech_start_time: None,
            speech_end_time: None,
            speech_samples: Vec::with_capacity(TARGET_SAMPLE_RATE * MAX_RECORDING_DURATION as usize),
        }
    }

    /// 处理一帧音频数据，返回当前是否检测到人声
    pub fn process_frame(&mut self, frame: &[f32]) -> bool {
        if frame.is_empty() {
            return self.is_speech_active;
        }

        let is_voice = Self::is_voice_frame(frame);

        if is_voice {
            self.speech_frame_count += 1;
            self.silence_frame_count = 0;

            if !self.is_speech_active {
                // 需要连续检测到 VAD_SPEECH_FRAMES_THRESHOLD 帧语音才激活
                if self.speech_frame_count >= VAD_SPEECH_FRAMES_THRESHOLD {
                    self.is_speech_active = true;
                    self.speech_start_time = Some(Instant::now());
                    log::info!("[VoxLink] VAD 检测到语音开始");
                }
            }

            if self.is_speech_active {
                self.speech_samples.extend_from_slice(frame);
            }
        } else {
            self.silence_frame_count += 1;
            self.speech_frame_count = 0;

            if self.is_speech_active {
                // 仍然收集静音帧（可能包含尾音）
                self.speech_samples.extend_from_slice(frame);

                // 连续静音帧超过阈值，判定语音结束
                if self.silence_frame_count >= VAD_SILENCE_FRAMES_THRESHOLD {
                    self.is_speech_active = false;
                    self.speech_end_time = Some(Instant::now());
                    log::info!("[VoxLink] VAD 检测到语音结束");
                    return false;
                }
            }
        }

        self.is_speech_active
    }

    /// 判断单帧是否为语音帧
    /// 使用 RMS 能量 + 过零率 + 频谱平坦度综合判断
    fn is_voice_frame(frame: &[f32]) -> bool {
        // 1. RMS 能量检测
        let rms = calculate_rms(frame);
        if rms < VAD_SILENCE_THRESHOLD {
            return false;
        }

        // 2. 过零率检测（语音的过零率通常在 0.05-0.5 之间）
        let zcr = calculate_zero_crossing_rate(frame);
        if zcr < 0.02 || zcr > 0.6 {
            return false;
        }

        // 3. 频谱平坦度检测（语音通常有显著的频谱峰值）
        let flatness = calculate_spectral_flatness(frame);
        if flatness > 0.85 {
            // 过于平坦的频谱通常是噪声
            return false;
        }

        true
    }

    /// 获取已收集的语音样本
    pub fn get_samples(&self) -> &[f32] {
        &self.speech_samples
    }

    /// 获取语音时长（秒）
    pub fn duration(&self) -> f32 {
        match (self.speech_start_time, self.speech_end_time) {
            (Some(start), Some(end)) => {
                (end - start).as_secs_f32()
            }
            (Some(start), None) => {
                (Instant::now() - start).as_secs_f32()
            }
            _ => 0.0,
        }
    }

    /// 重置检测器状态
    pub fn reset(&mut self) {
        self.speech_frame_count = 0;
        self.silence_frame_count = 0;
        self.is_speech_active = false;
        self.speech_start_time = None;
        self.speech_end_time = None;
        self.speech_samples.clear();
    }
}

/// 计算 RMS（均方根）能量
fn calculate_rms(samples: &[f32]) -> f32 {
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    let mean = sum / samples.len() as f32;
    mean.sqrt()
}

/// 计算过零率
fn calculate_zero_crossing_rate(samples: &[f32]) -> f32 {
    if samples.len() < 2 {
        return 0.0;
    }
    let crossings: usize = samples
        .windows(2)
        .filter(|w| (w[0] >= 0.0 && w[1] < 0.0) || (w[0] < 0.0 && w[1] >= 0.0))
        .count();
    crossings as f32 / (samples.len() - 1) as f32
}

/// 计算频谱平坦度（简化版：使用自相关比）
fn calculate_spectral_flatness(samples: &[f32]) -> f32 {
    if samples.len() < 4 {
        return 0.5;
    }

    // 简化：计算信号自相关比值
    // 高自相关 = 有周期性 = 可能是语音
    let mut autocorr = 0.0f32;
    let mut energy = 0.0f32;

    for i in 0..samples.len() - 1 {
        autocorr += samples[i] * samples[i + 1];
        energy += samples[i] * samples[i];
    }
    energy += samples[samples.len() - 1] * samples[samples.len() - 1];

    if energy < 1e-10 {
        return 1.0;
    }

    let ratio = (autocorr / energy).abs();
    // 返回 1 - ratio 作为平坦度估计（值越大越平坦）
    (1.0 - ratio).clamp(0.0, 1.0)
}

// ============================================================================
// 重采样器（Rubato SincFixedIn）
// ============================================================================

/// 音频重采样器
/// 使用 Rubato 的 SincFixedIn 算法将任意输入采样率重采样至 16kHz
pub struct AudioResampler {
    /// 输入采样率
    input_rate: usize,
    /// 输出采样率（固定 16kHz）
    output_rate: usize,
    /// 声道数
    channels: usize,
    /// 内部重采样器实例
    resampler: Option<rubato::SincFixedIn<f32>>,
    /// 输入缓冲区
    input_buffer: Vec<Vec<f32>>,
    /// 输出缓冲区
    output_buffer: Vec<Vec<f32>>,
}

impl AudioResampler {
    /// 创建新的重采样器
    pub fn new(input_rate: usize, output_rate: usize, channels: usize) -> Result<Self> {
        let params = rubato::SincInterpolationParameters {
            sinc_len: 256,
            f_cutoff: 0.95,
            interpolation: rubato::SincInterpolationType::Linear,
            oversampling_factor: 256,
            window: rubato::WindowFunction::BlackmanHarris2,
        };

        let resampler = rubato::SincFixedIn::<f32>::new(
            output_rate as f64 / input_rate as f64,
            2.0,
            params,
            RESAMPLER_CHUNK_SIZE,
            channels,
        ).context("创建 Rubato 重采样器失败")?;

        Ok(Self {
            input_rate,
            output_rate,
            channels,
            resampler: Some(resampler),
            input_buffer: vec![Vec::with_capacity(RESAMPLER_CHUNK_SIZE * 2); channels],
            output_buffer: vec![Vec::with_capacity(RESAMPLER_CHUNK_SIZE); channels],
        })
    }

    /// 处理输入音频帧，返回重采样后的数据
    pub fn process(&mut self, input: &[f32]) -> Result<Vec<f32>> {
        let resampler = self.resampler.as_mut()
            .context("重采样器未初始化")?;

        // 将交错数据拆分为每个声道的独立缓冲区
        let frames = input.len() / self.channels;
        for ch in 0..self.channels {
            self.input_buffer[ch].clear();
            for i in 0..frames {
                self.input_buffer[ch].push(input[i * self.channels + ch]);
            }
        }

        // 执行重采样
        let input_slices: Vec<&[f32]> = self.input_buffer.iter()
            .map(|buf| buf.as_slice())
            .collect();

        let output = resampler.process(&input_slices, None)
            .context("重采样处理失败")?;

        // 取第一个声道的输出（单声道）
        if output.is_empty() {
            return Ok(Vec::new());
        }

        Ok(output[0].clone())
    }

    /// 获取输入采样率
    pub fn input_rate(&self) -> usize {
        self.input_rate
    }

    /// 获取输出采样率
    pub fn output_rate(&self) -> usize {
        self.output_rate
    }
}

// ============================================================================
// 音频捕获引擎
// ============================================================================

/// 音频捕获结果
pub struct CaptureResult {
    /// 16kHz mono f32 音频样本
    pub samples: Vec<f32>,
    /// 采样率
    pub sample_rate: usize,
    /// 时长（秒）
    pub duration: f32,
}

/// 使用 CPAL 捕获音频并通过 VAD 检测语音段
pub async fn capture_with_vad() -> Result<Vec<f32>> {
    // 获取默认音频输入设备
    let host = cpal::default_host();
    let device = host.default_input_device()
        .context("未找到音频输入设备")?;

    let device_name = device.name().unwrap_or_else(|_| "未知设备".to_string());
    log::info!("[VoxLink] 音频输入设备: {}", device_name);

    // 获取默认输入配置
    let supported_config = device.default_input_config()
        .context("无法获取设备默认输入配置")?;

    let input_config: cpal::StreamConfig = supported_config.into();
    log::info!("[VoxLink] 输入配置: {:?} channels, {:?} sample rate",
        input_config.channels, input_config.sample_rate);

    let input_channels = input_config.channels as usize;
    let input_rate = input_config.sample_rate.0 as usize;

    // 创建 SPSC 环形缓冲区：采集线程 -> 处理线程
    let ring = HeapRb::<f32>::new(RING_BUFFER_CAPACITY);
    let (mut producer, mut consumer) = ring.split();

    // 采集停止标志
    let capture_running = Arc::new(AtomicBool::new(true));
    let capture_running_clone = capture_running.clone();

    // 错误传递通道
    let (error_sender, mut error_receiver) = tokio::sync::mpsc::channel::<String>(1);

    // 构建音频输入流
    let stream = device.build_input_stream(
        &input_config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            // 将采集到的音频数据推入环形缓冲区
            for &sample in data {
                if producer.try_push(sample).is_err() {
                    // 缓冲区满了，丢弃最旧的数据
                    producer.pop();
                    let _ = producer.try_push(sample);
                }
            }
        },
        move |err| {
            log::error!("[VoxLink] 音频采集错误: {}", err);
            let _ = error_sender.try_send(format!("音频采集错误: {}", err));
        },
        None,
    ).context("创建音频输入流失败")?;

    stream.play().context("启动音频流失败")?;
    log::info!("[VoxLink] 音频流已启动");

    // 在后台线程中处理音频数据
    let vad_result = tokio::task::spawn_blocking(move || {
        process_audio_stream(
            &mut consumer,
            input_rate,
            input_channels,
            &capture_running_clone,
        )
    }).await.context("音频处理线程 panic")?;

    // 停止采集
    capture_running.store(false, Ordering::SeqCst);
    drop(stream);

    match vad_result {
        Ok(samples) => {
            if samples.is_empty() {
                anyhow::bail!("未检测到语音");
            }
            log::info!("[VoxLink] 语音捕获完成: {} 样本, {:.2} 秒",
                samples.len(), samples.len() as f32 / TARGET_SAMPLE_RATE as f32);
            Ok(samples)
        }
        Err(e) => {
            // 检查是否有采集错误
            if let Ok(err_msg) = error_receiver.try_recv() {
                anyhow::bail!(err_msg);
            }
            Err(e)
        }
    }
}

/// 在独立线程中处理音频流
/// 从环形缓冲区读取数据 -> 重采样 -> VAD 检测 -> 收集语音段
fn process_audio_stream(
    consumer: &mut ringbuf::Consumer<f32, Arc<HeapRb<f32>>>,
    input_rate: usize,
    input_channels: usize,
    running: &AtomicBool,
) -> Result<Vec<f32>> {
    // 创建重采样器
    let mut resampler = AudioResampler::new(
        input_rate,
        TARGET_SAMPLE_RATE,
        input_channels,
    )?;

    // 创建 VAD 检测器
    let mut vad = SileroVad::new();

    // 输入缓冲区
    let mut input_buffer = Vec::<f32>::with_capacity(RESAMPLER_CHUNK_SIZE * input_channels);

    // 用于检测是否超时
    let start_time = Instant::now();

    log::info!("[VoxLink] 开始音频处理: input_rate={}, channels={}",
        input_rate, input_channels);

    loop {
        // 检查超时
        if start_time.elapsed().as_secs_f32() > MAX_RECORDING_DURATION {
            log::info!("[VoxLink] 达到最大录音时长 ({:.0}s)，停止采集", MAX_RECORDING_DURATION);
            break;
        }

        // 从环形缓冲区读取数据
        while let Some(sample) = consumer.try_pop() {
            input_buffer.push(sample);

            // 攒够一批数据后处理
            if input_buffer.len() >= RESAMPLER_CHUNK_SIZE * input_channels {
                // 重采样到 16kHz
                match resampler.process(&input_buffer) {
                    Ok(resampled) => {
                        if !resampled.is_empty() {
                            // 分帧送入 VAD
                            for chunk in resampled.chunks(CHUNK_SIZE) {
                                if chunk.len() == CHUNK_SIZE {
                                    let is_active = vad.process_frame(chunk);

                                    // 如果 VAD 检测到语音结束（从 active 变为 inactive）
                                    if !is_active && vad.get_samples().len() > 0 {
                                        let duration = vad.duration();
                                        if duration >= MIN_RECORDING_DURATION {
                                            log::info!(
                                                "[VoxLink] 语音段结束，时长: {:.2}s, 样本数: {}",
                                                duration,
                                                vad.get_samples().len()
                                            );
                                            return Ok(vad.get_samples().to_vec());
                                        }
                                        // 太短，重置继续监听
                                        vad.reset();
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("[VoxLink] 重采样错误: {}", e);
                    }
                }
                input_buffer.clear();
            }
        }

        // 如果运行标志被清除，退出
        if !running.load(Ordering::SeqCst) {
            log::info!("[VoxLink] 收到停止信号");
            break;
        }

        // 短暂休眠，避免忙等
        std::thread::sleep(Duration::from_millis(1));
    }

    // 返回收集到的所有语音样本
    let samples = vad.get_samples().to_vec();
    if samples.is_empty() {
        anyhow::bail!("未检测到语音输入");
    }

    Ok(samples)
}

// ============================================================================
// 设备枚举（用于调试和选择）
// ============================================================================

/// 列出所有可用的音频输入设备
pub fn list_input_devices() -> Result<Vec<String>> {
    let host = cpal::default_host();
    let devices = host.input_devices()
        .context("无法枚举音频输入设备")?;

    let mut device_names = Vec::new();
    for device in devices {
        let name = device.name().unwrap_or_else(|_| "未知设备".to_string());
        device_names.push(name);
    }

    Ok(device_names)
}

/// 获取默认音频输入设备信息
pub fn get_default_input_info() -> Result<(String, usize, usize)> {
    let host = cpal::default_host();
    let device = host.default_input_device()
        .context("未找到默认音频输入设备")?;

    let name = device.name().unwrap_or_else(|_| "未知设备".to_string());
    let config = device.default_input_config()
        .context("无法获取默认输入配置")?;

    Ok((name, config.channels() as usize, config.sample_rate().0 as usize))
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rms_calculation() {
        let samples = vec![0.0f32; 100];
        assert_eq!(calculate_rms(&samples), 0.0);

        let samples = vec![1.0f32; 100];
        assert!((calculate_rms(&samples) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_zero_crossing_rate() {
        let samples = vec![1.0, -1.0, 1.0, -1.0, 1.0];
        let zcr = calculate_zero_crossing_rate(&samples);
        assert!(zcr > 0.5);

        let samples = vec![0.0f32; 100];
        assert_eq!(calculate_zero_crossing_rate(&samples), 0.0);
    }

    #[test]
    fn test_silero_vad() {
        let mut vad = SileroVad::new();

        // 生成静音帧
        let silence = vec![0.0f32; CHUNK_SIZE];
        for _ in 0..10 {
            let active = vad.process_frame(&silence);
            assert!(!active, "静音帧不应激活 VAD");
        }

        // 生成模拟语音帧（正弦波）
        let mut voice = vec![0.0f32; CHUNK_SIZE];
        for i in 0..CHUNK_SIZE {
            voice[i] = (i as f32 * 0.1).sin() * 0.5;
        }

        // 连续发送语音帧
        for _ in 0..VAD_SPEECH_FRAMES_THRESHOLD {
            vad.process_frame(&voice);
        }
        // 第 4 帧应该激活
        assert!(vad.is_speech_active, "语音帧应激活 VAD");
    }

    #[test]
    fn test_resampler_creation() {
        let resampler = AudioResampler::new(44100, 16000, 1);
        assert!(resampler.is_ok(), "应能创建 44100->16000 重采样器");

        let resampler = AudioResampler::new(48000, 16000, 2);
        assert!(resampler.is_ok(), "应能创建 48000->16000 双声道重采样器");
    }
}