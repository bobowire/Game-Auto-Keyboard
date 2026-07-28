// 麦克风音频采集 - 用 cpal 共享模式采集，统一转为 i16 单声道 16kHz
//
// cpal 的采集在独立回调线程，通过 channel 把处理好的音频帧送出。

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use crossbeam_channel::{Receiver, Sender};

use crate::voice::dsp::PreFilter;

/// 目标采样率（唤醒词检测和 ASR 通用）
pub const TARGET_SAMPLE_RATE: u32 = 16000;

pub struct AudioCapture {
    _stream: Stream,
    rx: Receiver<Vec<i16>>,
    /// 实际输入采样率（用于调试）
    input_sample_rate: u32,
}

impl AudioCapture {
    /// 打开默认输入设备开始采集。返回采集器（持有 stream，drop 即停止）
    pub fn start() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("未找到麦克风输入设备")?;

        let config = device
            .default_input_config()
            .map_err(|e| format!("获取输入配置失败: {}", e))?;

        let input_sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let sample_format = config.sample_format();

        let (tx, rx) = crossbeam_channel::unbounded::<Vec<i16>>();

        let stream_config: cpal::StreamConfig = config.into();

        // 根据设备采样格式构建对应的采集流
        let stream = match sample_format {
            SampleFormat::F32 => build_stream_f32(&device, &stream_config, channels, input_sample_rate, tx)?,
            SampleFormat::I16 => build_stream_i16(&device, &stream_config, channels, input_sample_rate, tx)?,
            SampleFormat::U16 => build_stream_u16(&device, &stream_config, channels, input_sample_rate, tx)?,
            other => return Err(format!("不支持的采样格式: {:?}", other)),
        };

        stream.play().map_err(|e| format!("启动采集失败: {}", e))?;

        Ok(Self {
            _stream: stream,
            rx,
            input_sample_rate,
        })
    }

    /// 非阻塞取出已采集的音频帧（已转为 i16 单声道 16kHz）
    pub fn poll(&self) -> Vec<i16> {
        let mut out = Vec::new();
        while let Ok(frame) = self.rx.try_recv() {
            out.extend(frame);
        }
        out
    }

    pub fn input_sample_rate(&self) -> u32 {
        self.input_sample_rate
    }
}

/// 把多声道交错样本降为单声道（取各声道平均）
fn to_mono(samples: &[i16], channels: usize) -> Vec<i16> {
    if channels <= 1 {
        return samples.to_vec();
    }
    samples
        .chunks(channels)
        .map(|frame| {
            let sum: i32 = frame.iter().map(|&s| s as i32).sum();
            (sum / channels as i32) as i16
        })
        .collect()
}

/// 线性重采样到目标采样率（简单最近邻/线性插值，够用于语音）
fn resample(input: &[i16], from_rate: u32, to_rate: u32) -> Vec<i16> {
    if from_rate == to_rate || input.is_empty() {
        return input.to_vec();
    }
    let ratio = to_rate as f64 / from_rate as f64;
    let out_len = (input.len() as f64 * ratio) as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 / ratio;
        let idx = src_pos as usize;
        if idx + 1 < input.len() {
            // 线性插值
            let frac = src_pos - idx as f64;
            let a = input[idx] as f64;
            let b = input[idx + 1] as f64;
            out.push((a + (b - a) * frac) as i16);
        } else if idx < input.len() {
            out.push(input[idx]);
        }
    }
    out
}

/// 处理一帧：转单声道 → 抗混叠 → 重采样 → 去低频轰鸣 → RNNoise 降噪，然后发送
///
/// 滤波顺序很关键：
/// 1. 抗混叠必须在重采样**之前**，否则高频已经折叠进语音频段，事后再滤也救不回来
/// 2. RNNoise 在最后，确保输入已经是干净的 16kHz 音频
///
/// RNNoise 按 10ms 整帧工作，所以本次回调可能只吐出一部分样本、剩下的留在
/// 滤波器内部等下一次回调。发送的是降噪输出本身，不能是原始切片——否则尾部
/// 会混进未降噪的数据。降噪输出为空（攒不满一帧）时这次就不发。
fn process_and_send(
    samples: Vec<i16>,
    channels: usize,
    input_rate: u32,
    filter: &mut PreFilter,
    denoised: &mut Vec<i16>,
    tx: &Sender<Vec<i16>>,
) {
    let mut mono = to_mono(&samples, channels);
    filter.apply_anti_alias(&mut mono);
    let mut resampled = resample(&mono, input_rate, TARGET_SAMPLE_RATE);
    if resampled.is_empty() {
        return;
    }
    filter.apply_rumble_filter(&mut resampled);
    if filter.apply_denoise(&resampled, denoised) > 0 {
        let _ = tx.send(denoised.clone());
    }
}

fn build_stream_f32(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    input_rate: u32,
    tx: Sender<Vec<i16>>,
) -> Result<Stream, String> {
    let mut filter = PreFilter::new(input_rate, TARGET_SAMPLE_RATE);
    // 降噪输出缓冲，复用避免在音频回调里反复分配
    let mut denoised = Vec::new();
    device
        .build_input_stream(
            config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let samples: Vec<i16> = data
                    .iter()
                    .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                    .collect();
                process_and_send(samples, channels, input_rate, &mut filter, &mut denoised, &tx);
            },
            |err| eprintln!("音频采集错误: {}", err),
            None,
        )
        .map_err(|e| format!("构建采集流失败: {}", e))
}

fn build_stream_i16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    input_rate: u32,
    tx: Sender<Vec<i16>>,
) -> Result<Stream, String> {
    let mut filter = PreFilter::new(input_rate, TARGET_SAMPLE_RATE);
    let mut denoised = Vec::new();
    device
        .build_input_stream(
            config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                process_and_send(
                    data.to_vec(),
                    channels,
                    input_rate,
                    &mut filter,
                    &mut denoised,
                    &tx,
                );
            },
            |err| eprintln!("音频采集错误: {}", err),
            None,
        )
        .map_err(|e| format!("构建采集流失败: {}", e))
}

fn build_stream_u16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    input_rate: u32,
    tx: Sender<Vec<i16>>,
) -> Result<Stream, String> {
    let mut filter = PreFilter::new(input_rate, TARGET_SAMPLE_RATE);
    let mut denoised = Vec::new();
    device
        .build_input_stream(
            config,
            move |data: &[u16], _: &cpal::InputCallbackInfo| {
                let samples: Vec<i16> = data
                    .iter()
                    .map(|&s| (s as i32 - 32768) as i16)
                    .collect();
                process_and_send(samples, channels, input_rate, &mut filter, &mut denoised, &tx);
            },
            |err| eprintln!("音频采集错误: {}", err),
            None,
        )
        .map_err(|e| format!("构建采集流失败: {}", e))
}
