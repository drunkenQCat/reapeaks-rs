# Golden 生成与验证方案（reapeaks-rs）

> 参考蓝本：`moys-asr-workflow/tests/test_data/`（`FIXTURES.md`、`gen_fixtures.py`、
> `test_reapeaks.py`、`test_reapeaks_fixture.py`）。本方案把 MAW 已跑通的
> "REAPER 真机 fixture + 确定性合成输入 + 分层断言" 机制平移/升级到 reapeaks-rs。

## 1. 总原则（继承自 MAW）

1. **媒体文件（wav）不入库**：`*.wav` 进 `.gitignore`（MAW 仓库根 `.gitignore` 已有 `*.wav` 与调试产物 `*.ReaPeaks.maw` 的约定，reapeaks-rs 照搬）；fixture 源 wav 由确定性生成器按需现场重建。
2. **`.ReaPeaks`（REAPER 真机产物）可入库**：它是解析字节级兼容性的真实基准，且是小型二进制数据。
3. **fixture 缺失时测试 auto-skip**，不阻塞其余测试；fixture 放回后自动启用（沿用 `_fixture_present` 模式）。
4. **验证命令与 MAW 一致**：`uv run python -m unittest discover -s tests -p "test_*.py"`；Rust 侧另跑 `cargo test`。
5. **等价性传递**：L2 差分测试锁死 "Rust 生成器 ≡ Python 参考实现"（wave 层逐字节）。因此 Python 参考与 REAPER 的兼容程度（MAW 已有 `GeneratedFixtureTests` 验证）自动被 Rust 版继承——我们不必重复论证 Rust 与 REAPER 的绝对关系，只需保证 "Rust ≡ Python" 这条链。

## 2. 三层 golden

### L1 — 单元向量（`cargo test`，纯 Rust，无 Python 依赖）

手工从格式规范（`reapeaks-knowledge/reapeaks.txt`）推导的微型期望值，锁死格式骨架：

- header 布局（magic `RPKN`、channels、mipmap 数、sr、timestamp/filesize 回写）
- 单 bucket 的 min/max 对与字节序（`<hh` 交错：Lmax Lmin Rmax Rmin …）
- bucket 边界：整除 / 尾部不足一 bucket（残留仍出 max/min）
- **chunk 切分不变性**：同一输入按随机大小（1..65536）分块 feed ≡ 一次喂完（streaming carry 正确性）
- mono / stereo / 多声道；非对称信号（防 min/max 写反）；全零、DC、满幅 ±32768
- 特征开关矩阵：`features`（wave/spectral/loudness 子集）与 `mipmaps`（层数子集）下 header 计数、npeak、数据段长度

### L2 — 差分 oracle（Python 参考实现为基准）

同一份输入字节分别喂 `_ReaPeaksStreamer`（参考，**本仓库内的带开关副本**，见决策 ③）与 `ReapeaksStreamer`（Rust），断言：

- **wave 层：逐字节相等**（整数运算，必须精确）
- spectral：`±1` 容差（freq/density 取整边界，FFT 舍入顺序不同）
- loudness：f32 输出 `1 ulp` 容差（i64 精确平方和 → f64，与 numpy 求和路径的舍入差异）
- 各特征开关组合下输出与参考一致（参考已补 features/mipmaps 开关，语义与 Rust 同构）
- `finish()` 的 header 元数据（src_timestamp/src_filesize）与参考一致

输入生成器**不要求与 MAW 相同**：差分测试只需要两边吃同一份字节。用纯 Python 确定性合成（正弦 + 定点 LCG 噪声）即可，不强制 numpy。

### L3 — REAPER 真机 fixture（语义验证，复用 MAW 资产）

把 MAW 已提交的 3 个 REAPER 真机 `.ReaPeaks` 拷入 `tests/test_data/`：

| fixture | 规模 | 覆盖维度 |
|---|---|---|
| `tone30.wav.ReaPeaks` | 4.9 MB（30 分钟） | 长时长、分段内容（静音/200Hz/粉噪/1kHz/3kHz/静音）、缺 fixture 时可跳过 |
| `tone_dual.wav.ReaPeaks` | 109 KB（20s 立体声） | 多声道交错布局、每声道独立峰值 |
| `tone_48k.wav.ReaPeaks` | 27 KB（10s @48kHz） | 48kHz 的 division factor 路径 |

断言沿用 MAW 三类（具体照抄其实现思路）：

1. **解析语义**（`FixtureReaPeaksTests` 式）：header 字段；按段验证波形形状（静音段振幅≤1、纯音/噪声段振幅>40、非零占比>0.8）；立体声每声道振幅；48k 路径。
2. **生成对比**（`_compare_reapeaks` 式）：Rust 生成 vs fixture —— 前 10 字节相等（magic+channels+count+sr）、`bytes[14:18]` 相等（filesize）、**所有 mipmap 的 div 逐一相等**、数据段长度差异 <10%；src_timestamp 不比较。
3. **往返可解析**：Rust 输出能被 MAW 的 `reapeaks.ReaPeaksFile` 完整解析（`data_end == len(data)`），并可用 `load_waveform_payload` / `load_spectral_payload` 提取（源签名匹配）。

> L3 的 wav 源由确定性生成器重建（见 §3）；生成器为本仓库独立重写（决策 ①），重建的 wav 与 MAW 信号设计一致——div 由 sr 决定、语义断言对具体字节不敏感。

## 3. 输入（fixture 源 wav）生成规则

- `tests/test_data/gen_fixtures.py`：独立重写（借鉴 MAW 的信号分段设计，代码自写）：
  - `tone30`：44.1kHz 单声道 30 分钟；0-10s 静音 / 10-600s 200Hz / 600-900s 粉噪声 / 900-1350s 1kHz / 1350-1790s 3kHz / 1790-1800s 静音；每 5 分钟末 30s 叠加确定性白噪声；固定种子。
  - `tone_dual`：44.1kHz 20s 立体声；左 1kHz，右 500Hz+噪声。
  - `tone_48k`：48kHz 10s；前 5s 440Hz + 后 5s 噪声。
- 统一 `_write_wav`（float → int16 PCM，`np.clip(np.round(x*32767))`）。
- `*.wav` gitignore；测试 `setUpClass` 在 `.ReaPeaks` 存在而 wav 缺失时自动生成、`tearDownClass` 清理（照抄 MAW 模式）。
- 大文件（1h/2h 性能基准）**不入库**：bench 脚本运行时临时生成（见 §6）。

## 4. 验证矩阵

| 层 | 工具 | 环境依赖 | 内容 |
|---|---|---|---|
| L1 | `cargo test` | 无 | 格式骨架、bucket 边界、chunk 切分不变性、声道、开关矩阵 |
| L2 | `python -m unittest`（`test_reapeaks_rust_differential.py`） | numpy（可选）、Python 参考实现、已构建的 reapeaks | 差分：wave 逐字节 / spectral ±1 / loudness 1ulp；开关组合 |
| L3 | `python -m unittest`（`test_reapeaks_fixture.py`） | numpy、fixture 文件（缺失 auto-skip） | 语义断言 + 生成对比 + 往返可解析 |
| 集成 | `python -m unittest`（`test_media_pipe.py`） | ffmpeg | 真实媒体经 ffmpeg pipe → Rust streamer → 生成 → MAW parser 解析 → 源签名匹配 |
| 性能 | `bench/bench_compare.py`（独立脚本，不进 CI 断言） | numpy、ffmpeg、参考实现 | 小文件 sanity → 1h / 2h：墙钟、峰值内存、输出一致性抽查 |

## 5. 必须锁死的不变式

1. chunk 切分不变性（feed 任意分块 ≡ 一次喂完）——ffmpeg pipe 路径的根基
2. 特征开关任意组合下，输出结构与参考一致（wave 层仍逐字节）
3. Rust 输出可被 MAW parser 完整解析，头部元数据与源媒体签名匹配
4. wave 层对任意输入逐字节等于 Python 参考
5. 长时间媒体内存有界（bench 记录 RSS，避免整段缓冲）

## 6. 性能验证（独立脚本）

`bench/bench_compare.py`：
- 输入：临时生成的确定性 wav（1s / 1min sanity，最终 1h、2h 各一；2h 44.1kHz 立体声 ≈ 1.27 GB）
- 路径 A（现状）：ffmpeg pipe → Python `_ReaPeaksStreamer`
- 路径 B（新）：ffmpeg pipe → Rust `ReapeaksStreamer`（同接口）
- 输出：墙钟、峰值 RSS、输出 SHA-256（wave 层）一致性
- 按 prompt.md 约定：小文件先跑，全部功能完成后再生成 1h/2h 大文件做最终验收。

## 7. 文件布局（reapeaks-rs）

```
Cargo.toml / pyproject.toml（maturin）
src/                      # Rust 内核：streamer + 各层 + pyclass
tests/                    # Python unittest（MAW 风格）
  test_reapeaks_rust_differential.py
  test_reapeaks_fixture.py
  test_media_pipe.py
  python_ref/             # 带开关的 Python 参考实现副本（路径待架构讨论定，见 §10）
  test_data/
    FIXTURES.md           # 重写（独立编写）
    gen_fixtures.py       # 重写（独立编写，决策 ①）
    tone30.wav.ReaPeaks   # 已入库（4.9 MB）
    tone_dual.wav.ReaPeaks# 已入库
    tone_48k.wav.ReaPeaks # 已入库
    .gitignore            # *.wav
bench/
  bench_compare.py
```

## 8. CI 建议（可选）

GitHub Actions：ubuntu-latest；`apt install python3-dev` + `pip install maturin numpy`；`maturin build` → `cargo test` → `unittest discover`；L3 自动跳过缺 fixture 的用例；fixture 库存在制品缓存中。

## 9. 决策记录（2025-08-15 确认）

1. **仓库公开；许可证策略**：
   - `gen_fixtures.py` 独立重写（代码自写，信号设计借鉴 MAW 的分段事实），避免 AGPL 传染。
   - L2 差分用的 Python 参考实现（带开关版，见 ③）随本仓库发布：作者同为 MAW 版权人，有权以宽松许可证（与 reapeaks-rs 主许可证一致）双授权该参考副本；MAW 侧文件保持 AGPL 不变。若后续希望参考副本继续 AGPL，需调整公开策略——此处先按宽松授权记录。
   - `tone*.wav.ReaPeaks` 为 REAPER 输出的事实数据文件，作为 fixture 入库。
2. **三个 REAPER fixture 全部入库**：`tone30.wav.ReaPeaks`（4.9 MB，长时长）、`tone_dual.wav.ReaPeaks`、`tone_48k.wav.ReaPeaks`；源 wav 仍 gitignore + 确定性重建。
3. **Python 参考补 features / mipmaps 开关**：参考实现升级为产品基线（与 Rust 同构的开关语义），L2 差分测试直接对比同仓库的带开关参考，覆盖全部开关组合；MAW 侧后续接入时可先用全开输出。
4. **CI 先本地跑**：按 MAW 节奏，发布前再定 GitHub Actions。

## 10. 架构相关事项（已定稿，见 `architecture.md`）

- Crate 形态：单 crate + optional `py` feature（pyo3 绑定 feature-gated）
- 模块划分：`options` / `streamer` / `wave` / `spectral` / `loudness` / `format` / `py`
- 开关接口形态：`features` 字符串 tuple、`mipmap_levels` int、`divs` 自定义列表；Python 参考同构补开关
- streaming 并行默认开（块内并行）；bulk `generate` 全量并行
- ffmpeg 分批喂保持不变（块 64KB → 1–4 MB），子进程管理留在 MAW