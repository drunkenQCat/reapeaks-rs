//! spectral 层：按 division factor 分桶，输出每桶每声道一个 32-bit
//! 频谱码 `freq(15 bits) | density(15 bits)`。
//!
//! 语义与 Python 参考 `_feed_spec` / `_spectral_code` 一致：
//! 以每层的"下一中心"为锚点、步进 `div`，取以中心为对称的 2048 样本
//! Hanning 窗（中心两侧各 1024），对每声道做一次 2048 点实数 FFT，
//! freq 由 argmax + 抛物线插值，density 由谱平坦度映射。
//! 跨块需要携带上一块尾部 2048 样本（`hist`）；输出在 `finish()` 阶段由
//! streamer 依据“主 wave 层 fine div × fine npeak − 1280”的统一口径做
//! `c_total // div` 截断（见 `streamer.rs`），本层不做 trim。

/// 频谱窗口半宽（2048 / 2）。
pub const HALF_FFT: u64 = 1024;

/// 单层 spectral 累加器（无 pyo3 依赖）。
#[derive(Debug)]
pub struct SpectralLayer {
    div: u32,
    channels: usize,
    sample_rate: u32,
    /// 下一个谱峰中心（绝对帧位置，`_spec_next`）。
    next_center: u64,
    /// 上一块尾部 2048 样本（跨块窗口切片用；首块为 None）。
    hist: Option<Vec<i16>>,
    /// 已输出频谱码（i32，`freq | density<<15`），按峰→声道顺序。
    out: Vec<i32>,
}

/// 每层桶中心与输出（每峰每声道一个 i32 码）。
impl SpectralLayer {
    /// `div`：本层 division factor；`channels`：声道数；`sample_rate`：源采样率。
    pub fn new(div: u32, channels: usize, sample_rate: u32) -> Self {
        todo!()
    }

    /// 本层 division factor。
    pub fn div(&self) -> u32 {
        todo!()
    }

    /// 消费一块**完整帧**的交错 i16 样本；`block_start_frame` 为该块第一帧的
    /// 绝对位置（用于中心判定）。不变量：`block.len() % channels == 0`。
    pub fn feed(&mut self, block: &[i16], block_start_frame: u64) {
        todo!()
    }

    /// 结束输入（无额外冲刷语义；truncation 由 streamer 统一执行）。
    pub fn finish(&mut self) {
        todo!()
    }

    /// 已输出峰值数。
    pub fn peak_count(&self) -> usize {
        todo!()
    }

    /// 输出本层频谱码字节（i32 小端）。
    pub fn bytes(&self) -> Vec<u8> {
        todo!()
    }
}

/// 由一段 2048 样本窗口（每声道）计算 `(freq_hz, density)`。
///
/// 与 `/home/deck/MyCode/reapeaks-rs/reapeaks-knowledge/reapeaks_generate.py`
/// 的 `_freq_density` / `_spectral_code` 数学等价（±1 取整容差内）：
/// - 计算 f64 频谱，丢弃 DC
/// - freq = argmax + 抛物线插值（采样率/2048 分辨率）
/// - density = −2961.5 * ln(flatness) + 3995.3，截断到 [1, 16383]
pub fn freq_density(window: &[i16], sample_rate: u32) -> (u16, u16) {
    todo!()
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn freq_density_of_300hz_tone() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn silence_gives_zero_code() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn chunk_split_invariance_with_hist_carry() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn centers_step_by_div() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn stereo_produces_per_channel_codes() {
        todo!()
    }
}