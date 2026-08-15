//! PyO3 绑定（`py` feature）——薄翻译层，不含生成逻辑。
//!
//! 暴露：
//! - `ReapeaksStreamer`：与 Python 参考 `_ReaPeaksStreamer` 同构的类
//!   （`feed(bytes)` / `finish(src_timestamp=0, src_filesize=0) -> bytes`）
//! - `generate(pcm, sample_rate, channels, ...) -> bytes`：bulk 快速入口
//!
//! 所有重活在 `py.detach(...)` 中执行（释放 GIL，pyo3 0.29 API）；
//! 输入输出均为 bytes，不依赖 numpy。

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::options::{Feature, OptionsError, StreamerOptions};
use crate::streamer::ReapeaksStreamer as CoreStreamer;

/// 把 crate 错误映射为 Python 异常。
fn to_py_err(err: OptionsError) -> PyErr {
    PyValueError::new_err(err.to_string())
}

/// 解析 features 参数：None → 默认 ["wave"]；否则逐名解析。
fn parse_features(features: Option<Vec<String>>) -> PyResult<Vec<Feature>> {
    let features = features.unwrap_or_else(|| vec!["wave".to_string()]);
    features
        .iter()
        .map(|name| {
            Feature::parse(name)
                .ok_or_else(|| PyValueError::new_err(format!("未知特性名: {name}")))
        })
        .collect()
}

/// 流式生成器（对 Python 暴露）。
#[pyclass(module = "reapeaks_rust")]
pub struct ReapeaksStreamer {
    inner: CoreStreamer,
}

#[pymethods]
impl ReapeaksStreamer {
    /// 构造。
    ///
    /// :param sample_rate: 源采样率
    /// :param channels: 声道数（>=1）
    /// :param divs: wave 层 division factors（升序）；None → 默认
    ///   300/20/1 峰值每秒（由采样率推导）
    /// :param features: 生成特性子集，``None`` 或 ``("wave",)`` 为默认；
    ///   可选 ``"wave" / "spectral" / "loudness"``
    /// :param mipmap_levels: 保留最细的 N 层（默认 1）
    #[new]
    #[pyo3(signature = (sample_rate, channels, divs=None, features=None, mipmap_levels=1))]
    fn new(
        sample_rate: u32,
        channels: u32,
        divs: Option<Vec<u32>>,
        features: Option<Vec<String>>,
        mipmap_levels: usize,
    ) -> PyResult<Self> {
        let features = parse_features(features)?;
        let options = StreamerOptions::new(sample_rate, channels, divs, Some(features), mipmap_levels)
            .map_err(to_py_err)?;
        let inner = CoreStreamer::new(sample_rate, channels, options).map_err(to_py_err)?;
        Ok(ReapeaksStreamer { inner })
    }

    /// 消费一个 s16le 交错字节块（可任意切分）。
    /// 重活发生在释放 GIL 的线程中。
    fn feed(&mut self, py: Python<'_>, data: &[u8]) -> PyResult<()> {
        let inner = &mut self.inner;
        let result = py.detach(move || inner.feed(data));
        result.map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// 冲刷并返回完整 `.ReaPeaks` 字节。
    #[pyo3(signature = (src_timestamp=0, src_filesize=0))]
    fn finish(&mut self, py: Python<'_>, src_timestamp: i32, src_filesize: i32) -> PyResult<Py<PyBytes>> {
        let inner = &mut self.inner;
        let bytes = py.detach(move || inner.finish(src_timestamp, src_filesize));
        Python::attach(|py| Ok(PyBytes::new(py, &bytes).unbind()))
    }
}

/// bulk 快速入口：一次性传入全部 s16le 交错 PCM，返回完整 RPKN 字节。
///
/// 参数语义同 `ReapeaksStreamer`；入口内部整段并行处理。
#[pyfunction]
#[pyo3(signature = (pcm, sample_rate, channels, divs=None, features=None, mipmap_levels=1, src_timestamp=0, src_filesize=0))]
#[allow(clippy::too_many_arguments)]
fn generate(
    py: Python<'_>,
    pcm: &[u8],
    sample_rate: u32,
    channels: u32,
    divs: Option<Vec<u32>>,
    features: Option<Vec<String>>,
    mipmap_levels: usize,
    src_timestamp: i32,
    src_filesize: i32,
) -> PyResult<Py<PyBytes>> {
    let features = parse_features(features)?;
    let options = StreamerOptions::new(sample_rate, channels, divs, Some(features), mipmap_levels)
        .map_err(to_py_err)?;
    let bytes = py.detach(move || -> Result<Vec<u8>, PyErr> {
        let mut streamer = CoreStreamer::new(sample_rate, channels, options).map_err(to_py_err)?;
        streamer
            .feed(pcm)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(streamer.finish(src_timestamp, src_filesize))
    })?;
    Python::attach(|py| Ok(PyBytes::new(py, &bytes).unbind()))
}

/// `reapeaks_rust` 模块入口。
#[pymodule]
fn reapeaks_rust(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ReapeaksStreamer>()?;
    m.add_function(wrap_pyfunction!(generate, m)?)?;
    Ok(())
}