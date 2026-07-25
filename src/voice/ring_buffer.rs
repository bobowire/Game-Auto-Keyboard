// 音频环形缓冲 - 持续写入，保留最近 N 秒，防止唤醒词检测延迟导致指令音频丢失

use std::collections::VecDeque;

pub struct AudioRingBuffer {
    buffer: VecDeque<i16>,
    capacity: usize,
    sample_rate: usize,
}

impl AudioRingBuffer {
    /// 创建环形缓冲，保留最近 `seconds` 秒的音频
    pub fn new(sample_rate: usize, seconds: usize) -> Self {
        let capacity = sample_rate * seconds;
        Self {
            buffer: VecDeque::with_capacity(capacity),
            capacity,
            sample_rate,
        }
    }

    /// 写入音频帧，超出容量丢弃最旧数据
    pub fn push(&mut self, samples: &[i16]) {
        self.buffer.extend(samples.iter().copied());
        while self.buffer.len() > self.capacity {
            self.buffer.pop_front();
        }
    }

    /// 取出最近 `ms` 毫秒的音频（用于唤醒后回溯指令起点）
    pub fn take_recent(&self, ms: usize) -> Vec<i16> {
        let n = ms * self.sample_rate / 1000;
        let start = self.buffer.len().saturating_sub(n);
        self.buffer.iter().skip(start).copied().collect()
    }

    /// 当前缓冲的样本数
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// 清空缓冲
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    pub fn sample_rate(&self) -> usize {
        self.sample_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_within_capacity() {
        let mut rb = AudioRingBuffer::new(1000, 2); // 容量 2000
        rb.push(&[1, 2, 3]);
        assert_eq!(rb.len(), 3);
    }

    #[test]
    fn test_overflow_drops_oldest() {
        let mut rb = AudioRingBuffer::new(10, 1); // 容量 10
        let data: Vec<i16> = (0..15).collect();
        rb.push(&data);
        // 只保留最近 10 个：5..15
        assert_eq!(rb.len(), 10);
        let recent = rb.take_recent(1000); // 全部
        assert_eq!(recent, (5..15).collect::<Vec<i16>>());
    }

    #[test]
    fn test_take_recent_ms() {
        let mut rb = AudioRingBuffer::new(1000, 3); // 1000 samples/sec
        let data: Vec<i16> = (0..1000).collect();
        rb.push(&data);
        // 取最近 500ms = 500 samples
        let recent = rb.take_recent(500);
        assert_eq!(recent.len(), 500);
        assert_eq!(recent[0], 500);
    }

    #[test]
    fn test_clear() {
        let mut rb = AudioRingBuffer::new(1000, 2);
        rb.push(&[1, 2, 3]);
        rb.clear();
        assert!(rb.is_empty());
    }
}
