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
        todo!()
    }

    /// 本层 division factor。
    pub fn div(&self) -> u32 {
        todo!()
    }

    /// 消费一块**完整帧**的交错 i16 样本（帧数 = `block.len() / channels`）。
    ///
    /// 不变量：`block.len()` 必须是 `channels` 的整数倍（streamer 层负责 carry）。
    pub fn feed(&mut self, block: &[i16]) {
        todo!()
    }

    /// 冲刷尾部未满 bucket（不足 div 的残段也输出 max/min），此后不再 feed。
    pub fn finish(&mut self) {
        todo!()
    }

    /// 已输出峰值数（每峰每声道占 2 个 i16）。
    pub fn peak_count(&self) -> usize {
        todo!()
    }

    /// 输出本层峰值字节（`Vec<i16>` 的小端字节表示）。
    pub fn bytes(&self) -> Vec<u8> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn single_bucket_mono_minmax() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn exact_bucket_boundary_no_tail() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn partial_tail_bucket_flushes_on_finish() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn stereo_interleaved_layout() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn asymmetric_signal_keeps_max_min_order() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn chunk_splitting_invariance() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn multi_channel_beyond_stereo() {
        todo!()
    }
}