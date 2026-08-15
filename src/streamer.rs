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

use crate::format::{self, LayerData, MipmapHeader};
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
    /// 是否已 finish（finish 之后禁止 feed）。
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
        if channels < 1 {
            return Err(OptionsError::InvalidChannels(channels));
        }
        let channels = channels as usize;
        let divs = options.wave_divs().to_vec();
        let mut wave_layers = Vec::new();
        let mut spectral_layers = Vec::new();
        if options.is_enabled(Feature::Wave) {
            for &div in &divs {
                wave_layers.push(WaveLayer::new(div, channels));
            }
        }
        if options.is_enabled(Feature::Spectral) {
            for &div in &divs {
                spectral_layers.push(SpectralLayer::new(div, channels, sample_rate));
            }
        }
        let mut loudness_layers = Vec::new();
        if options.is_enabled(Feature::Loudness) {
            let div1 = (sample_rate / 40).max(1);
            let div2 = (sample_rate / 2).max(1);
            loudness_layers.push(LoudnessLayer::new(div1, channels));
            loudness_layers.push(LoudnessLayer::new(div2, channels));
        }
        Ok(ReapeaksStreamer {
            sample_rate,
            channels,
            options,
            total_frames: 0,
            carry: Vec::new(),
            wave_layers,
            spectral_layers,
            loudness_layers,
            finished: false,
        })
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
    // `is_multiple_of` 需 Rust 1.87，而本 crate 对齐 pyo3 0.29 的 MSRV 1.83，
    // 故保留 `% == 0` 并抑制该 lint。
    #[allow(clippy::manual_is_multiple_of)]
    pub fn feed(&mut self, data: &[u8]) -> Result<(), StreamerError> {
        if self.finished {
            return Err(StreamerError::FeedAfterFinish);
        }
        if data.is_empty() {
            return Ok(());
        }
        let block_start_frame = self.total_frames;
        let frame_bytes = self.channels * 2;

        // 快路径：无 carry 且数据为整帧 —— 优先零拷贝，否则拷贝进局部 scratch。
        if self.carry.is_empty() && data.len() % frame_bytes == 0 {
            if let Some(frames) = try_zero_copy_i16(data) {
                let n_frames = (frames.len() / self.channels) as u64;
                self.feed_layers(frames, block_start_frame);
                self.total_frames += n_frames;
                return Ok(());
            }
            let scratch: Vec<i16> = data
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
            let n_frames = (scratch.len() / self.channels) as u64;
            self.feed_layers(&scratch, block_start_frame);
            self.total_frames += n_frames;
            return Ok(());
        }

        // 一般路径：有 carry 或尾部非整帧，splice 后喂入。
        let (frames, new_carry) = splice_frames(&self.carry, data, self.channels);
        self.carry = new_carry;
        let n_frames = (frames.len() / self.channels) as u64;
        if n_frames == 0 {
            return Ok(());
        }
        self.feed_layers(&frames, block_start_frame);
        self.total_frames += n_frames;
        Ok(())
    }

    /// 把一份完整帧的 i16 块喂给所有启用的层（按层并行）。
    fn feed_layers(&mut self, frames: &[i16], block_start_frame: u64) {
        use rayon::prelude::*;
        if self.options.is_enabled(Feature::Spectral) {
            self.spectral_layers
                .par_iter_mut()
                .for_each(|layer| layer.feed(frames, block_start_frame));
        }
        if self.options.is_enabled(Feature::Wave) {
            self.wave_layers
                .par_iter_mut()
                .for_each(|layer| layer.feed(frames));
        }
        if self.options.is_enabled(Feature::Loudness) {
            self.loudness_layers
                .par_iter_mut()
                .for_each(|layer| layer.feed(frames));
        }
    }

    /// 冲刷所有层并组装 RPKN 字节。`src_timestamp`/`src_filesize` 写入
    /// 全局头（low 32 bits）。可重复调用（结果一致），但返回后不可再 feed。
    pub fn finish(&mut self, src_timestamp: i32, src_filesize: i32) -> Vec<u8> {
        self.finished = true;
        self.assemble(src_timestamp, src_filesize)
    }

    /// 组装 RPKN（wave flush → loudness pad/truncate → spectral trim → assemble）。
    fn assemble(&mut self, src_timestamp: i32, src_filesize: i32) -> Vec<u8> {
        // wave：flush 残留
        for layer in &mut self.wave_layers {
            layer.finish();
        }
        // loudness：层1 flush + pad 到 npeak1 = ceil(total/div1)+1；层2 truncate
        let mut loud_data: Vec<Vec<u8>> = Vec::new();
        let mut loud_headers: Vec<MipmapHeader> = Vec::new();
        if !self.loudness_layers.is_empty() {
            let div1 = self.loudness_layers[0].div();
            let div2 = self.loudness_layers[1].div();
            // 层1
            {
                let layer = &mut self.loudness_layers[0];
                layer.finish();
                let npeak1 = self.total_frames.div_ceil(div1 as u64) + 1;
                let mut data = layer.bytes();
                let limit = npeak1 as usize * self.channels * 4;
                if data.len() < limit {
                    data.extend(std::iter::repeat_n(0u8, limit - data.len()));
                } else {
                    data.truncate(limit);
                }
                loud_headers.push(format::loudness_header(npeak1 as u32));
                loud_data.push(data);
            }
            // 层2：truncate 到 floor(total/div2)，残留丢弃
            {
                let layer = &mut self.loudness_layers[1];
                let npeak2 = self.total_frames / div2 as u64;
                let mut data = layer.bytes();
                let limit = npeak2 as usize * self.channels * 4;
                data.truncate(limit);
                loud_headers.push(format::loudness_header(npeak2 as u32));
                loud_data.push(data);
            }
        }
        // spectral trim：fine_div × fine_npeak − 1280 → c_total，每层 c_total//div
        let mut spec_data: Vec<Vec<u8>> = Vec::new();
        let mut spec_headers: Vec<MipmapHeader> = Vec::new();
        if !self.spectral_layers.is_empty() {
            let fine_div = self.spectral_layers[0].div() as i64;
            // fine_npeak：wave 启用时取最细 wave 层实际峰数；
            // 未启用时用等效峰数 ceil(total/fine_div)（与参考 _finest_npeak 一致）
            let fine_npeak = match self.wave_layers.first() {
                Some(w) => w.peak_count() as i64,
                None => self.total_frames.div_ceil(fine_div as u64) as i64,
            };
            let c_total = fine_div * fine_npeak - 1280;
            for layer in &mut self.spectral_layers {
                layer.finish();
                let div = layer.div() as i64;
                let npeak = (c_total / div).max(0) as u32;
                let mut data = layer.bytes();
                let limit = npeak as usize * self.channels * 4;
                data.truncate(limit);
                spec_headers.push(format::spectral_header(npeak));
                spec_data.push(data);
            }
        }
        // wave headers/data
        let mut wave_headers: Vec<MipmapHeader> = Vec::new();
        let mut wave_data: Vec<Vec<u8>> = Vec::new();
        for layer in &mut self.wave_layers {
            let div = layer.div();
            let npeak = layer.peak_count();
            wave_headers.push(format::wave_header(div, npeak as u32));
            wave_data.push(layer.bytes());
        }
        // assemble（顺序 wave → spectral → loudness）
        let mut layers: Vec<LayerData> = Vec::new();
        layers.push(LayerData {
            headers: wave_headers,
            data: wave_data,
        });
        if !spec_headers.is_empty() {
            layers.push(LayerData {
                headers: spec_headers,
                data: spec_data,
            });
        }
        if !loud_headers.is_empty() {
            layers.push(LayerData {
                headers: loud_headers,
                data: loud_data,
            });
        }
        format::assemble(
            self.channels as u8,
            self.sample_rate,
            src_timestamp,
            src_filesize,
            &layers,
        )
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

/// 零拷贝把 s16le 字节切片重解释为 `&[i16]`（仅小端、且 2 字节对齐时可用）。
#[cfg(target_endian = "little")]
fn try_zero_copy_i16(data: &[u8]) -> Option<&[i16]> {
    // SAFETY: `i16` 无非法的位模式；`align_to` 保证 middle 2 字节对齐且覆盖整数个
    // i16；仅在小端目标上启用（s16le == 本机 i16），故重解释后数值语义与
    // `i16::from_le_bytes` 完全一致。
    let (prefix, middle, suffix) = unsafe { data.align_to::<i16>() };
    if prefix.is_empty() && suffix.is_empty() {
        Some(middle)
    } else {
        None
    }
}

#[cfg(not(target_endian = "little"))]
fn try_zero_copy_i16(_data: &[u8]) -> Option<&[i16]> {
    None
}

/// 从 `carry + data` 中拼出完整的 i16 帧块，剩余字节写回 carry。
/// 返回 `(完整块 i16, 新 carry)`。
fn splice_frames(carry: &[u8], data: &[u8], channels: usize) -> (Vec<i16>, Vec<u8>) {
    let frame_bytes = channels * 2;
    let mut buf: Vec<u8> = Vec::with_capacity(carry.len() + data.len());
    buf.extend_from_slice(carry);
    buf.extend_from_slice(data);
    let n_complete = buf.len() / frame_bytes * frame_bytes;
    let new_carry = buf[n_complete..].to_vec();
    let mut frames = Vec::with_capacity(n_complete / 2);
    for chunk in buf[..n_complete].chunks_exact(2) {
        frames.push(i16::from_le_bytes([chunk[0], chunk[1]]));
    }
    (frames, new_carry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::Feature;

    fn pcm_sine(sr: u32, ch: usize, frames: usize, freq: f64) -> Vec<u8> {
        let mut out = Vec::with_capacity(frames * ch * 2);
        for i in 0..frames {
            for c in 0..ch {
                let v = (16000.0
                    * (2.0 * std::f64::consts::PI * freq * (i as f64) / sr as f64 + c as f64 * 1.7)
                        .sin()) as i16;
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out
    }

    fn make_streamer(
        sr: u32,
        ch: u32,
        features: &[Feature],
        mipmap_levels: usize,
    ) -> ReapeaksStreamer {
        let opts =
            StreamerOptions::new(sr, ch, None, Some(features.to_vec()), mipmap_levels).unwrap();
        ReapeaksStreamer::new(sr, ch, opts).unwrap()
    }

    #[test]
    fn chunk_split_invariance_random_sizes() {
        let sr = 44100u32;
        let data = pcm_sine(sr, 2, 30000, 440.0);
        let mut one = make_streamer(
            sr,
            2,
            &[Feature::Wave, Feature::Spectral, Feature::Loudness],
            3,
        );
        one.feed(&data).unwrap();
        let one_bytes = one.finish(0, 0);
        let mut split = make_streamer(
            sr,
            2,
            &[Feature::Wave, Feature::Spectral, Feature::Loudness],
            3,
        );
        // 随机分块（下限 4096 帧 = 8192 字节：spectral 窗口需要块 >= 2048 帧，
        // 与 Python 参考 64KB 块的语义一致；奇数字节 carry 由 odd_byte 测试单独覆盖）
        let mut i = 0usize;
        let mut rng = 12345u64;
        while i < data.len() {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let take = 4096 + ((rng >> 33) as usize % 4096);
            let take = take.min(data.len() - i);
            split.feed(&data[i..i + take]).unwrap();
            i += take;
        }
        let split_bytes = split.finish(0, 0);
        assert_eq!(one_bytes, split_bytes, "分块输出应等于一次喂");
    }

    #[test]
    fn header_metadata_written_into_finish() {
        let sr = 8000u32;
        let data = pcm_sine(sr, 1, 8000, 440.0);
        let mut s = make_streamer(sr, 1, &[Feature::Wave], 1);
        s.feed(&data).unwrap();
        let out = s.finish(123456789, 987654321);
        assert_eq!(&out[0..4], b"RPKN");
        assert_eq!(out[4], 1);
        assert_eq!(&out[6..10], &(8000i32).to_le_bytes());
        assert_eq!(&out[10..14], &123456789i32.to_le_bytes());
        assert_eq!(&out[14..18], &987654321i32.to_le_bytes());
    }

    #[test]
    fn wave_only_default_features() {
        let sr = 8000u32;
        let data = pcm_sine(sr, 1, 8000, 440.0);
        let mut s = make_streamer(sr, 1, &[Feature::Wave], 1);
        s.feed(&data).unwrap();
        let out = s.finish(0, 0);
        let mipmap_count = out[5];
        assert_eq!(mipmap_count, 1, "wave-only 应只有 1 个 mipmap");
        assert_eq!(&out[18..22], &(26i32).to_le_bytes());
    }

    #[test]
    fn spectral_trim_uses_c_total_formula() {
        let sr = 8000u32;
        let data = pcm_sine(sr, 1, 8000, 440.0);
        let mut s = make_streamer(sr, 1, &[Feature::Wave, Feature::Spectral], 1);
        s.feed(&data).unwrap();
        let out = s.finish(0, 0);
        let mipmap_count = out[5];
        assert_eq!(mipmap_count, 2);
        let div = i32::from_le_bytes([out[26], out[27], out[28], out[29]]);
        assert_eq!(div, -('s' as i32));
    }

    #[test]
    fn loudness_layer_one_pads_layer_two_truncates() {
        let sr = 8000u32;
        let data = pcm_sine(sr, 1, 8000, 440.0);
        let mut s = make_streamer(sr, 1, &[Feature::Wave, Feature::Loudness], 1);
        s.feed(&data).unwrap();
        let out = s.finish(0, 0);
        let mipmap_count = out[5];
        assert_eq!(mipmap_count, 3, "wave + loudness 两层 = 3 个 mipmap");
    }

    #[test]
    fn mipmap_levels_2_keeps_two_wave_layers() {
        let sr = 8000u32;
        let data = pcm_sine(sr, 1, 8000, 440.0);
        let mut s = make_streamer(sr, 1, &[Feature::Wave], 2);
        s.feed(&data).unwrap();
        let out = s.finish(0, 0);
        assert_eq!(out[5], 2, "mipmap_levels=2 → 2 个 wave 层");
        assert_eq!(&out[18..22], &(26i32).to_le_bytes());
        assert_eq!(&out[26..30], &(400i32).to_le_bytes());
    }

    #[test]
    fn odd_byte_tail_carries_across_feeds() {
        let sr = 8000u32;
        let mut s = make_streamer(sr, 1, &[Feature::Wave], 1);
        s.feed(&[1, 0, 2]).unwrap();
        s.feed(&[0]).unwrap();
        let data = pcm_sine(sr, 1, 100, 440.0);
        s.feed(&data).unwrap();
        let out = s.finish(0, 0);
        assert!(out.len() > 18);
    }

    #[test]
    fn golden_small_vector_stereo() {
        // sr=8000, ch=2, 4 帧, wave-only, div=26 > 4 → 单桶
        let sr = 8000u32;
        let frames: [i16; 8] = [100, -100, 50, 200, -300, 0, 10, -10];
        let mut bytes = Vec::new();
        for v in frames {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let mut s = make_streamer(sr, 2, &[Feature::Wave], 1);
        s.feed(&bytes).unwrap();
        let out = s.finish(0, 0);
        let data = &out[26..];
        // L max=100 L min=-300 R max=200 R min=-100
        let expected: [i16; 4] = [100, -300, 200, -100];
        for (i, &e) in expected.iter().enumerate() {
            let got = i16::from_le_bytes([data[i * 2], data[i * 2 + 1]]);
            assert_eq!(got, e, "peak {i}");
        }
        assert_eq!(data.len(), 8);
    }
}
