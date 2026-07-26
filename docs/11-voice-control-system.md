# 语音控制系统技术方案

## 1. 概述

通过语音识别 + 规则匹配意图理解，实现自然语言控制游戏多窗口操作。

### 核心目标
- 🎙️ 解放双手，纯语音操控多个游戏窗口
- 🎯 规则匹配指令（"窗口1跟随我"、"所有人停止"）
- 🔄 流畅的方案切换（战斗 → 采集 → 挂机）
- ⚡ 低延迟响应（< 3秒从语音到执行）

### 实现状态（2026-07-26）

✅ **Phase 1 (音频前端)**：麦克风采集 + 环形缓冲 + 唤醒词检测 + VAD 录音（已完成）
✅ **Phase 2 (ASR)**：百度短语音识别集成（已完成）
✅ **Phase 3 (意图识别 + 执行)**：规则匹配 + 脚本执行（已完成）

**当前状态**: 语音控制系统完全可用，支持生产环境部署。

---

## 1.1 麦克风共享与唤醒词（关键设计）

### 麦克风共享问题

游戏玩家通常同时开着语音软件（YY/Discord/微信语音），需要确认能否与本程序**共享麦克风**。

**结论：默认可以共享**
- Windows Vista 之后，WASAPI 默认使用**共享模式**（Shared Mode）
- 多个程序可同时监听同一麦克风，Windows 底层做混音分发
- `cpal` 默认走共享模式，与游戏语音软件不冲突

**例外情况**：
- 极少数软件使用**独占模式**（Exclusive Mode）抢占麦克风
- 游戏语音软件基本都用共享模式，冲突概率极低
- 若检测到设备无法打开，提示用户并建议改用独立麦克风

### 唤醒词方案（解决串音/误触发）

**问题**：与游戏语音共用麦克风时，存在两个隐患：
1. 队友说话进入麦克风，可能**误触发**指令
2. 自己的指令会通过游戏语音**传给队友**

**解决：唤醒词 + 短时指令窗口**

```
持续监听 → 检测唤醒词"小助手" → 打开 2-3 秒指令窗口 →
识别这段话 → 执行 → 关闭窗口回到待命状态
```

效果：
- ✅ 队友说话不会误触发（除非恰好说出唤醒词+指令，概率极低）
- ✅ 只有唤醒后的那句话被当作指令，其他音频直接丢弃
- ⚠️ 指令仍会通过游戏语音传给队友（同一麦克风的物理限制，无法完全避免）
  - 缓解：唤醒词设短，说话快，队友当作自言自语
  - 进阶（后期）：说指令时临时静音游戏语音（需与语音软件联动，复杂）

### 唤醒词技术选型

**方案 A：本地关键词检测（推荐）**
- 用轻量级本地模型持续监听，只识别唤醒词，不上传音频
- 库：`rustpotter`（纯 Rust）或 Porcupine
- 优点：省流量、低延迟、保护隐私、不消耗 ASR 额度
- 流程：本地检测唤醒词 → 唤醒后才调用百度/Whisper 识别完整指令

**方案 B：全程 ASR + 文本匹配**
- 持续将音频送 ASR，识别文本后检查是否以唤醒词开头
- 缺点：持续消耗 ASR 额度/算力，不划算

**采用方案 A**：`rustpotter` 本地唤醒词检测 + 百度/Whisper 指令识别

```toml
[dependencies]
rustpotter = "3"  # 本地唤醒词检测
```

### 完整音频状态机

```
        ┌──────────────┐
        │  待命监听      │◄─────────────┐
        │ (唤醒词检测)   │              │
        └──────┬───────┘              │
               │ 检测到"小助手"         │
               ▼                      │
        ┌──────────────┐              │
        │  指令录音      │              │
        │ (VAD 检测)    │              │
        └──────┬───────┘              │
               │ 语音结束 (静音 600ms) │
               ▼                      │
        ┌──────────────┐              │
        │  ASR 识别      │              │
        └──────┬───────┘              │
               │ 文本                  │
               ▼                      │
        ┌──────────────┐              │
        │ LLM 意图理解   │              │
        └──────┬───────┘              │
               │ 结构化指令            │
               ▼                      │
        ┌──────────────┐              │
        │  执行 + 反馈   │──────────────┘
        └──────────────┘
```

---

## 1.2 环形缓冲（防止指令音频丢失，必需）

### 为什么必需

唤醒词是**说完之后**才被检测到的，存在固有延迟：

```
时刻 0ms:   用户开始说 "小助手，开始打怪"
时刻 500ms: "小助手" 说完
时刻 700ms: 唤醒词检测器确认触发 ← 此刻才知道要开始听指令
时刻 500-700ms: "开始打..." 已经在说了！
```

如果**检测到唤醒词才开始录音**，指令开头（"开始打"）已经丢失，ASR 只能识别到残缺片段甚至失败。
用户常常连读（"小助手开始打怪"中间不停顿），不缓冲的话指令几乎全丢。

### 解决方案

**持续将音频写入固定长度的环形缓冲区**（保留最近 2-3 秒）。唤醒词触发时，从缓冲区**回溯**取出唤醒点之后的音频，保证指令开头不丢失。

```
┌─────────────────────────────────────────┐
│  RingBuffer (保留最近 3 秒音频)            │
│  [......旧......|小助手|开始打怪|...新...]  │
└─────────────────────────────────────────┘
                  ↑
         唤醒词触发时，从这个位置往后
         （往前留 200-300ms 余量，避免切掉指令开头）
         取音频送 ASR
```

### 设计要点

1. **缓冲长度**：2-3 秒覆盖"唤醒词 + 检测延迟"；可扩到 5 秒容纳连读的完整指令
2. **回溯起点**：从唤醒词结束点往后取，保险起见往前留 200-300ms 余量
3. **持续录音**：取出历史音频后，继续实时录音直到 VAD 检测到静音（600ms），拼接成完整指令
4. **无损**：即使唤醒词检测延迟 0.5-1 秒，指令音频始终躺在缓冲区，不会丢失

### 数据结构

```rust
use std::collections::VecDeque;

pub struct AudioRingBuffer {
    buffer: VecDeque<i16>,
    capacity: usize, // 如 16000 * 3 = 3 秒 @ 16kHz
}

impl AudioRingBuffer {
    pub fn new(sample_rate: usize, seconds: usize) -> Self {
        Self {
            buffer: VecDeque::new(),
            capacity: sample_rate * seconds,
        }
    }

    /// 持续写入音频帧，超出容量丢弃最旧数据
    pub fn push(&mut self, samples: &[i16]) {
        self.buffer.extend(samples);
        while self.buffer.len() > self.capacity {
            self.buffer.pop_front();
        }
    }

    /// 唤醒触发后，取出最近 N 毫秒的音频作为指令起点
    pub fn take_recent(&self, ms: usize, sample_rate: usize) -> Vec<i16> {
        let n = ms * sample_rate / 1000;
        let start = self.buffer.len().saturating_sub(n);
        self.buffer.iter().skip(start).copied().collect()
    }
}
```

### 修正后的音频流水线

```
麦克风 → 环形缓冲 (持续写入，保留最近 3 秒)
                    │
                    ├─→ 唤醒词检测器 (实时消费每一帧)
                    │        │ 触发
                    │        ▼
                    │   从环形缓冲回溯取音频 (唤醒点往前留余量)
                    │        + 继续实时录音直到 VAD 静音
                    │        ▼
                    │   拼接成完整指令音频 → ASR
```

---

## 1.3 交互细节（已确定的设计决策）

### 环形缓冲取音频策略（方案 A）
- 唤醒触发时，以触发时刻为切分点，**往前留 100ms 余量**作为指令音频起点
- 只把唤醒词**之后**的音频送 ASR（不含唤醒词本身）
- 对唤醒词检测器的精度不敏感：若实测指令开头偶尔被切，调大余量即可

### 唤醒词训练（用户本人录制）
- 采用**首次使用引导录制**，而非通用预训练模型
- 流程：设置中点"录制唤醒词" → 引导念 3-5 遍"小助手"（每遍单独录制 wav）
  → rustpotter 训练出 `.rpw` 模型 → 保存本地 → 启动时加载
- 支持**重新录制**（换环境/换人时）
- 优点：识别的是用户本人声音说的词，准确率高、误报低

### 超时控制
- 唤醒后 **3 秒**内未检测到有效语音（VAD 持续静音）→ 超时，回待命
- 开始说话后，整段指令**最长 8 秒**，超过强制截断送 ASR

### 声音反馈（wav 音效 + PlaySound）
- 使用 Windows `PlaySound` API 播放（零额外依赖）
- **不在唤醒时反馈**，仅在以下时机：
  - 指令识别成功（拿到文本准备执行）→ 播放**成功音效**（升调，愉快）
  - 超时/识别失败 → 播放**失败音效**（降调，提示未成功）
- 音效文件：`assets/beep_success.wav` / `assets/beep_fail.wav`

---

## 2. 技术架构

```
┌─────────────┐
│   麦克风     │
└──────┬──────┘
       │ 音频流（WASAPI 共享模式，与游戏语音共存）
       ▼
┌─────────────────────────────────┐
│   音频处理层 (Audio Pipeline)    │
│  - 音频采集 (cpal, 共享模式)     │
│  - 环形缓冲 (保留最近3秒，防丢失) │
│  - 唤醒词检测 (rustpotter)       │
│  - VAD 静音检测 (webrtc-vad)    │
│  - 格式转换 (PCM 16kHz)         │
└──────┬──────────────────────────┘
       │ 唤醒后的指令音频段落（含回溯）
       ▼
┌─────────────────────────────────┐
│   ASR 识别层 (双模式)            │
│  ┌────────────┐  ┌────────────┐ │
│  │ 在线模式    │  │ 离线模式    │ │
│  │ 百度语音    │  │ Whisper    │ │
│  │ HTTP API   │  │ 本地推理   │ │
│  └────────────┘  └────────────┘ │
└──────┬──────────────────────────┘
       │ 识别文本
       ▼
┌─────────────────────────────────┐
│   意图理解层 (LLM)               │
│  - 调用 GPT/Claude/本地模型     │
│  - 解析用户意图                  │
│  - 提取参数（窗口号、方案名）    │
└──────┬──────────────────────────┘
       │ 结构化指令
       ▼
┌─────────────────────────────────┐
│   指令调度层 (Dispatcher)        │
│  - 映射到脚本方案                │
│  - 窗口选择                      │
│  - 执行控制（启动/停止/切换）    │
└──────┬──────────────────────────┘
       │ 脚本执行
       ▼
┌─────────────────────────────────┐
│   游戏窗口 (1-8)                 │
└─────────────────────────────────┘
```

---

## 3. ASR 识别方案

### 3.1 双模式设计

```rust
pub enum AsrMode {
    /// 在线模式：百度语音识别
    Online(BaiduAsr),
    /// 离线模式：Whisper 本地推理
    Offline(WhisperAsr),
}

pub trait AsrProvider: Send + Sync {
    /// 识别音频数据，返回文本
    fn recognize(&self, audio: &[i16]) -> Result<String, AsrError>;
    
    /// 是否支持流式识别
    fn supports_streaming(&self) -> bool;
    
    /// 模式名称（用于 UI 显示）
    fn name(&self) -> &str;
}
```

### 3.2 百度语音（在线模式）

**技术选型**：
- API：百度短语音识别 REST API
- HTTP 客户端：`reqwest`
- 音频格式：PCM 16kHz 单声道

**优势**：
- ✅ 免费额度：50,000 次/天
- ✅ 低延迟：200-400ms
- ✅ 高准确率：中文 96%+
- ✅ 简单易用：HTTP POST 调用

**依赖**：
```toml
[dependencies]
reqwest = { version = "0.11", features = ["json"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
base64 = "0.21"
```

**实现要点**：
```rust
pub struct BaiduAsr {
    app_key: String,
    secret_key: String,
    access_token: String,
    token_expires: Instant,
}

impl BaiduAsr {
    /// 获取 Access Token（有效期 30 天）
    async fn refresh_token(&mut self) -> Result<()> {
        let url = format!(
            "https://aip.baidubce.com/oauth/2.0/token?grant_type=client_credentials&client_id={}&client_secret={}",
            self.app_key, self.secret_key
        );
        let resp: TokenResponse = reqwest::get(&url).await?.json().await?;
        self.access_token = resp.access_token;
        self.token_expires = Instant::now() + Duration::from_secs(30 * 24 * 3600);
        Ok(())
    }
    
    /// 识别音频
    async fn recognize(&self, audio_pcm: &[i16]) -> Result<String> {
        // 转换为字节数组
        let audio_bytes: Vec<u8> = audio_pcm.iter()
            .flat_map(|&s| s.to_le_bytes())
            .collect();
        
        // Base64 编码
        let audio_base64 = base64::encode(&audio_bytes);
        
        // 构造请求
        let request = BaiduAsrRequest {
            format: "pcm",
            rate: 16000,
            channel: 1,
            cuid: "game_controller",
            token: &self.access_token,
            speech: audio_base64,
            len: audio_bytes.len(),
        };
        
        // 发送请求
        let resp: BaiduAsrResponse = reqwest::Client::new()
            .post("https://vop.baidu.com/server_api")
            .json(&request)
            .send()
            .await?
            .json()
            .await?;
        
        if resp.err_no == 0 && !resp.result.is_empty() {
            Ok(resp.result[0].clone())
        } else {
            Err(AsrError::RecognitionFailed(resp.err_msg))
        }
    }
}
```

**API 文档**：https://ai.baidu.com/ai-doc/SPEECH/Vk38lxily

---

### 3.3 Whisper（离线模式）

**技术选型**：
- 实现：`whisper-rs`（Rust 绑定 whisper.cpp）
- 模型：ggml 格式量化模型
- 推荐模型：`ggml-base.bin`（143MB，准确率 vs 速度平衡）

**优势**：
- ✅ 完全离线，无需网络
- ✅ 隐私保护，数据不上传
- ✅ 无使用限制
- ✅ 多语言支持

**劣势**：
- ❌ 延迟较高：base 模型 ~500ms
- ❌ 资源占用：需要加载模型（100MB+ 内存）
- ❌ 需要手动 VAD

**依赖**：
```toml
[dependencies]
whisper-rs = "0.10"
```

**实现要点**：
```rust
pub struct WhisperAsr {
    ctx: WhisperContext,
    params: FullParams<'static>,
}

impl WhisperAsr {
    pub fn new(model_path: &str) -> Result<Self> {
        // 加载模型
        let ctx = WhisperContext::new(model_path)
            .map_err(|e| AsrError::ModelLoadFailed(e.to_string()))?;
        
        // 配置参数
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("zh")); // 中文
        params.set_print_progress(false);
        params.set_print_special(false);
        params.set_print_realtime(false);
        
        Ok(Self { ctx, params })
    }
    
    pub fn recognize(&self, audio_pcm: &[i16]) -> Result<String> {
        // Whisper 需要 f32 格式
        let audio_f32: Vec<f32> = audio_pcm.iter()
            .map(|&s| s as f32 / 32768.0)
            .collect();
        
        // 推理
        self.ctx.full(self.params.clone(), &audio_f32)
            .map_err(|e| AsrError::RecognitionFailed(e.to_string()))?;
        
        // 提取文本
        let num_segments = self.ctx.full_n_segments()
            .map_err(|e| AsrError::RecognitionFailed(e.to_string()))?;
        
        let mut result = String::new();
        for i in 0..num_segments {
            if let Ok(text) = self.ctx.full_get_segment_text(i) {
                result.push_str(&text);
            }
        }
        
        Ok(result.trim().to_string())
    }
}
```

**模型下载**：
- Base 模型（143MB）：https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin
- Small 模型（488MB）：https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin
- 放置在：`models/ggml-base.bin`

---

## 4. 音频处理

### 4.1 音频采集

**技术选型**：`cpal`（跨平台音频库）

```toml
[dependencies]
cpal = "0.15"
```

**实现**：
```rust
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub struct AudioCapture {
    stream: cpal::Stream,
    buffer: Arc<Mutex<Vec<i16>>>,
}

impl AudioCapture {
    pub fn new() -> Result<Self> {
        let host = cpal::default_host();
        let device = host.default_input_device()
            .ok_or(AudioError::NoInputDevice)?;
        
        let config = device.default_input_config()?;
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let buffer_clone = buffer.clone();
        
        let stream = device.build_input_stream(
            &config.into(),
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                buffer_clone.lock().unwrap().extend_from_slice(data);
            },
            |err| eprintln!("音频采集错误: {}", err),
            None,
        )?;
        
        stream.play()?;
        
        Ok(Self { stream, buffer })
    }
    
    pub fn read_samples(&self, duration_ms: u64) -> Vec<i16> {
        std::thread::sleep(Duration::from_millis(duration_ms));
        let mut buffer = self.buffer.lock().unwrap();
        let samples = buffer.clone();
        buffer.clear();
        samples
    }
}
```

### 4.2 VAD 静音检测

**技术选型**：`webrtc-vad`（WebRTC 的 VAD 实现）

```toml
[dependencies]
webrtc-vad = "0.4"
```

**实现**：
```rust
use webrtc_vad::{Vad, SampleRate, VadMode};

pub struct VoiceActivityDetector {
    vad: Vad,
    speech_buffer: Vec<i16>,
    silence_frames: usize,
}

impl VoiceActivityDetector {
    pub fn new() -> Result<Self> {
        let mut vad = Vad::new();
        vad.set_mode(VadMode::Aggressive); // 激进模式，快速检测
        vad.set_sample_rate(SampleRate::Rate16kHz);
        
        Ok(Self {
            vad,
            speech_buffer: Vec::new(),
            silence_frames: 0,
        })
    }
    
    /// 处理音频帧，返回完整的语音段落
    pub fn process_frame(&mut self, samples: &[i16]) -> Option<Vec<i16>> {
        let is_speech = self.vad.is_voice_segment(samples).unwrap_or(false);
        
        if is_speech {
            self.speech_buffer.extend_from_slice(samples);
            self.silence_frames = 0;
        } else if !self.speech_buffer.is_empty() {
            self.silence_frames += 1;
            
            // 连续 30 帧静音（约 600ms）认为语音结束
            if self.silence_frames > 30 {
                let result = self.speech_buffer.clone();
                self.speech_buffer.clear();
                self.silence_frames = 0;
                return Some(result);
            }
        }
        
        None
    }
}
```

---

## 5. 运行时模式切换

### 5.1 配置结构

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    /// 当前 ASR 模式
    pub asr_mode: AsrModeConfig,
    
    /// 百度语音配置
    pub baidu: Option<BaiduConfig>,
    
    /// Whisper 配置
    pub whisper: Option<WhisperConfig>,
    
    /// 是否启用语音控制
    pub enabled: bool,
    
    /// 唤醒词（可选）
    pub wake_word: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AsrModeConfig {
    Online,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaiduConfig {
    pub app_key: String,
    pub secret_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperConfig {
    pub model_path: String,
}
```

### 5.2 动态切换实现

```rust
pub struct VoiceController {
    config: Arc<RwLock<VoiceConfig>>,
    asr: Arc<RwLock<Box<dyn AsrProvider>>>,
    audio_capture: AudioCapture,
    vad: VoiceActivityDetector,
}

impl VoiceController {
    /// 切换 ASR 模式
    pub async fn switch_mode(&self, mode: AsrModeConfig) -> Result<()> {
        let mut config = self.config.write().unwrap();
        config.asr_mode = mode.clone();
        
        // 重新初始化 ASR
        let new_asr: Box<dyn AsrProvider> = match mode {
            AsrModeConfig::Online => {
                let baidu_config = config.baidu.as_ref()
                    .ok_or(VoiceError::ConfigMissing("百度配置缺失"))?;
                Box::new(BaiduAsr::new(baidu_config).await?)
            }
            AsrModeConfig::Offline => {
                let whisper_config = config.whisper.as_ref()
                    .ok_or(VoiceError::ConfigMissing("Whisper 配置缺失"))?;
                Box::new(WhisperAsr::new(&whisper_config.model_path)?)
            }
        };
        
        *self.asr.write().unwrap() = new_asr;
        
        println!("✅ 已切换到 {} 模式", match mode {
            AsrModeConfig::Online => "在线",
            AsrModeConfig::Offline => "离线",
        });
        
        Ok(())
    }
    
    /// 主循环：音频采集 → VAD → ASR → 意图理解 → 执行
    pub async fn run(&self) -> Result<()> {
        loop {
            // 1. 采集音频帧（20ms）
            let samples = self.audio_capture.read_samples(20);
            
            // 2. VAD 检测
            if let Some(speech_segment) = self.vad.process_frame(&samples) {
                // 3. ASR 识别
                let asr = self.asr.read().unwrap();
                let text = asr.recognize(&speech_segment)?;
                
                if text.is_empty() {
                    continue;
                }
                
                println!("🎤 识别: {}", text);
                
                // 4. 意图理解（调用 LLM）
                let intent = self.understand_intent(&text).await?;
                
                // 5. 执行指令
                self.dispatch_command(intent).await?;
            }
        }
    }
}
```

### 5.3 UI 切换控制

```rust
// 在 App UI 中添加切换按钮
impl App {
    fn ui_voice_control_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("🎙️ 语音控制");
        
        ui.horizontal(|ui| {
            ui.label("ASR 模式:");
            
            let current_mode = self.voice_controller.get_mode();
            
            if ui.selectable_label(
                matches!(current_mode, AsrModeConfig::Online),
                "在线（百度）"
            ).clicked() {
                self.voice_controller.switch_mode(AsrModeConfig::Online);
            }
            
            if ui.selectable_label(
                matches!(current_mode, AsrModeConfig::Offline),
                "离线（Whisper）"
            ).clicked() {
                self.voice_controller.switch_mode(AsrModeConfig::Offline);
            }
        });
        
        ui.separator();
        
        // 状态显示
        let status = self.voice_controller.get_status();
        ui.label(format!("状态: {}", status));
        
        if status.is_listening {
            ui.colored_label(egui::Color32::GREEN, "● 正在监听");
        }
    }
}
```

---

## 6. 意图理解（LLM 集成）

### 6.1 结构化输出

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct UserIntent {
    /// 意图类型
    pub intent_type: IntentType,
    
    /// 目标窗口（空表示全部）
    pub target_windows: Vec<u8>,
    
    /// 方案名称（用于切换方案）
    pub scheme_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum IntentType {
    /// 启动循环执行
    Start,
    /// 停止执行
    Stop,
    /// 单次执行
    RunOnce,
    /// 切换方案
    SwitchScheme,
    /// 即兴发送按键
    SendKey { key: String },
    /// 未知意图
    Unknown,
}
```

### 6.2 LLM Prompt 设计

```rust
const INTENT_SYSTEM_PROMPT: &str = r#"
你是一个游戏多窗口控制助手。用户会用自然语言描述操作意图，你需要解析为结构化指令。

可用窗口：1-8 号
可用方案：combat（战斗）、gather（采集）、idle（挂机）、dungeon（副本）

输出 JSON 格式：
{
  "intent_type": "Start | Stop | RunOnce | SwitchScheme | SendKey | Unknown",
  "target_windows": [1, 2, 3],  // 空数组表示全部窗口
  "scheme_name": "combat",       // 仅 SwitchScheme 需要
  "key": "H"                     // 仅 SendKey 需要
}

示例：
用户："开始打怪"
输出：{"intent_type": "Start", "target_windows": [], "scheme_name": null}

用户："让1号和3号去采集"
输出：{"intent_type": "SwitchScheme", "target_windows": [1, 3], "scheme_name": "gather"}

用户："停一下"
输出：{"intent_type": "Stop", "target_windows": [], "scheme_name": null}
"#;

pub async fn understand_intent(text: &str) -> Result<UserIntent> {
    let client = reqwest::Client::new();
    
    let request = serde_json::json!({
        "model": "gpt-3.5-turbo",
        "messages": [
            {"role": "system", "content": INTENT_SYSTEM_PROMPT},
            {"role": "user", "content": text}
        ],
        "temperature": 0.3,
        "response_format": { "type": "json_object" }
    });
    
    let resp = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&request)
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;
    
    let content = resp["choices"][0]["message"]["content"]
        .as_str()
        .ok_or(VoiceError::LlmParseFailed)?;
    
    let intent: UserIntent = serde_json::from_str(content)?;
    Ok(intent)
}
```

---

## 7. 指令调度

```rust
impl VoiceController {
    /// 执行用户意图
    async fn dispatch_command(&self, intent: UserIntent) -> Result<()> {
        match intent.intent_type {
            IntentType::Start => {
                if intent.target_windows.is_empty() {
                    self.app.start_all();
                } else {
                    self.app.start_windows(&intent.target_windows);
                }
                println!("✅ 已启动窗口");
            }
            
            IntentType::Stop => {
                if intent.target_windows.is_empty() {
                    self.app.stop_all();
                } else {
                    self.app.stop_windows(&intent.target_windows);
                }
                println!("✅ 已停止窗口");
            }
            
            IntentType::SwitchScheme => {
                let scheme_name = intent.scheme_name
                    .ok_or(VoiceError::MissingParameter("scheme_name"))?;
                
                self.app.switch_scheme(&intent.target_windows, &scheme_name)?;
                println!("✅ 已切换到方案: {}", scheme_name);
            }
            
            IntentType::SendKey { key } => {
                self.app.send_key_to_windows(&intent.target_windows, &key)?;
                println!("✅ 已发送按键: {}", key);
            }
            
            IntentType::Unknown => {
                println!("❌ 无法理解指令");
            }
        }
        
        Ok(())
    }
}
```

---

## 8. Phase 3 实现：规则匹配意图识别（已完成）

### 8.1 指令规则

Phase 3 采用**规则匹配**而非 LLM，降低成本和延迟。支持以下指令：

1. **执行动作**：`"窗口1跟随我"` → 在窗口1执行包含"跟随"的脚本
2. **执行动作**：`"窗口1快加血"` → 在窗口1执行包含"加血"的脚本
3. **停止全部**：`"所有人停止"` / `"所有窗口停止执行"`
4. **停止指定窗口**：`"窗口1停止"` / `"窗口1停止执行"`

### 8.2 意图解析器（`src/voice/intent.rs`）

**核心逻辑**：

```rust
pub enum VoiceIntent {
    StopAll,
    StopWindow(usize),
    RunAction { window: usize, action: String },
}

pub fn parse_intent(text: &str, windows: &[(usize, String)]) -> Option<VoiceIntent>
```

**处理流程**：
1. **文本规范化**：去空白、剥离"小助手"前缀、去标点
2. **中文数字归一化**：`"窗口一"` → `"窗口1"`（解决百度 ASR 识别偏差）
3. **窗口名匹配**：按长度降序匹配，避免"窗口1"误中"窗口11"
4. **动作提取**：窗口名之后的文本作为动作关键词
5. **停止判断**：含"停止/暂停"且带"所有/全部" → StopAll

**脚本匹配**（`match_script`）：
- 脚本名去扩展名后作为主名：`"跟随.ag"` → `"跟随"`
- 动作文本包含主名即命中：`"跟随我"` 包含 `"跟随"` ✓
- 多个命中取主名最长者（更精确）

**脚本设置**（`setting` 指令）：
- 脚本可通过 `setting(audio_onlyonce)` 声明语音场景下的执行模式
- 语音触发时检查该设置，决定是**循环执行**还是**单次执行**
- 适合"跟随"、"加血"等只需执行一次的动作

```ag
// 跟随.ag - 语音单次执行示例
setting(audio_onlyonce)

click(1)
delay_ms(500)
```

| 设置 | 作用 |
|------|------|
| `setting(audio_onlyonce)` | 语音场景下仅执行一次（执行一轮后自动停止） |

### 8.3 语音运行时（`src/voice/runtime.rs`）

后台线程运行完整流水线，通过 channel 回传事件：

```rust
pub enum VoiceEvent {
    Status(String),    // 一般状态（待命/超时等）
    Woke,              // 唤醒
    Recognized(String), // 识别文本
    Error(String),     // 错误
    Stopped,           // 线程已停
}

pub struct VoiceRuntime {
    pub fn start(config: VoiceConfig) -> Self;
    pub fn poll(&self) -> Vec<VoiceEvent>;
    pub fn stop(&mut self);
}
```

**状态机**：
```
Idle(唤醒词检测) → Woke → 回溯2秒 → Listening(VAD录音)
  → 录音完成 → ASR识别 → 发送Recognized事件 → 回到Idle
```

### 8.4 App 集成（`src/app.rs`）

**配置扩展**：
- `SlotConfig` 新增 `name: String` 字段（自定义窗口名）
- `BaiduConfig` 存储 API Key / Secret Key

**核心方法**：
- `start_voice()` / `stop_voice()`：开关语音控制
- `process_voice()`：轮询事件，收到 `Recognized` 调用 `handle_voice_text()`
- `handle_voice_text()`：调用 `parse_intent()` 解析，分发到执行逻辑
- `run_voice_action()`：按动作关键词匹配脚本 → 设标识 → 启动槽位

**UI 扩展**：
- 工具栏：`"🎤 语音: 开/关"` 切换按钮 + `"⚙ 语音设置"` 窗口
- 语音设置窗口：百度密钥编辑、模型状态、最近识别文本、使用说明
- 槽位标题行：可编辑窗口名（默认"窗口1~8"，可改"主号"等）

### 8.5 调试日志（`src/voice/vlog.rs`）

全链路日志输出到控制台 + `voice_debug.log`：

```
[voice] 唤醒 score=0.550，开始听指令
[voice] 回溯补齐 32000 样本(2.00秒)
[voice] VAD 检测到静音，录音结束
[voice] 指令音频 48000 样本(3.00秒)，开始 ASR 识别...
[voice] ASR 识别结果: 「小助手窗口一跟随我」
[intent] 原始文本: 「小助手窗口一跟随我」
[intent] 当前窗口名: ["1=窗口1", "2=窗口2", ...]
[intent] 匹配: 窗口 1 执行动作「跟随我」
[intent] 窗口 1(窗口1) 已添加脚本: ["跟随.ag", "加血.ag"]
[intent] 动作「跟随我」匹配到脚本「跟随.ag」，启动
[intent] 已启动窗口 1 的脚本「跟随.ag」
```

**使用**：
```bash
cargo run
# 测试后查看日志
cat voice_debug.log
```

详细排查指南见 [VOICE_DEBUG.md](../VOICE_DEBUG.md)

### 8.6 中文数字处理

**问题**：百度 ASR 常把"窗口1"识别为"窗口一"，导致匹配失败

**解决**：`normalize()` 函数统一转换：
```rust
s = s.replace("一", "1")
     .replace("二", "2")
     // ... 零到九全部映射
```

现在 `"窗口一跟随我"` 和 `"窗口1跟随我"` 都能正确匹配。

### 8.7 使用流程

1. **训练唤醒词**（首次）：运行 `wakeword_test` 录制4遍"小助手" → 生成 `wakeword_model.rpw`
2. **配置密钥**：打开 "⚙ 语音设置"，填入百度 API Key / Secret Key，保存
3. **准备环境**：
   - 窗口1抓取游戏窗口
   - 添加脚本（如 `跟随.ag`、`加血.ag`）
   - 可编辑窗口名（点标题行文本框）
4. **开启语音**：点击 "🎤 语音: 关" 切换为开启
5. **语音指令**：说 `"小助手，窗口1跟随我"` → 自动执行脚本

---

## 9. 实施计划（更新）

### Phase 1: 音频基础（已完成 ✅）
- ✅ 集成 `cpal` 实现音频采集（共享模式）
- ✅ 实现环形缓冲 `AudioRingBuffer`（保留最近 3 秒，防指令丢失）
- ✅ 集成 `rustpotter` 唤醒词检测（训练+检测）
- ✅ 集成 `webrtc-vad` 实现 VAD
- ✅ 音频预处理（重采样、格式转换）
- ✅ 打通音频状态机：待命 → 唤醒 → 回溯取音频 → 录音 → 输出

### Phase 2: ASR（已完成 ✅）
- ✅ 百度语音 API 封装
  - Token 获取和刷新（30天缓存）
  - 音频识别接口
- ⏸️ Whisper 本地推理（暂缓，需 LLVM 编译环境）

### Phase 3: 意图识别 + 执行（已完成 ✅）
- ✅ 规则匹配意图解析器
- ✅ 中文数字归一化
- ✅ 脚本名包含匹配
- ✅ 语音运行时后台线程
- ✅ App 集成 + UI
- ✅ 可编辑窗口名 + 配置持久化
- ✅ 全链路调试日志

### Phase 4: 优化与扩展（待定）
- [ ] Whisper 离线 ASR（需解决 LLVM 依赖）
- [ ] 更多指令类型（单次执行、发送按键等）
- [ ] 语音反馈（TTS 播报执行结果）
- [ ] 多轮对话支持

### Phase 4: 后续优化（待定）
- [ ] Whisper 离线 ASR（需解决 LLVM 依赖）
- [ ] 更多指令类型（单次执行、发送按键）
- [ ] 语音反馈（TTS 播报结果）
- [ ] 多轮对话

**实际完成时间**: Phase 1-3 约 10 天（2026-07-15 至 2026-07-26）

---

## 9. 依赖清单

```toml
[dependencies]
# 音频处理
cpal = "0.15"
webrtc-vad = "0.4"

# ASR
reqwest = { version = "0.11", features = ["json"] }
whisper-rs = "0.10"

# 通用
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
base64 = "0.21"
tokio = { version = "1", features = ["full"] }
```

---

## 10. 配置示例

```toml
# config/voice.toml

[voice]
enabled = true
asr_mode = "Online"  # "Online" | "Offline"
wake_word = "小助手"  # 可选

[voice.baidu]
app_key = "your_baidu_app_key"
secret_key = "your_baidu_secret_key"

[voice.whisper]
model_path = "models/ggml-base.bin"

[voice.llm]
provider = "OpenAI"  # "OpenAI" | "Claude" | "Local"
api_key = "your_openai_key"
model = "gpt-3.5-turbo"
```

---

## 11. 测试计划

### 单元测试
- [ ] 音频采集正确性
- [ ] VAD 检测准确率
- [ ] ASR 识别准确率
- [ ] 意图解析准确率

### 集成测试
- [ ] 端到端流程（语音 → 执行）
- [ ] 模式切换稳定性
- [ ] 错误恢复能力

### 性能测试
- [ ] 识别延迟（目标 < 1s）
- [ ] 内存占用（Whisper 模型）
- [ ] CPU 占用率

---

## 12. 风险和挑战

### 技术风险
1. **Whisper 延迟**：离线模式可能慢于预期
   - 缓解：使用更小的模型（tiny/base）
   - 备选：切换到在线模式

2. **LLM 理解准确率**：复杂指令可能解析错误
   - 缓解：优化 Prompt，增加示例
   - 备选：先实现简单规则匹配

3. **网络依赖**：在线模式依赖网络稳定性
   - 缓解：增加重试和超时控制
   - 备选：自动切换到离线模式

### 用户体验
1. **唤醒词误触发**：环境噪音可能触发
   - 缓解：调整 VAD 灵敏度
   - 备选：添加手动开关

2. **识别错误**：口音、环境噪音影响
   - 缓解：提供纠错机制
   - 备选：显示识别结果让用户确认

---

## 13. 未来扩展

- [ ] **离线 LLM**：集成 llama.cpp + Qwen 模型
- [ ] **TTS 反馈**：语音播报执行结果
- [ ] **对话上下文**：记住用户习惯和历史指令
- [ ] **自定义唤醒词**：训练专属唤醒模型
- [ ] **多语言支持**：英文、日文等

---

**文档版本**: v1.0  
**最后更新**: 2024-01
