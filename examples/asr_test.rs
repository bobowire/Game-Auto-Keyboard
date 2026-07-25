// 百度 ASR 测试
//
// 用途：
// 1. 测试百度 API 能否正常识别
// 2. 用之前 wakeword_test 录的 command wav 测试识别效果
//
// 运行前：在 config/config.json 里填入百度 API Key 和 Secret Key

use game_auto_keyboard::config::AppConfig;
use game_auto_keyboard::voice::BaiduAsr;
use std::env;
use std::fs::File;
use std::io::Read;

fn main() {
    println!("=== 百度 ASR 测试 ===");
    println!();

    // 加载配置
    let config = AppConfig::load();
    if config.baidu.api_key.is_empty() || config.baidu.secret_key.is_empty() {
        eprintln!("错误：配置文件中未填写百度 API Key 和 Secret Key");
        eprintln!();
        eprintln!("请编辑 {} 添加：", AppConfig::path().display());
        eprintln!(r#"  "baidu": {{"#);
        eprintln!(r#"    "api_key": "你的API Key","#);
        eprintln!(r#"    "secret_key": "你的Secret Key""#);
        eprintln!(r#"  }}"#);
        eprintln!();
        eprintln!("API Key 申请：https://console.bce.baidu.com/ai/#/ai/speech/app/list");
        return;
    }

    let mut asr = BaiduAsr::new(config.baidu.api_key, config.baidu.secret_key);

    // 从命令行参数读取 wav 文件，或使用默认
    let wav_path = env::args().nth(1).unwrap_or_else(|| {
        println!("未指定 wav 文件，使用默认 command_1.wav");
        println!("用法: cargo run --example asr_test <wav文件路径>");
        println!();
        "command_1.wav".to_string()
    });

    // 读取 wav 文件
    println!("读取音频文件: {}", wav_path);
    let audio = match read_pcm_from_wav(&wav_path) {
        Ok(pcm) => {
            println!("  ✓ 已读取 {} 样本（{:.2}秒）", pcm.len(), pcm.len() as f32 / 16000.0);
            pcm
        }
        Err(e) => {
            eprintln!("读取失败: {}", e);
            return;
        }
    };

    // 识别
    println!();
    println!("正在识别...");
    match asr.recognize(&audio) {
        Ok(text) => {
            println!();
            println!("识别结果: {}", text);
        }
        Err(e) => {
            eprintln!("识别失败: {}", e);
        }
    }
}

/// 从 wav 文件读取 PCM 数据（跳过 44 字节 wav 头）
fn read_pcm_from_wav(path: &str) -> Result<Vec<i16>, String> {
    let mut file = File::open(path).map_err(|e| format!("打开文件失败: {}", e))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| format!("读取文件失败: {}", e))?;

    // 简单处理：跳过 44 字节 wav 头（假设标准格式）
    if bytes.len() < 44 {
        return Err("wav 文件过小".to_string());
    }

    let pcm_bytes = &bytes[44..];
    let mut pcm = Vec::with_capacity(pcm_bytes.len() / 2);

    for chunk in pcm_bytes.chunks_exact(2) {
        let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
        pcm.push(sample);
    }

    Ok(pcm)
}
