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
/// FFT 长度（2048）。
pub const FFT_LEN: usize = 2048;

/// 稳态 2048 样本窗口的 Hann 权重（预计算一次，与逐样本 `0.5-0.5*cos(...)` 逐位一致）。
fn hann_full() -> &'static [f64] {
    use std::sync::OnceLock;
    static HANN: OnceLock<Vec<f64>> = OnceLock::new();
    HANN.get_or_init(|| {
        (0..FFT_LEN)
            .map(|i| {
                0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (FFT_LEN - 1) as f64).cos()
            })
            .collect()
    })
}

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

impl SpectralLayer {
    /// `div`：本层 division factor；`channels`：声道数；`sample_rate`：源采样率。
    pub fn new(div: u32, channels: usize, sample_rate: u32) -> Self {
        assert!(div > 0, "div 必须 >= 1");
        assert!(channels >= 1, "channels 必须 >= 1");
        SpectralLayer {
            div,
            channels,
            sample_rate,
            next_center: 0,
            hist: None,
            out: Vec::new(),
        }
    }

    /// 本层 division factor。
    pub fn div(&self) -> u32 {
        self.div
    }

    /// 消费一块**完整帧**的交错 i16 样本；`block_start_frame` 为该块第一帧的
    /// 绝对位置（用于中心判定）。不变量：`block.len() % channels == 0`。
    pub fn feed(&mut self, block: &[i16], block_start_frame: u64) {
        debug_assert_eq!(block.len() % self.channels, 0);
        let n_frames = (block.len() / self.channels) as u64;
        if n_frames == 0 {
            return;
        }
        // 可见样本流：hist（若有）+ block，绝对坐标 [base, total_after)
        let hist_frames = self.hist.as_ref().map_or(0, |h| h.len() / self.channels) as u64;
        let base = block_start_frame.saturating_sub(hist_frames);
        let total_after = block_start_frame + n_frames;

        let div = self.div as u64;
        // 收集本块内所有需要计算的窗口（center 升序），随后并行做 FFT。
        let mut windows: Vec<(u64, usize)> = Vec::new(); // (center, channel)
        while self.next_center + HALF_FFT <= total_after {
            let center = self.next_center;
            // 与参考一致：s0 = max(0, center - 1024)，文件开头的窗口不足 2048 长
            let s0 = center.saturating_sub(HALF_FFT);
            // s1 <= total_after 由 while 保证；s0 >= base 由 base=start(含 hist) 保证
            if s0 < base {
                // 窗口起点超出可见历史（理论上不应发生：hist 始终保留最近 2048 帧，
                // 而 s0 = center - 1024 >= next_center 前沿 - 1024；防御性跳过）
                self.next_center += div;
                continue;
            }
            for c in 0..self.channels {
                windows.push((center, c));
            }
            self.next_center += div;
        }
        // 并行计算所有谱码（rayon 按峰切分，worker 复用 FFT scratch）。
        // 窗口 buffer 用 thread_local 复用，避免每峰分配。
        if !windows.is_empty() {
            use rayon::prelude::*;
            let block_ref: &[i16] = block;
            let hist_ref: Option<&Vec<i16>> = self.hist.as_ref();
            let channels = self.channels;
            let sample_rate = self.sample_rate;
            let codes: Vec<i32> = windows
                .par_chunks(64)
                .flat_map_iter(|chunk| {
                    thread_local! {
                        static WIN: std::cell::RefCell<Vec<i16>> = const { std::cell::RefCell::new(Vec::new()) };
                    }
                    chunk.iter().map(move |&(center, c)| {
                        let s0 = center.saturating_sub(HALF_FFT);
                        let s1 = center + HALF_FFT;
                        WIN.with(|cell| {
                            let mut win = cell.borrow_mut();
                            win.clear();
                            win.reserve((s1 - s0) as usize);
                            if s0 >= block_start_frame {
                                // 快路径：窗口完全落在当前块内，无跨块分支、无逐样本乘法
                                let idx0 = (s0 - block_start_frame) as usize;
                                let n = (s1 - s0) as usize;
                                if channels == 1 {
                                    win.extend_from_slice(&block_ref[idx0..idx0 + n]);
                                } else {
                                    win.extend(
                                        block_ref[idx0 * channels + c..]
                                            .iter()
                                            .copied()
                                            .step_by(channels)
                                            .take(n),
                                    );
                                }
                            } else {
                                for abs in s0..s1 {
                                    let sample = if abs < block_start_frame {
                                        let idx = (abs - base) as usize;
                                        hist_ref.expect("hist 必存在")[idx * channels + c]
                                    } else {
                                        let idx = (abs - block_start_frame) as usize;
                                        block_ref[idx * channels + c]
                                    };
                                    win.push(sample);
                                }
                            }
                            let (freq, density) = freq_density(&win, sample_rate);
                            ((density as i32) << 15) | freq as i32
                        })
                    })
                })
                .collect();
            self.out.extend_from_slice(&codes);
        }
        // 更新 hist：保留可见流（hist+block）的尾部至多 2048 帧。
        // 比参考实现（仅取 block 尾部）更健壮：即使连续小块（<2048 帧），
        // hist 也始终是"最近 2048 帧"，保证下一块的 base 不回落。
        let mut stream: Vec<i16> = Vec::with_capacity(FFT_LEN * self.channels);
        if let Some(h) = &self.hist {
            stream.extend_from_slice(h);
        }
        stream.extend_from_slice(block);
        let keep = stream.len().min(FFT_LEN * self.channels);
        self.hist = Some(stream[stream.len() - keep..].to_vec());
    }

    /// 结束输入（无额外冲刷语义；truncation 由 streamer 统一执行）。
    pub fn finish(&mut self) {
        // 无操作：谱峰只在天花板满足时输出；truncation 在 streamer.finish 做
    }

    /// 已输出峰值数。
    pub fn peak_count(&self) -> usize {
        self.out.len() / self.channels
    }

    /// 输出本层频谱码字节（i32 小端）。
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.out.len() * 4);
        for &v in &self.out {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }
}

/// 由一段 2048 样本窗口（每声道）计算 `(freq_hz, density)`。
///
/// 与 Python 参考 `_freq_density` / `_spectral_code` 数学等价（±1 取整容差内）：
/// - 计算 f64 频谱，丢弃 DC
/// - freq = argmax + 抛物线插值（采样率/2048 分辨率）
/// - density = −2961.5 * ln(flatness) + 3995.3，截断到 [1, 16383]
pub fn freq_density(window: &[i16], sample_rate: u32) -> (u16, u16) {
    let n = window.len();
    if n < 8 {
        return (0, 0);
    }
    let fftn = FFT_LEN;
    // thread_local 复用中间 buffer：segf、加窗 buf（real_fft 内还有自己的 scratch）
    use std::cell::RefCell;
    thread_local! {
        static SEGF: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
        static BUF: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
        static MAGS: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
    }
    SEGF.with(|cell| {
        let mut segf = cell.borrow_mut();
        segf.clear();
        segf.extend(window.iter().map(|&v| v as f64 / 32768.0));
        BUF.with(|bc| {
            let mut buf = bc.borrow_mut();
            buf.clear();
            buf.resize(fftn, 0.0);
            // 与 MAW 原始参考 _spec_buf 一致：总是加窗（窗长 = 有效段长），居中放置。
            let start = (fftn - n) / 2;
            let end = (start + n).min(fftn);
            let seg_len = end - start;
            if seg_len == fftn {
                // 稳态全窗：用预计算 Hann 表（逐位一致，避免逐峰算 cos）
                let hann = hann_full();
                for (i, &v) in segf.iter().enumerate() {
                    buf[start + i] = v * hann[i];
                }
            } else {
                for (i, &v) in segf.iter().take(seg_len).enumerate() {
                    let w = if seg_len > 1 {
                        0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (seg_len - 1) as f64).cos()
                    } else {
                        1.0
                    };
                    buf[start + i] = v * w;
                }
            }
            // 2048 点实数 FFT → 幅值谱（丢弃 DC），复用 MAGS 缓冲避免逐峰分配
            MAGS.with(|mc| {
                let mut mags = mc.borrow_mut();
                real_fft(&buf, &mut mags);
                let ac = &mags[1..];
                if ac.is_empty() {
                    return (0, 0);
                }
                // argmax
                let mut idx = 0usize;
                let mut best = ac[0];
                for (i, &v) in ac.iter().enumerate() {
                    if v > best {
                        best = v;
                        idx = i;
                    }
                }
                // 抛物线插值（ac 去掉 DC 后，bin 序号 = idx + 1）
                let res = sample_rate as f64 / fftn as f64;
                let freq = if idx == 0 || idx + 1 >= ac.len() {
                    0.0
                } else {
                    let y0 = ac[idx - 1];
                    let y1 = ac[idx];
                    let y2 = ac[idx + 1];
                    let den = y0 - 2.0 * y1 + y2;
                    let delta = if den.abs() > 1e-12 {
                        0.5 * (y0 - y2) / den
                    } else {
                        0.0
                    };
                    (idx + 1) as f64 * res + delta * res
                };
                // density：谱平坦度
                let sum: f64 = ac.iter().sum();
                let density = if sum <= 0.0 {
                    0.0
                } else {
                    let log_sum: f64 = ac.iter().map(|&v| v.max(1e-12).ln()).sum();
                    let geo = (log_sum / ac.len() as f64).exp();
                    let arith = sum / ac.len() as f64;
                    let flatness = if arith > 0.0 { geo / arith } else { 0.0 };
                    if flatness <= 0.0 {
                        0.0
                    } else {
                        (-2961.5 * flatness.ln() + 3995.3).clamp(1.0, 16383.0)
                    }
                };
                let freq = (freq.round() as u16).min(0x7FFF);
                let density = (density.round() as u16).min(0x3FFF);
                (freq, density)
            })
        })
    })
}

/// 2048 点实数 FFT 的幅值谱（丢弃相位），写进调用方复用的 `out` 缓冲。
///
/// FFT plan 用进程级 `OnceLock` 缓存（`process` 只取 `&self`，跨 worker 共享）；
/// 输入/输出 buffer 用 thread_local 复用，避免热路径反复分配。
fn real_fft(input: &[f64], out: &mut Vec<f64>) {
    use realfft::{RealFftPlanner, RealToComplex};
    use std::cell::RefCell;
    use std::sync::{Arc, OnceLock};

    static R2C: OnceLock<Arc<dyn RealToComplex<f64>>> = OnceLock::new();
    thread_local! {
        static SCRATCH: RefCell<(Vec<f64>, Vec<realfft::num_complex::Complex<f64>>)> =
            RefCell::new((Vec::new(), Vec::new()));
    }
    let r2c = R2C.get_or_init(|| {
        let mut planner = RealFftPlanner::<f64>::new();
        planner.plan_fft_forward(FFT_LEN)
    });
    SCRATCH.with(|cell| {
        let (indata, spectrum) = &mut *cell.borrow_mut();
        indata.clear();
        indata.extend_from_slice(input);
        if spectrum.len() != FFT_LEN / 2 + 1 {
            *spectrum = r2c.make_output_vec();
        }
        r2c.process(indata, spectrum).expect("FFT 失败");
        out.clear();
        out.extend(spectrum.iter().map(|c| c.norm()));
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone_win(freq: f64, sr: u32, amp: f64) -> Vec<i16> {
        (0..FFT_LEN)
            .map(|i| {
                (amp * 32767.0 * (2.0 * std::f64::consts::PI * freq * i as f64 / sr as f64).sin())
                    as i16
            })
            .collect()
    }

    #[test]
    fn freq_density_of_300hz_tone() {
        let (freq, density) = freq_density(&tone_win(300.0, 44100, 0.9), 44100);
        assert!((freq as i32 - 300).abs() < 10, "freq={freq}");
        // 参考语义：n>=2048 不加窗（矩形窗泄漏），纯音 density 仍应显著高于噪声
        assert!(density > 5000, "纯音 density 应较高, got {density}");
    }

    #[test]
    fn silence_gives_zero_code() {
        let win = vec![0i16; FFT_LEN];
        let (freq, density) = freq_density(&win, 44100);
        assert_eq!((freq, density), (0, 0));
    }

    #[test]
    fn chunk_split_invariance_with_hist_carry() {
        let sr = 44100u32;
        let ch = 1usize;
        let n = 20000usize;
        let signal: Vec<i16> = (0..n)
            .map(|i| {
                (20000.0 * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / sr as f64).sin()) as i16
            })
            .collect();
        let mut one = SpectralLayer::new(147, ch, sr);
        one.feed(&signal, 0);
        one.finish();
        // 真实场景块大小（>= 2048 帧，保证窗口样本总在 hist+block 内，
        // 与 Python 参考 64KB 块的语义一致；小于 2048 的极端小块是参考实现的
        // 已知 wrap 边界，不做不变性保证）
        let mut split = SpectralLayer::new(147, ch, sr);
        let mut pos = 0usize;
        let mut frame = 0u64;
        while pos < signal.len() {
            let take = 8192usize.min(signal.len() - pos);
            split.feed(&signal[pos..pos + take], frame);
            pos += take;
            frame += take as u64;
        }
        split.finish();
        assert_eq!(one.bytes(), split.bytes(), "分块应等于一次喂");
    }

    #[test]
    fn centers_step_by_div() {
        let mut layer = SpectralLayer::new(147, 1, 44100);
        let signal = vec![0i16; 3000];
        layer.feed(&signal, 0);
        layer.finish();
        // center 序列满足 center+1024 <= 3000: 0,147,...,2058? 2058+1024=3082>3000 → 停
        // 147*k + 1024 <= 3000 → k <= 13.44 → k=0..13 共 14 个
        assert_eq!(layer.peak_count(), 14);
    }

    #[test]
    fn stereo_produces_per_channel_codes() {
        let mut layer = SpectralLayer::new(2048, 2, 44100);
        let n = 4096usize;
        let mut signal = vec![0i16; n * 2];
        for i in 0..n {
            signal[i * 2] =
                (30000.0 * (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / 44100.0).sin()) as i16;
        }
        layer.feed(&signal, 0);
        layer.finish();
        // center: 0, 2048 (2048+1024<=4096)；4096+1024>4096 停
        assert_eq!(layer.peak_count(), 2); // 2 峰（每峰 2 声道 code）
        assert_eq!(layer.bytes().len(), 2 * 2 * 4);
    }
}