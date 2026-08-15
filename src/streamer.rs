//! `ReapeaksStreamer`：流式消费交错 s16le 字节，产出完整 RPKN 字节。
//!
//! 与 Python 参考 `_ReaPeaksStreamer` 接口同构：
//! - `feed(&[u8])`：可任意分块（含非整帧尾部字节，内部 carry 到下一块）；
//!   任意分块序列的输出 ≡ 一次喂完全部输入（chunk 切分不变性）。
//! - `finish(timestamp, filesize) -> Vec<u8>`：冲刷各层残桶、统一 spectral
//!   trim 口径（`fine_div * fine_npeak - 1280` 的 `c_total // div`）、
//!   loudness pad/truncate，再调用 `format::assemble`。
//!
//! 并行：`feed` 内（rayon）按层并行；bulk 场景（一次性大块）自动获得
//! 最大并行度。GIL 释放由 `py.rs` 负责，本层无 Python 依赖。

use crate::format::{self, LayerData};
use crate::loudness::LoudnessLayer;
use crate::options::{Feature, OptionsError, StreamerOptions};
use crate::spectral::SpectralLayer;
use crate::wave::WaveLayer;

/// 流式生成器（无 pyo3 依赖）。
#[derive(Debug)]
pub struct ReapeaksStreamer {
    sample_rate: u32,
    channels: usize,
    options: StreamerOptions,
    /// 已消费总帧数（每声道）。
    total_frames: u64,
    /// 跨块不足一帧的字节（< channels*2）。
    carry: Vec<u8>,
    wave_layers: Vec<WaveLayer>,
    spectral_layers: Vec<SpectralLayer>,
    // loudness 固定两层：div = sr/40（层1）、sr/2（层2）
    loudness_layers: Vec<LoudnessLayer>,
    /// feed 是否已被调用过（finish 之后禁止 feed 见文档约定）。
    finished: bool,
}

impl ReapeaksStreamer {
    /// 构造。`channels`、`divs`、`features`、`mipmap_levels` 语义见
    /// `StreamerOptions::new`；loudness 层数固定为 2（div = sr/40、sr/2）。
    pub fn new(
        sample_rate: u32,
        channels: u32,
        options: StreamerOptions,
    ) -> Result<Self, OptionsError> {
        todo!()
    }

    /// 采样率。
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// 声道数。
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// 已消费总帧数。
    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// 消费一个字节块（s16le 交错）。可任意分块；尾部非整帧字节
    /// carry 到下一次 `feed`。`finish()` 之后调用返回 `StreamerError`。
    pub fn feed(&mut self, data: &[u8]) -> Result<(), StreamerError> {
        todo!()
    }

    /// 冲刷所有层并组装 RPKN 字节。`src_timestamp`/`src_filesize` 写入
    /// 全局头（low 32 bits）。可重复调用（结果一致），但返回后不可再 feed。
    pub fn finish(&mut self, src_timestamp: i32, src_filesize: i32) -> Vec<u8> {
        todo!()
    }
}

/// 流式错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamerError {
    /// `finish()` 之后再次 `feed`。
    FeedAfterFinish,
}

impl std::fmt::Display for StreamerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamerError::FeedAfterFinish => write!(f, "finish() 之后不能再 feed()"),
        }
    }
}

impl std::error::Error for StreamerError {}

/// 从 `carry + data` 中拼出完整的 i16 帧块，剩余字节写回 carry。
/// 返回 `(完整块 i16, 新 carry)`。
fn splice_frames(carry: &[u8], data: &[u8], channels: usize) -> (Vec<i16>, Vec<u8>) {
    todo!()
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn chunk_split_invariance_random_sizes() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn header_metadata_written_into_finish() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn wave_only_default_features() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn spectral_trim_uses_c_total_formula() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn loudness_layer_one_pads_layer_two_truncates() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn mipmap_levels_2_keeps_two_wave_layers() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn odd_byte_tail_carries_across_feeds() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn golden_small_vector_stereo() {
        todo!()
    }
}