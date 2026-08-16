//! 生成 `python/reapeaks/_native.pyi` 类型桩。
//!
//! 桩是构建产物（不入 Git）：本地 `cargo run --bin stub_gen --features py`
//! 与 CI build 前执行的是同一条命令。
//!
//! 用法：
//! ```text
//! cargo run --bin stub_gen --features py
//! ```

use pyo3_stub_gen::Result;
use std::path::Path;

fn main() -> Result<()> {
    reapeaks::stub_info()?.generate()?;

    // pyo3-stub-gen 0.23 在 mixed layout 下把桩写到 `<module>/__init__.pyi`
    // （目录形式），而 maturin 只把扁平的 `<module>.pyi` 打进 wheel/sdist。
    // 这里把它挪成扁平文件并清掉空目录（见 maturin project_layout 文档）。
    let stub_dir = Path::new("python/reapeaks/_native");
    let stub_file = stub_dir.join("__init__.pyi");
    if stub_file.is_file() {
        std::fs::rename(&stub_file, Path::new("python/reapeaks/_native.pyi"))?;
        if stub_dir.read_dir()?.next().is_none() {
            std::fs::remove_dir(stub_dir)?;
        }
    }
    Ok(())
}
