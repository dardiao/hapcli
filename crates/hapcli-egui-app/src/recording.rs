//! 终端会话录制：采集输出字节流并按时间戳保存为 .hrec 文件，支持回放。

use std::fs;
use std::path::Path;
use std::time::Instant;

const MAGIC: &[u8; 11] = b"HAPCLI_REC1";

pub struct Recording {
    chunks: Vec<(u64, Vec<u8>)>,
    started: Instant,
    total_bytes: usize,
}

impl Default for Recording {
    fn default() -> Self {
        Self::new()
    }
}

impl Recording {
    pub fn new() -> Self {
        Self {
            chunks: Vec::new(),
            started: Instant::now(),
            total_bytes: 0,
        }
    }

    pub fn append(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let delta = self.started.elapsed().as_millis() as u64;
        self.chunks.push((delta, bytes.to_vec()));
        self.total_bytes += bytes.len();
    }

    pub fn event_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    pub fn elapsed_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        for (delta, bytes) in &self.chunks {
            out.extend_from_slice(&delta.to_le_bytes());
            out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            out.extend_from_slice(bytes);
        }
        fs::write(path, out).map_err(|error| error.to_string())
    }

    pub fn load(path: &Path) -> Result<Vec<(u64, Vec<u8>)>, String> {
        let data = fs::read(path).map_err(|error| error.to_string())?;
        if !data.starts_with(MAGIC) {
            return Err("不是 hapcli 录制文件 (.hrec)".to_string());
        }
        let mut rest = &data[MAGIC.len()..];
        let mut chunks = Vec::new();
        while !rest.is_empty() {
            if rest.len() < 16 {
                return Err("录制文件损坏：头部不完整".to_string());
            }
            let delta = u64::from_le_bytes(rest[..8].try_into().expect("8 bytes"));
            let len = u64::from_le_bytes(rest[8..16].try_into().expect("8 bytes"));
            rest = &rest[16..];
            if len as usize > rest.len() {
                return Err("录制文件损坏：数据长度越界".to_string());
            }
            chunks.push((delta, rest[..len as usize].to_vec()));
            rest = &rest[len as usize..];
        }
        Ok(chunks)
    }
}

/// 回放状态：按时间戳逐步把录制内容喂给回放会话。
pub struct PlaybackState {
    pub chunks: Vec<(u64, Vec<u8>)>,
    pub cursor: usize,
    pub elapsed_ms: f64,
    pub playing: bool,
    pub speed: f64,
    pub file_name: String,
    last_tick: Option<Instant>,
}

impl PlaybackState {
    pub fn new(chunks: Vec<(u64, Vec<u8>)>, file_name: String) -> Self {
        Self {
            chunks,
            cursor: 0,
            elapsed_ms: 0.0,
            playing: true,
            speed: 1.0,
            file_name,
            last_tick: None,
        }
    }

    pub fn finished(&self) -> bool {
        self.cursor >= self.chunks.len()
    }

    pub fn progress(&self) -> f32 {
        if self.chunks.is_empty() {
            0.0
        } else {
            self.cursor as f32 / self.chunks.len() as f32
        }
    }

    /// 推进时间并返回需要喂给内核的字节（可能为空）。
    pub fn advance(&mut self) -> Vec<u8> {
        let now = Instant::now();
        let dt = match self.last_tick {
            Some(last) => now.duration_since(last).as_secs_f64(),
            None => 0.0,
        };
        self.last_tick = Some(now);
        if !self.playing {
            return Vec::new();
        }
        self.elapsed_ms += dt * 1000.0 * self.speed;

        let mut output = Vec::new();
        while self.cursor < self.chunks.len()
            && self.chunks[self.cursor].0 as f64 <= self.elapsed_ms
        {
            output.extend_from_slice(&self.chunks[self.cursor].1);
            self.cursor += 1;
        }
        if self.finished() {
            self.playing = false;
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_round_trip_preserves_chunks() {
        let path = std::env::temp_dir().join(format!(
            "hapcli_rec_test_{}.hrec",
            std::process::id()
        ));
        let mut recording = Recording::new();
        recording.append(b"hello");
        std::thread::sleep(std::time::Duration::from_millis(5));
        recording.append(b"world");
        recording.save(&path).unwrap();

        let chunks = Recording::load(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].1, b"hello");
        assert_eq!(chunks[1].1, b"world");
        assert!(chunks[1].0 > 0, "第二个事件时间戳应大于 0");
    }

    #[test]
    fn load_rejects_invalid_file() {
        let path = std::env::temp_dir().join("hapcli_rec_bad.hrec");
        std::fs::write(&path, b"not a recording").unwrap();
        assert!(Recording::load(&path).is_err());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn playback_advances_through_chunks() {
        let chunks = vec![(0, b"a".to_vec()), (10, b"b".to_vec())];
        let mut state = PlaybackState::new(chunks, "test.hrec".to_string());
        assert_eq!(state.advance(), b"a"); // 第一帧立即输出 0ms 事件
        assert!(!state.finished());
        // 手动推进时间：模拟 20ms 过去
        state.elapsed_ms = 20.0;
        state.last_tick = None;
        assert_eq!(state.advance(), b"b");
        assert!(state.finished());
    }
}
