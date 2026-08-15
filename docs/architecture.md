# reapeaks-rs 代码架构

> 状态：已确认（2025-08-15）｜配套文档：`technical-selection.md`（crate 选型）、`golden-verification.md`（测试与 golden 策略）

## 1. crate 形态：单 crate + optional pyo3 feature

```
Cargo.toml            # [features] py = ["dep:pyo3", ...]（默认关闭）
src/
  lib.rs              # crate 根：导出 core API；#[cfg(feature="py")] 挂绑定
  options.rs          # 开关类型（features / mipmap_levels / divs）+ 参数校验
  streamer.rs         # ReapeaksStreamer 核心（无 pyo3 依赖）
  wave.rs             # wave 层累加器（min/max，i16 整数运算）
  spectral.rs         # spectral 层（FFT 窗口 + freq/density）
  loudness.rs         # loudness 层（RMS，i64 平方和累加）
  format.rs           # RPKN 字节组装（header/mipmap 布局，纯函数）
  py.rs               # #[cfg(feature="py")] pyclass/pyfunction 薄绑定
```

理由：
- `cargo test`（L1）不编译 pyo3，迭代快；core 零 pyo3 依赖，可被任意 Rust 侧复用
- maturin 构建时 `--features py` 出 wheel
- 不搞 workspace：单 PyO3 包，单 crate 最简单，没有入口歧义

## 2. 数据流

```
Python（MAW 侧）                 reapeaks-rs
ffmpeg 子进程 ──pipe 1–4MB 块──▶ feed(&[i16 交错]) ──▶ wave/spectral/loudness 累加器
                                                     （各带 carry 状态，内存有界）
                              finish() ──▶ format::assemble() ──▶ RPKN bytes
```

**职责边界（不变式）**：reapeaks-rs 只做纯计算内核，接收"已解码的 s16le 交错字节"；
ffmpeg 子进程管理（查找/缺失降级/stderr/进程生命周期/缓存复用）全部留在 MAW 的
`reapeaks.py`。Python 侧唯一改动：读块从 64KB 加大到 **1–4 MB**（并行度考量，见 §4）。

**核心不变式**：feed 支持任意分块（1..N 字节），输出 ≡ 一次喂完全部输入（chunk 切分不变性）；残余帧 carry 跨块保持。

## 3. 流式内核

- `ReapeaksStreamer::new(sr, channels, opts)` → `feed(&[i16])` → `finish() -> Vec<u8>`
- 内部三个独立累加器（feature 未开启的不创建）：
  - `wave`: 每层每声道 running min/max，按 div 分 bucket；尾 bucket 残留也在 finish 时出峰
  - `spectral`: 2048 样本窗历史（跨块携带）+ 每层下一中心；freq/density 公式与参考一致（argmax + 抛物线插值 + 平坦度→density）
  - `loudness`: i64 平方和累加（不溢出：1.7e17 < i64 max），按参考的 div（sr//40、sr//2）与 finish 的 pad/截断逻辑
- `finish()` 组装逻辑（npeak 计算、spectral trim、loudness pad、header 元数据）逐条复刻参考实现细节，保证逐字节契约

## 4. 并行策略

- **streaming 路径：块内并行，默认开**：
  - spectral：`rayon par_iter` 按峰切 FFT（主战场，块 1–4MB 时每块 ~1 万+ 峰）
  - wave/loudness：层间并行（层数少，与 FFT 并行互补）
- **bulk 路径**（`generate`，内存允许时）：全量并行，spectral 按峰切
- **FFT 资源复用**：`realfft::RealFftPlanner` 预计算 2048 点 plan；每条 worker 线程一块 scratch + 预生成 Hanning 窗；杜绝每次 FFT 重复分配
- GIL：feed/finish 全程 `py.allow_threads` 包裹；Python 侧循环仅在块边界拿 GIL

## 5. Python API（`reapeaks` 模块）

```python
import reapeaks

# 流式（无缝替换 _ReaPeaksStreamer）
s = reapeaks.ReapeaksStreamer(
    sample_rate, channels,
    divs=None,                    # None → 默认 300/20/1 峰值每秒三层
    features=("wave",),           # ("wave",) | ("wave","spectral") 等任一子集，默认只 wave
    mipmap_levels=1,              # 保留最细的 N 层，默认 1
)
s.feed(chunk_bytes)               # 零拷贝 view，重活在 allow_threads 内
s.finish(src_timestamp=0, src_filesize=0) -> bytes

# bulk 快速入口（全量并行）
reapeaks.generate(pcm_bytes, sample_rate, channels, **同上) -> bytes
```

- 输入输出都是 `bytes`（不引 numpy crate）；`features` 为字符串 tuple（与 Python 风格一致）
- 开关语义与 Python 参考（带开关副本）同构；L2 差分对齐每个组合
- 参数校验（channels ≥ 1、divs 正数、features 合法、mipmap_levels ≥ 1）失败抛 `ValueError`

## 6. 精度契约（引自选型文档 §4/§7）

- wave：全整数 i16 min/max，**逐字节等于参考**
- spectral：±1 容差（FFT 舍入顺序差异）；公式与参考一致
- loudness：f32 输出 1 ulp 容差（i64 精确平方和 → f64）
- 验证分层见 `golden-verification.md`（L1/L2/L3）

## 7. v1 明确不做

- 编译期 feature 开关（wave/spectral/loudness）——运行期参数即可
- progress 回调、numpy 数组交换、symphonia 解码、Rust 内 spawn ffmpeg
- 若上表项将来需要，单独评估，不进 v1