//! reapeaks-rust：REAPER .ReaPeaks（RPKN v1.1）流式生成器。
//!
//! 纯 Rust 内核（无 pyo3 依赖），PyO3 绑定由 `py` feature 门控（见 `py.rs`）。
//! 语义基准：`reapeaks-knowledge/reapeaks_generate.py`（Python 参考实现），
//! 验收契约见 `docs/golden-verification.md`（L1 单元向量 / L2 差分 / L3 REAPER fixture）。

pub mod format;
pub mod loudness;
pub mod options;
pub mod spectral;
pub mod streamer;
pub mod wave;

#[cfg(feature = "py")]
pub mod py;

pub use format::{assemble, LayerData, MipmapHeader};
pub use options::{Feature, OptionsError, StreamerOptions};
pub use streamer::ReapeaksStreamer;