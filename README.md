# reapeaks-rs

REAPER `.ReaPeaks`（RPKN v1.1）流式生成器：Rust 内核 + PyO3 绑定。发行名与 Python 模块名统一为 `reapeaks`，仓库名沿用 `reapeaks-rs`。

仓库：[https://github.com/drunkenQCat/reapeaks-rs](https://github.com/drunkenQCat/reapeaks-rs)

## 这是什么

REAPER 在首次导入媒体时生成 `.ReaPeaks` 峰值文件，用于波形 / 频谱 / 响度的快速显示与读取。本项目把这一生成内核用 Rust 重写，经 PyO3 暴露为与现有 Python 调用近乎同构的接口：把逐峰值的解释器循环替换为原生循环 + 多核并行，显著降低长时间媒体的峰值生成耗时。

输入是已解码的 s16le 交错 PCM 字节，输出是完整的 RPKN 字节；ffmpeg 子进程管理仍留在上游（如 MAW 的 `reapeaks.py`），本包只做纯计算内核。

## 特性

- **流式接口**：`ReapeaksStreamer.feed(bytes)` / `finish()`，任意分块、内存有界，可无缝接 ffmpeg 管道。
- **bulk 入口**：`generate(pcm, ...)` 一次性传入整段 PCM，内部全量并行，最快路径。
- **三种 mipmap**：`wave`（默认）/ `spectral` / `loudness`，用 `features` 开关选择任意子集。
- **层数开关**：`mipmap_levels` 保留最细的 N 层（默认 1）。
- **自定义分层**：`divs` 自定义 division factors；默认约 300 / 20 / 1 峰值每秒。
- **分块不变性**：任意分块 `feed` 与一次喂完，输出逐字节一致。
- **精度契约**：wave 层与 Python 参考实现逐字节一致；spectral ±1 容差；loudness 1 ulp 容差。
- **abi3 wheel**：单份 wheel 覆盖多个 Python 版本（requires-python ≥ 3.9）。

## 安装

```bash
pip install reapeaks
```

从源码构建（需要 Rust ≥ 1.83 与 maturin）：

```bash
pip install maturin
maturin build --release --out dist
pip install dist/*.whl
```

开发安装（构建并安装到当前虚拟环境）：

```bash
maturin develop
```

## 快速上手

```python
import reapeaks

# 流式：适合接 ffmpeg pipe，内存有界
s = reapeaks.ReapeaksStreamer(
    sample_rate=48000,
    channels=2,
    divs=None,                 # None → 默认约 300/20/1 峰值每秒
    features=("wave",),        # ("wave",) / ("wave","spectral") / ("wave","loudness") 等任意子集
    mipmap_levels=1,           # 保留最细的 N 层
)
s.feed(chunk_bytes)            # s16le 交错 PCM，可任意分块
data = s.finish(src_timestamp=0, src_filesize=0)   # -> bytes（完整 .ReaPeaks）

# bulk：一次性传入整段 PCM，内部全量并行
data = reapeaks.generate(
    pcm_bytes, 48000, 2,
    features=["wave", "spectral", "loudness"],
    mipmap_levels=3,
    src_timestamp=0,
    src_filesize=0,
)
```

输入输出都是 `bytes`，不依赖 numpy。非法参数（channels < 1、features 为空或未知、mipmap_levels < 1、div 含 0 等）抛 `ValueError`。

### 参数

| 参数 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `sample_rate` | int | — | 源采样率（Hz） |
| `channels` | int | — | 声道数（≥ 1） |
| `divs` | `list[int] \| None` | `None` | wave/spectral 层 division factors（升序，最细在前） |
| `features` | `list[str] \| None` | `("wave",)` | `"wave" / "spectral" / "loudness"` 任一子集 |
| `mipmap_levels` | int | `1` | 保留最细的 N 层 wave/spectral |
| `src_timestamp` | int | `0` | 写回 header 的源文件时间戳（`finish`/`generate`） |
| `src_filesize` | int | `0` | 写回 header 的源文件大小（`finish`/`generate`） |

> `ReapeaksStreamer` 的构造参数为 `(sample_rate, channels, ...)`；`generate` 的签名是 `(pcm, sample_rate, channels, ...)`。

## 精度与验收

- wave 层：全整数 min/max，与 Python 参考实现**逐字节一致**。
- spectral 层：±1 容差（FFT 舍入顺序差异）。
- loudness 层：f32 输出 1 ulp 容差。

三层 golden 验收：L1 单元向量（`cargo test`）、L2 差分（vs `tests/python_ref` 参考实现）、L3 REAPER 真机 fixture。详见 `docs/golden-verification.md`。

## 开发

```bash
cargo test                                              # Rust 内核单元测试（不依赖 pyo3）
cargo clippy --all-targets -- -D warnings
cargo fmt --check

maturin develop                                         # 构建并安装到虚拟环境
python -m unittest discover -s tests -p "test_*.py"     # L2/L3 差分与 fixture 测试
```

### 项目结构

```text
Cargo.toml / pyproject.toml          # Rust 包与 maturin 构建配置
src/
  lib.rs                              # crate 根：core API + py feature 门控
  options.rs                          # 开关（features/mipmap_levels/divs）与校验
  streamer.rs                         # 流式内核（无 pyo3 依赖）
  wave.rs / spectral.rs / loudness.rs # 三种 mipmap 累加器
  format.rs                           # RPKN 字节组装
  py.rs                               # PyO3 绑定薄层（py feature）
tests/
  test_reapeaks_rust_differential.py  # L2 差分（vs tests/python_ref）
  test_reapeaks_fixture.py            # L3 REAPER fixture 语义验证
  python_ref/                         # 带开关的 Python 参考实现
  test_data/                          # REAPER 真机 fixture（只读）
docs/                                 # 架构 / 选型 / golden 方案
```

## 文档

- `docs/architecture.md` —— 代码架构与数据流
- `docs/technical-selection.md` —— crate 选型与路线图
- `docs/golden-verification.md` —— 三层 golden 验收方案
- `reapeaks-knowledge/` —— 格式规范与 MAW 参考实现

## License

`MIT OR Apache-2.0`（双许可，任选其一）。
