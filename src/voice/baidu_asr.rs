// 百度语音识别 API 封装
//
// 使用百度短语音识别 API，支持 60 秒内的音频识别。
// API 文档：https://ai.baidu.com/ai-doc/SPEECH/Vk38lxily

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::Deserialize;
use std::time::{Duration, Instant};

const TOKEN_URL: &str = "https://aip.baidubce.com/oauth/2.0/token";
const ASR_URL: &str = "https://vop.baidu.com/server_api";

/// 百度 ASR 客户端
pub struct BaiduAsr {
    api_key: String,
    secret_key: String,
    /// access_token 缓存
    token: Option<String>,
    /// token 获取时间（30天有效期）
    token_obtained: Option<Instant>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AsrResponse {
    err_no: i32,
    err_msg: Option<String>,
    sn: Option<String>,
    result: Option<Vec<String>>,
}

impl BaiduAsr {
    pub fn new(api_key: String, secret_key: String) -> Self {
        Self {
            api_key,
            secret_key,
            token: None,
            token_obtained: None,
        }
    }

    /// 识别音频（i16 单声道 16kHz PCM）
    pub fn recognize(&mut self, audio: &[i16]) -> Result<String, String> {
        // 确保 token 有效
        self.ensure_token()?;
        let token = self.token.as_ref().unwrap();

        // 转 u8 字节（小端序）
        let mut bytes = Vec::with_capacity(audio.len() * 2);
        for sample in audio {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }

        // base64 编码
        let speech = BASE64.encode(&bytes);
        let len = bytes.len();

        // 构造请求参数
        let body = serde_json::json!({
            "format": "pcm",
            "rate": 16000,
            "channel": 1,
            "cuid": "rust_client",
            "token": token,
            "speech": speech,
            "len": len,
        });

        // 发送请求
        let resp: AsrResponse = ureq::post(ASR_URL)
            .set("Content-Type", "application/json")
            .send_json(&body)
            .map_err(|e| format!("请求失败: {}", e))?
            .into_json()
            .map_err(|e| format!("解析响应失败: {}", e))?;

        // 处理响应
        if resp.err_no == 0 {
            if let Some(result) = resp.result {
                if !result.is_empty() {
                    return Ok(result.join(""));
                }
            }
            Err("识别结果为空".to_string())
        } else {
            Err(format!(
                "识别失败 err_no={}: {}",
                resp.err_no,
                resp.err_msg.unwrap_or_default()
            ))
        }
    }

    /// 确保 token 有效（过期或不存在时重新获取）
    fn ensure_token(&mut self) -> Result<(), String> {
        // token 有效期 30 天，提前 1 天刷新
        let need_refresh = match self.token_obtained {
            Some(obtained) => obtained.elapsed() > Duration::from_secs(29 * 24 * 3600),
            None => true,
        };

        if need_refresh {
            self.fetch_token()?;
        }
        Ok(())
    }

    /// 获取 access_token
    fn fetch_token(&mut self) -> Result<(), String> {
        let url = format!(
            "{}?grant_type=client_credentials&client_id={}&client_secret={}",
            TOKEN_URL, self.api_key, self.secret_key
        );

        let resp: TokenResponse = ureq::get(&url)
            .call()
            .map_err(|e| format!("获取 token 失败: {}", e))?
            .into_json()
            .map_err(|e| format!("解析 token 响应失败: {}", e))?;

        if let Some(token) = resp.access_token {
            self.token = Some(token);
            self.token_obtained = Some(Instant::now());
            Ok(())
        } else {
            Err(format!(
                "获取 token 失败: {} - {}",
                resp.error.unwrap_or_default(),
                resp.error_description.unwrap_or_default()
            ))
        }
    }
}
