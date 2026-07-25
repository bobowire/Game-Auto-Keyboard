# 语音控制系统技术方案

## 1. 概述

通过语音识别 + AI 意图理解，实现自然语言控制游戏多窗口操作，支持复杂指令、方案切换和智能调度。

### 核心目标
- 🎙️ 解放双手，纯语音操控多个游戏窗口
- 🤖 AI 理解复杂指令（"让1到4号打怪，其他挂机"）
- 🔄 流畅的方案切换（战斗 → 采集 → 挂机）
- ⚡ 低延迟响应（< 1秒从语音到执行）
- 🔌 运行时动态切换在线/离线模式

---

## 2. 技术架构

```
┌─────────────┐
│   麦克风     │
└──────┬──────┘
       │ 音频流
       ▼
┌─────────────────────────────────┐
│   音频处理层 (Audio Pipeline)    │
│  - 音频采集 (cpal)               │
│  - VAD 静音检测 (webrtc-vad)    │
│  - 格式转换 (PCM 16kHz)         │
└──────┬──────────────────────────┘
       │ 音频段落
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

## 8. 实施计划

### Phase 1: 音频基础（1-2 天）
- [ ] 集成 `cpal` 实现音频采集
- [ ] 集成 `webrtc-vad` 实现 VAD
- [ ] 音频预处理（重采样、格式转换）

### Phase 2: ASR 双模式（3-5 天）
- [ ] 百度语音 API 封装
  - Token 获取和刷新
  - 音频识别接口
- [ ] Whisper 本地推理
  - 模型加载
  - 推理接口
- [ ] 运行时模式切换

### Phase 3: 意图理解（2-3 天）
- [ ] LLM API 集成（OpenAI/Claude）
- [ ] Prompt 工程和测试
- [ ] 结构化输出解析

### Phase 4: 调度集成（1-2 天）
- [ ] 意图 → 脚本方案映射
- [ ] 与现有热键系统集成
- [ ] 错误处理和反馈

### Phase 5: UI 和配置（1-2 天）
- [ ] 语音控制面板
- [ ] 模式切换按钮
- [ ] 配置持久化

**总计：8-14 天**

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
