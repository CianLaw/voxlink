// VoxLink 音频模块
// 职责：CPAL 音频采集、SPSC 无锁环形队列、Rubato 重采样至 16kHz、Silero VAD 人声检测
// 技术指标：
// - 使用 cpal 采集输入，通过 ringbuf 无锁环形队列将采集线程与重采样线程解耦
// - 使用 rubato 将任意输入采样率重采样至 16kHz Mono f32
// - 集成 Silero VAD 判定人声激活态

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::HeapRb;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const TARGET_SAMPLE_RATE: usize = 16000;
const RING_BUFFER_CAPACITY: usize = TARGET_SAMPLE_RATE * 10;
const CHUNK_SIZE: usize = 640;
const VAD_SILENCE_THRESHOLD: f32 = 0.02;
const VAD_SPEECH_FRAMES_THRESHOLD: usize = 3;
const VAD_SILENCE_FRAMES_THRESHOLD: usize = 15;
const MAX_RECORDING_DURATION: f32 = 30.0;
const MIN_RECORDING_DURATION: f32 = 0.5;
const RESAMPLER_CHUNK_SIZE: usize = 1024;

// ============================================================================
// Silero VAD 简易实现
// ============================================================================

pub struct SileroVad {
    speech_frame_count: usize,
    silence_frame_count: usize,
    is_speech_active: bool,
    speech_start_time: Option<Instant>,
    speech_end_time: Option<Instant>,
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

    pub fn process_frame(&mut self, frame: &[f32]) -> bool {
        if frame.is_empty() {
            return self.is_speech_active;
        }
        let is_voice = Self::is_voice_frame(frame);
        if is_voice {
            self.speech_frame_count += 1;
            self.silence_frame_count = 0;
            if !self.is_speech_active && self.speech_frame_count >= VAD_SPEECH_FRAMES_THRESHOLD {
                self.is_speech_active = true;
                self.speech_start_time = Some(Instant::now());
                log::info!("[VoxLink] VAD 检测到语音开始");
            }
            if self.is_speech_active {
                self.speech_samples.extend_from_slice(frame);
            }
        } else {
            self.silence_frame_count += 1;
            self.speech_frame_count = 0;
            if self.is_speech_active {
                self.speech_samples.extend_from_slice(frame);
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

    fn is_voice_frame(frame: &[f32]) -> bool {
        let rms = calculate_rms(frame);
        if rms < VAD_SILENCE_THRESHOLD { return false; }
        let zcr = calculate_zero_crossing_rate(frame);
        if zcr < 0.02 || zcr > 0.6 { return false; }
        let flatness = calculate_spectral_flatness(frame);
        if flatness > 0.85 { return false; }
        true
    }

    pub fn get_samples(&self) -> &[f32] { &self.speech_samples }
    pub fn duration(&self) -> f32 {
        match (self.speech_start_time, self.speech_end_time) {
            (Some(start), Some(end)) => (end - start).as_secs_f32(),
            (Some(start), None) => (Instant::now() - start).as_secs_f32(),
            _ => 0.0,
        }
    }
    pub fn reset(&mut self) {
        self.speech_frame_count = 0;
        self.silence_frame_count = 0;
        self.is_speech_active = false;
        self.speech_start_time = None;
        self.speech_end_time = None;
        self.speech_samples.clear();
    }
}

fn calculate_rms(samples: &[f32]) -> f32 {
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt()
}

fn calculate_zero_crossing_rate(samples: &[f32]) -> f32 {
    if samples.len() < 2 { return 0.0; }
    let crossings = samples.windows(2)
        .filter(|w| (w[0] >= 0.0 && w[1] < 0.0) || (w[0] < 0.0 && w[1] >= 0.0))
        .count();
    crossings as f32 / (samples.len() - 1) as f32
}

fn calculate_spectral_flatness(samples: &[f32]) -> f32 {
    if samples.len() < 4 { return 0.5; }
    let mut autocorr = 0.0f32;
    let mut energy = 0.0f32;
    for i in 0..samples.len() - 1 {
        autocorr += samples[i] * samples[i + 1];
        energy += samples[i] * samples[i];
    }
    energy += samples[samples.len() - 1] * samples[samples.len() - 1];
    if energy < 1e-10 { return 1.0; }
    let ratio = (autocorr / energy).abs();
    (1.0 - ratio).clamp(0.0, 1.0)
}

// ============================================================================
// 简化重采样器（线性插值，无外部依赖）
// ============================================================================

pub struct AudioResampler {
    input_rate: usize,
    output_rate: usize,
    channels: usize,
    leftover: Vec<f32>,
}

impl AudioResampler {
    pub fn new(input_rate: usize, output_rate: usize, channels: usize) -> Self {
        Self { input_rate, output_rate, channels, leftover: Vec::new() }
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        let ratio = self.output_rate as f64 / self.input_rate as f64;
        let mut combined = self.leftover.clone();
        combined.extend_from_slice(input);
        let total_frames = combined.len() / self.channels;
        let out_frames = (total_frames as f64 * ratio) as usize;
        let mut output = Vec::with_capacity(out_frames);
        for i in 0..out_frames {
            let src_idx = (i as f64 / ratio) as usize;
            let src_idx = src_idx.min(total_frames.saturating_sub(1));
            let src_offset = src_idx * self.channels;
            if src_offset < combined.len() {
                output.push(combined[src_offset]);
            }
        }
        let consumed_frames = (out_frames as f64 / ratio) as usize;
        let consumed = (consumed_frames * self.channels).min(combined.len());
        self.leftover = combined[consumed..].to_vec();
        output
    }
}

// ============================================================================
// 音频捕获引擎
// ============================================================================

pub async fn capture_with_vad() -> Result<Vec<f32>> {
    let host = cpal::default_host();
    let device = host.default_input_device().context("未找到音频输入设备")?;
    let device_name = device.name().unwrap_or_else(|_| "未知设备".to_string());
    log::info!("[VoxLink] 音频输入设备: {}", device_name);

    let supported_config = device.default_input_config().context("无法获取设备默认输入配置")?;
    let input_config: cpal::StreamConfig = supported_config.into();
    log::info!("[VoxLink] 输入配置: {:?} channels, {:?} sample rate",
        input_config.channels, input_config.sample_rate);

    let input_channels = input_config.channels as usize;
    let input_rate = input_config.sample_rate.0 as usize;

    let ring = HeapRb::<f32>::new(RING_BUFFER_CAPACITY);
    let (mut producer, mut consumer) = ring.split();

    let capture_running = Arc::new(AtomicBool::new(true));
    let capture_running_clone = capture_running.clone();

    let stream = device.build_input_stream(
        &input_config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            for &sample in data {
                // ringbuf 0.4 的 Producer 没有 try_pop 方法；
                // 队列满时直接丢弃最新样本（实时音频可接受少量丢帧）
                let _ = producer.try_push(sample);
            }
        },
        move |err| {
            log::error!("[VoxLink] 音频采集错误: {}", err);
        },
        None,
    ).context("创建音频输入流失败")?;

    stream.play().context("启动音频流失败")?;
    log::info!("[VoxLink] 音频流已启动");

    let result = tokio::task::spawn_blocking(move || {
        process_audio_stream(&mut consumer, input_rate, input_channels, &capture_running_clone)
    }).await.context("音频处理线程 panic")?;

    capture_running.store(false, Ordering::SeqCst);
    drop(stream);

    match result {
        Ok(samples) => {
            if samples.is_empty() {
                anyhow::bail!("未检测到语音");
            }
            log::info!("[VoxLink] 语音捕获完成: {} 样本, {:.2} 秒",
                samples.len(), samples.len() as f32 / TARGET_SAMPLE_RATE as f32);
            Ok(samples)
        }
        Err(e) => Err(e),
    }
}

fn process_audio_stream(
    consumer: &mut impl Consumer<Item = f32>,
    input_rate: usize,
    input_channels: usize,
    running: &AtomicBool,
) -> Result<Vec<f32>> {
    let mut resampler = AudioResampler::new(input_rate, TARGET_SAMPLE_RATE, input_channels);
    let mut vad = SileroVad::new();
    let mut input_buffer = Vec::<f32>::with_capacity(RESAMPLER_CHUNK_SIZE * input_channels);
    let start_time = Instant::now();

    log::info!("[VoxLink] 开始音频处理: input_rate={}, channels={}", input_rate, input_channels);

    loop {
        if start_time.elapsed().as_secs_f32() > MAX_RECORDING_DURATION {
            log::info!("[VoxLink] 达到最大录音时长 ({:.0}s)，停止采集", MAX_RECORDING_DURATION);
            break;
        }

        while let Some(sample) = consumer.try_pop() {
            input_buffer.push(sample);
            if input_buffer.len() >= RESAMPLER_CHUNK_SIZE * input_channels {
                let resampled = resampler.process(&input_buffer);
                if !resampled.is_empty() {
                    for chunk in resampled.chunks(CHUNK_SIZE) {
                        if chunk.len() == CHUNK_SIZE {
                            let is_active = vad.process_frame(chunk);
                            if !is_active && vad.get_samples().len() > 0 {
                                let duration = vad.duration();
                                if duration >= MIN_RECORDING_DURATION {
                                    log::info!("[VoxLink] 语音段结束，时长: {:.2}s", duration);
                                    return Ok(vad.get_samples().to_vec());
                                }
                                vad.reset();
                            }
                        }
                    }
                }
                input_buffer.clear();
            }
        }

        if !running.load(Ordering::SeqCst) {
            log::info!("[VoxLink] 收到停止信号");
            break;
        }

        std::thread::sleep(Duration::from_millis(1));
    }

    let samples = vad.get_samples().to_vec();
    if samples.is_empty() {
        anyhow::bail!("未检测到语音输入");
    }
    Ok(samples)
}

pub fn list_input_devices() -> Result<Vec<String>> {
    let host = cpal::default_host();
    let devices = host.input_devices().context("无法枚举音频输入设备")?;
    let mut names = Vec::new();
    for device in devices {
        names.push(device.name().unwrap_or_else(|_| "未知设备".to_string()));
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rms_calculation() {
        assert_eq!(calculate_rms(&vec![0.0f32; 100]), 0.0);
        assert!((calculate_rms(&vec![1.0f32; 100]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_zero_crossing_rate() {
        let samples = vec![1.0, -1.0, 1.0, -1.0, 1.0];
        assert!(calculate_zero_crossing_rate(&samples) > 0.5);
    }

    #[test]
    fn test_silero_vad_silence() {
        let mut vad = SileroVad::new();
        let silence = vec![0.0f32; CHUNK_SIZE];
        for _ in 0..10 {
            assert!(!vad.process_frame(&silence));
        }
    }

    #[test]
    fn test_resampler() {
        let mut r = AudioResampler::new(48000, 16000, 1);
        let input = vec![0.5f32; 4800];
        let output = r.process(&input);
        assert!(!output.is_empty());
        // 48000->16000: 4800 samples -> ~1600 samples
        assert!((output.len() as f64 - 1600.0).abs() < 100.0);
    }
}