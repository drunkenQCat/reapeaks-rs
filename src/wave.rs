//! wave 层：按 division factor 分桶，输出每桶每声道 min/max 对（i16）。
//!
//! 字节布局（v1.1 RPKN）：每峰每声道 `[max:i16, min:i16]`，声道交错：
//! `L0MAX L0MIN R0MAX R0MIN L1MAX L1MIN ...`。
//! 语义与 Python 参考 `_feed_wave` / `finish` 完全一致：不足一个 div 的
//! 尾部 bucket 在 `finish()` 时仍输出其 max/min。

/// 单层 wave 累加器（无 pyo3 依赖）。
#[derive(Debug)]
pub struct WaveLayer {
    div: u32,
    channels: usize,
    /// 已完结峰值的原始 i16 序列（按上述交错布局，恰好每峰 2*channels 个）。
    out: Vec<i16>,
    /// 当前（未满）bucket 的 per-channel max。
    acc_max: Vec<i16>,
    /// 当前（未满）bucket 的 per-channel min。
    acc_min: Vec<i16>,
    /// 当前 bucket 已积累的样本数（0 = 无未满 bucket）。
    acc_count: u32,
}

impl WaveLayer {
    /// `div`：本层 division factor（>0）；`channels`：声道数（>=1）。
    pub fn new(div: u32, channels: usize) -> Self {
        assert!(div > 0, "div 必须 >= 1");
        assert!(channels >= 1, "channels 必须 >= 1");
        WaveLayer {
            div,
            channels,
            out: Vec::new(),
            acc_max: vec![i16::MIN; channels],
            acc_min: vec![i16::MAX; channels],
            acc_count: 0,
        }
    }

    /// 本层 division factor。
    pub fn div(&self) -> u32 {
        self.div
    }

    /// 消费一块**完整帧**的交错 i16 样本（帧数 = `block.len() / channels`）。
    ///
    /// 不变量：`block.len()` 必须是 `channels` 的整数倍（streamer 层负责 carry）。
    pub fn feed(&mut self, block: &[i16]) {
        debug_assert_eq!(block.len() % self.channels, 0);
        let n = block.len() / self.channels;
        if n == 0 {
            return;
        }
        let div = self.div as usize;
        let mut start_frames = 0usize;
        // 先补贴当前未满 bucket（如果存在）
        if self.acc_count > 0 {
            let take = ((self.div - self.acc_count) as usize).min(n);
            for frame in block[..take * self.channels].chunks_exact(self.channels) {
                for (c, &v) in frame.iter().enumerate() {
                    if v > self.acc_max[c] {
                        self.acc_max[c] = v;
                    }
                    if v < self.acc_min[c] {
                        self.acc_min[c] = v;
                    }
                }
            }
            self.acc_count += take as u32;
            start_frames = take;
            if self.acc_count >= self.div {
                self.flush_acc();
                // acc_count 已归零
            } else {
                return; // 这块还不够填满 bucket，整块都已并入 acc
            }
        }
        let rest = &block[start_frames * self.channels..];
        let rest_frames = rest.len() / self.channels;
        if rest_frames == 0 {
            return;
        }
        let full = rest_frames / div * div; // 完整 bucket 的帧数
        if full > 0 {
            let mut bmax: Vec<i16> = Vec::with_capacity(self.channels);
            let mut bmin: Vec<i16> = Vec::with_capacity(self.channels);
            for bucket in rest[..full * self.channels].chunks_exact(div * self.channels) {
                bmax.clear();
                bmin.clear();
                for (i, frame) in bucket.chunks_exact(self.channels).enumerate() {
                    if i == 0 {
                        bmax.extend_from_slice(frame);
                        bmin.extend_from_slice(frame);
                    } else {
                        for c in 0..self.channels {
                            let v = frame[c];
                            if v > bmax[c] {
                                bmax[c] = v;
                            }
                            if v < bmin[c] {
                                bmin[c] = v;
                            }
                        }
                    }
                }
                for c in 0..self.channels {
                    self.out.push(bmax[c]);
                    self.out.push(bmin[c]);
                }
            }
        }
        // 尾部不足一个 bucket 的帧进入 acc
        let tail = &rest[full * self.channels..];
        if !tail.is_empty() {
            self.acc_max.fill(i16::MIN);
            self.acc_min.fill(i16::MAX);
            self.acc_count = (tail.len() / self.channels) as u32;
            for frame in tail.chunks_exact(self.channels) {
                for (c, &v) in frame.iter().enumerate() {
                    if v > self.acc_max[c] {
                        self.acc_max[c] = v;
                    }
                    if v < self.acc_min[c] {
                        self.acc_min[c] = v;
                    }
                }
            }
        }
    }

    /// 冲刷尾部未满 bucket（不足 div 的残段也输出 max/min），此后不再 feed。
    pub fn finish(&mut self) {
        if self.acc_count > 0 {
            self.flush_acc();
        }
    }

    /// 已输出峰值数（每峰每声道占 2 个 i16）。
    pub fn peak_count(&self) -> usize {
        self.out.len() / (2 * self.channels)
    }

    /// 输出本层峰值字节（`Vec<i16>` 的小端字节表示）。
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.out.len() * 2);
        for &v in &self.out {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }
}

impl WaveLayer {
    /// 把当前 acc（无论满不满）作为一峰输出并清零。
    fn flush_acc(&mut self) {
        for c in 0..self.channels {
            self.out.push(self.acc_max[c]);
            self.out.push(self.acc_min[c]);
        }
        self.acc_max.fill(i16::MIN);
        self.acc_min.fill(i16::MAX);
        self.acc_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_bucket_mono_minmax() {
        let mut layer = WaveLayer::new(4, 1);
        layer.feed(&[1, 5, -3, 2]);
        layer.finish();
        assert_eq!(layer.peak_count(), 1);
        assert_eq!(layer.bytes(), vec![5, 0, 253, 255]); // max=5, min=-3 LE
    }

    #[test]
    fn exact_bucket_boundary_no_tail() {
        let mut layer = WaveLayer::new(4, 1);
        layer.feed(&[1, 2, 3, 4]);
        layer.finish();
        assert_eq!(layer.peak_count(), 1);
        assert_eq!(layer.bytes(), vec![4, 0, 1, 0]);
    }

    #[test]
    fn partial_tail_bucket_flushes_on_finish() {
        let mut layer = WaveLayer::new(4, 1);
        layer.feed(&[1, 2, 3, 4, 7]); // 5 帧：1 完整桶 + 1 帧残桶
        assert_eq!(layer.peak_count(), 1);
        layer.finish();
        assert_eq!(layer.peak_count(), 2);
        assert_eq!(layer.bytes(), vec![4, 0, 1, 0, 7, 0, 7, 0]);
    }

    #[test]
    fn stereo_interleaved_layout() {
        let mut layer = WaveLayer::new(2, 2);
        // 帧0: (L=10, R=-5), 帧1: (L=3, R=8)
        layer.feed(&[10, -5, 3, 8]);
        layer.finish();
        // Lmax=10 Lmin=3 Rmax=8 Rmin=-5
        assert_eq!(layer.bytes(), vec![10, 0, 3, 0, 8, 0, 251, 255]);
    }

    #[test]
    fn asymmetric_signal_keeps_max_min_order() {
        let mut layer = WaveLayer::new(4, 1);
        layer.feed(&[-100, -50, -200, -10]); // 全部负值
        layer.finish();
        assert_eq!(layer.bytes(), vec![246, 255, 56, 255]); // max=-10, min=-200
    }

    #[test]
    fn chunk_splitting_invariance() {
        let data: Vec<i16> = (0..500).map(|i| ((i * 37) % 200) as i16 - 100).collect();
        // 一次喂完
        let mut one = WaveLayer::new(7, 1);
        one.feed(&data);
        one.finish();
        // 随机分块
        let mut split = WaveLayer::new(7, 1);
        let mut i = 0;
        let mut rng = 12345u64;
        while i < data.len() {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let take = ((rng >> 33) as usize % 37) + 1;
            let take = take.min(data.len() - i);
            split.feed(&data[i..i + take]);
            i += take;
        }
        split.finish();
        assert_eq!(one.bytes(), split.bytes());
        assert_eq!(one.peak_count(), split.peak_count());
    }

    #[test]
    fn multi_channel_beyond_stereo() {
        let mut layer = WaveLayer::new(2, 4);
        // 帧0: [1, 2, 3, 4], 帧1: [5, -1, 3, 0]
        layer.feed(&[1, 2, 3, 4, 5, -1, 3, 0]);
        layer.finish();
        // ch0: max5 min1; ch1: max2 min-1; ch2: max3 min3; ch3: max4 min0
        assert_eq!(
            layer.bytes(),
            vec![
                5, 0, 1, 0, // ch0
                2, 0, 255, 255, // ch1 (min=-1)
                3, 0, 3, 0, // ch2
                4, 0, 0, 0, // ch3
            ]
        );
    }
}
