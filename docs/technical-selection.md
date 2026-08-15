# reapeaks 生成优化 — Rust/PyO3 技术选型

> 状态：草案（2025-08）｜范围：确定值得引用的 crate 与整体技术路线，不涉及具体实现细节

## 1. 背景与瓶颈定位

上游链路（`reapeaks-knowledge/reapeaks.py`）：

```
媒体文件 ──ffmpeg 子进程──▶ WAV pipe(64KB 块) ──▶ Python _ReaPeaksStreamer.feed() ──▶ finish() ──▶ .ReaPeaks 字节
```

瓶颈不在解码（ffmpeg 是 C，速度远快于生成），而在**纯 Python 的逐峰值处理循环**：

| 层 | 每峰值成本（Python 侧） | 1 小时 48kHz 立体声规模 |
|---|---|---|
| wave（3 层 mipmap） | 每声道 2 次 `struct.pack("<hh")` + 循环/属性开销 | fine 层 div=147 → **~108 万峰值**，共 3 层 |
| spectral（3 层） | 每次 2048 点 numpy FFT + 窗函数 + 切片 + `struct.pack` | 同上，**每小时 108 万次 FFT**，每层还有 Python while 循环 |
| loudness（2 层） | 每次 `math.sqrt` + `struct.pack` | div≈1102 → ~14 万峰值 |

规模参考：1 小时 48kHz×2ch×s16le ≈ **635 MB PCM**（2 小时 ≈ 1.27 GB）。解释器层每峰值 ~1–10 µs 开销，乘上数百万峰值，单文件生成是分钟级甚至更糟，且与时长线性增长。

**结论**：Rust 重写流式内核，把解释器循环换成原生循环（每峰值几十 ns），并用多核并行，是正确方向。ffmpeg 解码子进程可以保留，只替换 `feed/finish` 内核。

## 2. 目标形态

- PyO3 扩展包（maturin 构建），对外接口与 `_ReaPeaksStreamer` 同构：
  - `ReapeaksStreamer(sr, channels, divs=None, features=..., mipmaps=...)` → `.feed(bytes) / .finish() -> bytes`（streaming、内存有界，**无缝替换**上游调用）
  - 另提供 bulk 入口 `generate(sr, channels, pcm: bytes, ...) -> bytes`（一次性传入，内部可全量并行，最快路径）
- 特性开关（对应 prompt.md 要求）：
  - `features`：wave / spectral / loudness 各自开关，默认只开 wave
  - `mipmaps`：三层保留哪些层，默认只保留最精细层
- 全程 `py.allow_threads` 释放 GIL；返回 `PyBytes`。

## 3. 候选 crate 总览

| crate | 版本 | 许可证 | 用途 | 结论 |
|---|---|---|---|---|
| **pyo3** | 0.29.2 | MIT/Apache-2.0 | Python 绑定，GIL 释放，`abi3` 稳定 ABI | ✅ **必选**（本机 rustc 1.91 ≥ MSRV 1.83，Python 3.13.5 支持） |
| **rayon** | 1.12.0 | MIT/Apache-2.0 | 数据并行：按峰值/按 bucket 行/按通道切分 | ✅ **必选** |
| **realfft** | 3.5.0 | MIT | 实数 FFT（2048 点 R2C），planner 预计算 + 每线程 scratch，无重复分配 | ✅ **必选**（底层即 rustfft） |
| rustfft | 6.4.1 | MIT/Apache-2.0 | 通用 FFT；realfft 的传递依赖 | ✅ 传递引入（不直接依赖也可） |
| symphonia | 0.6.1 | **MPL-2.0** | 纯 Rust 解封装/解码（mp4/mkv/mp3/flac/ogg/aac/opus） | ⏸ 远期备选（见 §5）；MPL 文件级 copyleft，合规需评估 |
| numpy | 0.29.0 | BSD-2-Clause | 与 numpy 数组零拷贝互操作 | ⭕ 可选：仅当接口要直接收 numpy int16 数组；默认走 bytes 不引 |
| hound | 3.5.1 | Apache-2.0 | 纯 Rust 读 WAV | ⭕ 可选：WAV 直读快速路径，非本期必需 |
| **ebur128** | 0.1.10 | MIT | 真实 LUFS 计算（EBU R128/ITU-R BS.1770）：M/S/I、LRA、true peak、门控 | ⭕ 可选：仅当 loudness 层升级为真 LUFS 时引（见 §5），零依赖 |
| ebur128-stream | 0.2.0 | MIT | ebur128 的流式零分配包装 | ⭕ 与 ebur128 同进退；通常直接用 ebur128 即可 |
| bytes / memmap2 | 1.12.1 / 0.9.11 | 宽松 | 字节缓冲 / 内存映射 IO | ❌ 本期不需要 |
| criterion | dev-dep | MIT/Apache-2.0 | Rust 侧基准 | ⭕ 可选 dev-dep |
| maturin | 构建工具 | MIT/Apache-2.0 | pyproject 构建后端，产出 wheel | ✅ 工具（非 crate） |

## 4. 推荐核心栈与理由

**pyo3 0.29 + rayon 1.12 + realfft 3.5**，进程内只跑流式内核：

1. **pyo3 0.29**：当前最新稳定版（crates.io 权威查询），支持 Python 3.13（本机 3.13.5）；`abi3-py38` 特性让一个 wheel 覆盖所有 Python 版本；`py.allow_threads` 成熟可靠。
2. **rayon**：wave/loudness 是纯归约（min/max、平方和），spectral 每峰值独立 FFT——三种层都是天然宿主并行度最高的形态。streaming 下**块内并行**（每块数百~数千峰值），内存保持有界；bulk 入口可跨段并行。
3. **realfft**：频谱层是最大开销（每小时 108 万次 2048 点 FFT）。`RealFftPlanner` 一次性预计算 plan，每个 worker 线程复用一块 scratch buffer + 预生成 Hanning 窗——把"每次 FFT 重新分配"和 Python 循环全部消掉。2048 点 R2C 单次约 µs 级，多核下 1 小时文件的频谱层进秒级。
4. **精度设计**：wave 层全整数 i16 min/max（零精度损失）；loudness 用 i64 平方和累加（1.59e8×2³⁰ ≈ 1.7e17 < i64 max，不会溢出）再转 f64；spectral 公式照搬参考实现（argmax + 抛物线插值 + 平坦度→density）。

## 5. Loudness（响度）层选型

现状还原：参考实现（`reapeaks_generate.py::_feed_loud`）的 `-ord('r')` mipmap 存的是**每 bucket 纯 RMS**（`sqrt(Σx²/n)/32768`，每声道一个 f32）；parser（`reapeaks.py::_read_loudness`）也是按单 f32 读的（注释明确写了"Observed: stores ONE float…，not the two-float LUFS-M/LUFS-S pair"）。也就是说**当前实现并不是真正的 LUFS**：无 K 加权、无门控。

有现成库：**ebur128** v0.1.10（MIT，零依赖）— sdroege（GStreamer 核心维护者）对 C 库 libebur128（FFmpeg `loudnorm` / Audacity 同源）的 Rust 移植。能力：M（momentary）/ S（short-term）/ I（integrated 带门控）/ LRA（loudness range）、true peak、任意采样率（自算滤波器系数）、通过 EBU TECH 3341/3342 合规测试。API：`EbuR128::new(channels, rate, mode)` → `add_frames_i16/f32/f64`（直接吃我们的 s16 交错数据）→ `loudness_momentary() / loudness_shortterm() / loudness_global() / loudness_window(ms)`。

三种目标路径：

| 目标 | 是否需要引库 | 说明 |
|---|---|---|
| A. 保持现状：'r' 层 = 每 bucket 纯 RMS | ❌ 不需要 | i64 平方和累加逐字节可控，零新依赖 |
| B. 'r' 层升级为 REAPER 规范语义 z(i)（K 加权均方） | ❌ 不引库，自写 ~40 行 | BS.1770 两个双二阶滤波（38Hz 高通 + 1.5kHz 高架）+ 每 bucket 平方累加；ebur128 的窗口是 400ms/3s/整段门控，**不提供按任意 bucket 取 z(i) 的接口**（我们的 loudness div≈25ms/500ms，与 400ms 不对齐），用它反而别扭 |
| C. 提供整段真实响度扫描（LUFS-I 门控 / LRA / true peak，如"响度元数据"产品能力） | ✅ 引 ebur128 | 这正是它的定位：合规、省去自己实现门控/窗的出错点 |

注意：B 会改变文件语义（现在的 f32 RMS → z(i)），parser 与编辑器展示要同步改；C 不改文件格式，是额外能力。

## 6. 可选/远期项

- **symphonia 0.6.1**：彻底去掉 ffmpeg 子进程，Rust 内直接解封装+解码（音视频 mp4/mkv 常见音轨覆盖良好）。但：a) 本期瓶颈在生成不在解码，收益有限；b) MPL-2.0 许可证需要合规确认。→ 放 roadmap，v1 不引。
- **numpy crate**：仅在上游偏好直接传 numpy 数组时引入（需与 pyo3 0.29 版本匹配的 numpy 0.29）。默认接口保持 bytes，与现状零改动。

## 7. 风险与对策

| 风险 | 说明 | 对策 |
|---|---|---|
| **位级一致性** | wave 层整数运算可逐字节一致；loudness 的 f64 求和 / spectral 的 FFT（numpy pocketfft vs realfft 舍入顺序不同）无法保证逐位相同 | golden 测试分级断言：wave 逐字节；loudness 允许输出 f32 最后 1 ulp 差异；spectral 允许 freq/density 取整边界 ±1 |
| **streaming 并行度** | 64KB 块 ≈ 32k 帧，fine 层仅 ~200 峰值/块，并行度有限 | 三层×通道并行 + 块内按行切分已够用；Python 侧可加大 feed 块（1–4 MB）；bulk 入口提供全量并行 |
| **跨块状态** | wave 尾 bucket、spectral 2048 样本窗历史、loudness 尾 bucket 跨块传递 | 内核保留 carry 状态，语义与 Python 参考完全一致 |
| **MSRV/ABI** | pyo3 0.29 MSRV 1.83（本机 1.91 OK）；abi3 需编译器支持 | 没问题；锁定 Cargo.lock 与 pyproject 版本 |

## 8. 路线图

1. **阶段 0（本次）**：技术选型 ✅
2. **阶段 1**：cargo 工程 + maturin 脚手架，流式内核 wave 层，对 Python 参考做 golden 测试（先小文件）
3. **阶段 2**：loudness、spectral 层；features/mipmaps 开关
4. **阶段 3**：推 GitHub，主仓库 pyproject 引用远程源，Python 侧集成 + **小文件 benchmark**（须先装 numpy/maturin）
5. **阶段 4**：生成 1–2 小时大 WAV/视频做最终 benchmark，并行调优（rayon 线程数、bulk 入口）
6. **阶段 5（远期）**：symphonia 纯 Rust 解码替代 ffmpeg 子进程（需合规确认 MPL-2.0）

## 9. 待决问题

1. **接口形态**：只做 streaming（无缝替换现有调用），还是同时加 bulk `generate()` 快速入口？（推荐：两者都做）
2. **数据交换**：bytes（跟随现状）还是 numpy 数组（需引 numpy crate）？（推荐：bytes）
3. **一致性命中标准**：spectral 层接受"±1 容差"golden 断言吗？（推荐：接受，wave 层仍逐字节）
4. **loudness 语义**：走 §5 的 A（保持纯 RMS）/ B（K 加权 z(i)，改文件语义）/ C（另加 ebur128 真 LUFS 扫描）？（推荐：A + 远期 C）