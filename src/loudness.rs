//! loudness 层：按 division factor 分桶，输出每桶每声道 RMS（f32）。
//!
//! 语义与 Python 参考 `_feed_loud` 一致：每层维护 per-channel i64 平方和
//! 与计数，`sqrt(sq / count) / 32768.0` 输出 f32。分桶 div 固定为
//! `sr/40`（层 1）与 `sr/2`（层 2），由 streamer 构造。`finish()` 时
//! 层 1 的尾部残桶 flush 并 pad 到 npeak，层 2 的残桶丢弃——该差异由
//! streamer 的组合逻辑处理，本层只提供统一的"flush 当前残桶"原语。

/// 单层 loudness 累加器（无 pyo3 依赖）。
#[derive(Debug)]
pub struct LoudnessLayer {
    div: u32,
    channels: usize,
    /// 已完结的 RMS 值（per-channel f32，按峰→声道顺序）。
    out: Vec<f32>,
    /// 当前 bucket 的 per-channel 平方和（i64：1.7e17 内不溢出）。
    acc_sq: Vec<i64>,
    /// 当前 bucket 已积累样本数。
    acc_count: u32,
}

impl LoudnessLayer {
    /// `div`：本层 division factor（>0）；`channels`：声道数（>=1）。
    pub fn new(div: u32, channels: usize) -> Self {
        assert!(div > 0, "div 必须 >= 1");
        assert!(channels >= 1, "channels 必须 >= 1");
        LoudnessLayer {
            div,
            channels,
            out: Vec::new(),
            acc_sq: vec![0i64; channels],
            acc_count: 0,
        }
    }

    /// 本层 division factor。
    pub fn div(&self) -> u32 {
        self.div
    }

    /// 消费一块**完整帧**的交错 i16 样本（帧数 = `block.len() / channels`）。
    pub fn feed(&mut self, block: &[i16]) {
        debug_assert_eq!(block.len() % self.channels, 0);
        let n = block.len() / self.channels;
        if n == 0 {
            return;
        }
        let div = self.div as usize;
        let mut start_frames = 0usize;
        // 先补贴当前未满 bucket
        if self.acc_count > 0 {
            let take = ((self.div - self.acc_count) as usize).min(n);
            for frame in block[..take * self.channels].chunks_exact(self.channels) {
                for c in 0..self.channels {
                    let v = frame[c] as i64;
                    self.acc_sq[c] += v * v;
                }
            }
            self.acc_count += take as u32;
            start_frames = take;
            if self.acc_count >= self.div {
                self.flush_acc();
            } else {
                return;
            }
        }
        let rest = &block[start_frames * self.channels..];
        let rest_frames = rest.len() / self.channels;
        if rest_frames == 0 {
            return;
        }
        let full = rest_frames / div * div;
        if full > 0 {
            for bucket in rest[..full * self.channels].chunks_exact(div * self.channels) {
                let mut sqsum = vec![0i64; self.channels];
                for frame in bucket.chunks_exact(self.channels) {
                    for c in 0..self.channels {
                        let v = frame[c] as i64;
                        sqsum[c] += v * v;
                    }
                }
                for c in 0..self.channels {
                    let rms = (sqsum[c] as f64 / div as f64).sqrt() / 32768.0;
                    self.out.push(rms as f32);
                }
            }
        }
        let tail = &rest[full * self.channels..];
        if !tail.is_empty() {
            self.acc_sq = vec![0i64; self.channels];
            self.acc_count = (tail.len() / self.channels) as u32;
            for frame in tail.chunks_exact(self.channels) {
                for c in 0..self.channels {
                    let v = frame[c] as i64;
                    self.acc_sq[c] += v * v;
                }
            }
        }
    }

    /// 冲刷当前残桶（不足 div 也输出 RMS），此后不再 feed。
    pub fn finish(&mut self) {
        if self.acc_count > 0 {
            self.flush_acc();
        }
    }

    /// 已输出峰值数（每峰每声道一个 f32）。
    pub fn peak_count(&self) -> usize {
        self.out.len() / self.channels
    }

    /// 输出本层峰值字节（f32 小端）。
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.out.len() * 4);
        for &v in &self.out {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }
}

impl LoudnessLayer {
    /// 把当前 acc（无论满不满）作为一峰输出并清零。
    fn flush_acc(&mut self) {
        let count = self.acc_count as f64;
        if count > 0.0 {
            for c in 0..self.channels {
                let rms = (self.acc_sq[c] as f64 / count).sqrt() / 32768.0;
                self.out.push(rms as f32);
            }
        }
        self.acc_sq = vec![0i64; self.channels];
        self.acc_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f32_bytes(v: f32) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }

    #[test]
    fn rms_of_silence_is_zero() {
        let mut layer = LoudnessLayer::new(4, 1);
        layer.feed(&[0, 0, 0, 0]);
        layer.finish();
        assert_eq!(layer.bytes(), f32_bytes(0.0));
    }

    #[test]
    fn rms_of_full_scale_square_wave() {
        let mut layer = LoudnessLayer::new(4, 1);
        layer.feed(&[i16::MAX, i16::MAX, i16::MIN, i16::MIN]);
        layer.finish();
        // (32767^2 + 32767^2 + 32768^2 + 32768^2)/4 开方 /32768 ≈ 0.9999695
        let rms = f32::from_le_bytes([layer.bytes()[0], layer.bytes()[1], layer.bytes()[2], layer.bytes()[3]]);
        assert!((rms - 0.9999695).abs() < 1e-4, "rms={rms}");
    }

    #[test]
    fn partial_tail_flushes_on_finish() {
        let mut layer = LoudnessLayer::new(4, 1);
        layer.feed(&[0, 0, 0, 0, 10000]); // 残桶 1 帧
        layer.finish();
        // 峰0：全零 rms=0；峰1：10000/32768
        let bytes = layer.bytes();
        assert_eq!(bytes.len(), 8);
        let rms1 = f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        assert!((rms1 - (10000.0 / 32768.0)).abs() < 1e-4, "rms1={rms1}");
    }

    #[test]
    fn stereo_channels_independent() {
        let mut layer = LoudnessLayer::new(2, 2);
        // 帧0: L=0 R=32767, 帧1: L=0 R=-32768
        layer.feed(&[0, 32767, 0, -32768]);
        layer.finish();
        let bytes = layer.bytes();
        let l = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let r = f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        assert!(l.abs() < 1e-6, "L 应≈0, got {l}");
        assert!((r - 1.0).abs() < 1e-4, "R 应≈1.0, got {r}");
    }

    #[test]
    fn i64_accumulator_never_overflows() {
        // 大量满幅样本：100 万帧 i16::MIN，平方和 = 1e6 * 2^30 ≈ 1.07e15 << i64::MAX
        let div = 100_000u32;
        let mut layer = LoudnessLayer::new(div, 1);
        let block = vec![i16::MIN; 1_000_000];
        layer.feed(&block);
        layer.finish();
        let bytes = layer.bytes();
        let rms = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        assert!((rms - 1.0).abs() < 1e-4, "满幅 RMS 应≈1.0, got {rms}");
    }

    #[test]
    fn chunk_splitting_invariance() {
        let data: Vec<i16> = (0..500)
            .map(|i| (((i * 37) % 200) as i16) - 100)
            .collect();
        let mut one = LoudnessLayer::new(7, 2);
        // 构造双声道交错数据
        let mut interleaved = Vec::with_capacity(data.len() * 2);
        for &v in &data {
            interleaved.push(v);
            interleaved.push(v.wrapping_neg());
        }
        one.feed(&interleaved);
        one.finish();
        let mut split = LoudnessLayer::new(7, 2);
        let mut i = 0;
        let mut rng = 98765u64;
        while i < interleaved.len() {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let take = ((rng >> 33) as usize % 47) + 1;
            let take = take.min(interleaved.len() - i);
            // 只喂完整帧
            let take = take - take % 2;
            if take == 0 {
                break;
            }
            split.feed(&interleaved[i..i + take]);
            i += take;
        }
        split.finish();
        assert_eq!(one.bytes(), split.bytes());
        assert_eq!(one.peak_count(), split.peak_count());
    }
}