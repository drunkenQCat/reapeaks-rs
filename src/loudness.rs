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
        todo!()
    }

    /// 本层 division factor。
    pub fn div(&self) -> u32 {
        todo!()
    }

    /// 消费一块**完整帧**的交错 i16 样本（帧数 = `block.len() / channels`）。
    pub fn feed(&mut self, block: &[i16]) {
        todo!()
    }

    /// 冲刷当前残桶（不足 div 也输出 RMS），此后不再 feed。
    pub fn finish(&mut self) {
        todo!()
    }

    /// 已输出峰值数（每峰每声道一个 f32）。
    pub fn peak_count(&self) -> usize {
        todo!()
    }

    /// 输出本层峰值字节（f32 小端）。
    pub fn bytes(&self) -> Vec<u8> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn rms_of_silence_is_zero() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn rms_of_full_scale_square_wave() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn partial_tail_flushes_on_finish() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn stereo_channels_independent() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn i64_accumulator_never_overflows() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn chunk_splitting_invariance() {
        todo!()
    }
}