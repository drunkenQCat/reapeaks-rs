# reapeaks-rs 并行开发工作流（subagent + worktree + gh PR）

> 本文是 subagent 协作的"交接包"，subagent 开工前**必须通读**本文件与
> 其引用的契约文档。主 agent 与各 subagent 的分工、文件所有权、验收标准均以此为准。

## 1. 仓库拓扑

```
origin: https://github.com/drunkenQCat/reapeaks-rs.git
分支:   main（开发主线）
```

开发采用 **worktree + 特性分支 + gh PR** 模型：

```
阶段 1  main: 接口骨架（本契约）已提交
阶段 2  worktree A ── feat/rust-core ──▶ PR → main
        worktree B ── feat/python-ref ──▶ PR → main
阶段 3  主 agent 合并 PR、跑全量绿灯
阶段 4  红灯 → 按归属 send_message 续接 A/B → 回归
```

## 2. 文件所有权（冲突最小化）

| 路径 | 归属 | 说明 |
|---|---|---|
| `Cargo.toml` / `pyproject.toml` / `src/**` | **A（rust-core）** | Rust 内核 + PyO3 绑定 |
| `tests/python_ref/**` | **B（python-ref）** | 带开关的 Python 参考实现 |
| `tests/test_data/gen_fixtures.py` / `FIXTURES.md` | **B（python-ref）** | fixture 生成器（独立重写）+ 规格 |
| `tests/test_data/*.ReaPeaks` | 主 agent 已就位 | REAPER 真机 fixture（只读） |
| `tests/test_*.py` | **主 agent** | L1/L2/L3 验收测试（合入后写） |
| `docs/**`、`.gitignore` | 主 agent | 编排文档 |
| `bench/bench_compare.py` | **A（rust-core）** | 基准脚本（依赖 Rust 产物） |

## 3. 环境约定

```bash
# 公共编译产物目录（两个 worktree 共享，避免重复编译 pyo3）
export CARGO_TARGET_DIR=/home/deck/MyCode/reapeaks-rs/.cargo-target
# Python 虚拟环境（已装 numpy 2.5.2 + maturin 1.14.1）
.venv/bin/python   # 或 uv run python
```

验证命令（全量绿灯）：
```bash
CARGO_TARGET_DIR=.cargo-target cargo test
CARGO_TARGET_DIR=.cargo-target cargo clippy --all-targets -- -D warnings
CARGO_TARGET_DIR=.cargo-target cargo fmt --check
.venv/bin/python -m unittest discover -s tests -p "test_*.py"
```

## 4. Subagent 交接要求

- 每个 subagent 是**独立会话**：看不到主 agent 与另一 subagent 的对话。
- 开工先读：`docs/architecture.md`（Rust 结构）、`docs/golden-verification.md`
  （验收分层）、`docs/technical-selection.md`（crate 选型理由）、本文件。
- 契约：src/ 下所有 pub 签名、字段、测试名已在骨架中定死，**不要改签名**；
  只填 `todo!()` 的实现体和测试 body，去掉测试上的 `#[ignore]`。
- 提交规范：中文 commit message，**不附加任何 AI 署名**（Co-authored-by 等一律禁止）。
- 完成标准：自己拥有的模块 `cargo test` 相应测试全绿 + `cargo clippy` 无新警告
  + `cargo fmt`；推分支后 `gh pr create`（标题、描述给主 agent 评审）。

## 5. 各 subagent 任务书

### Subagent A：Rust 内核（feat/rust-core）

读 `docs/architecture.md` §2-§4，实现：
- `src/options.rs`：`StreamerOptions::new` 校验与默认值逻辑（骨架已含大部分，补细节）
- `src/wave.rs`：`WaveLayer` 全部方法 + 测试
- `src/loudness.rs`：`LoudnessLayer` 全部方法 + 测试
- `src/spectral.rs`：`SpectralLayer` + `freq_density`（realfft/rayon，重点）+ 测试
- `src/format.rs`：`assemble` + token helpers + 测试
- `src/streamer.rs`：`ReapeaksStreamer`（feed/finish、carry、trim/pad 语义）+ `splice_frames` + 测试
- `src/py.rs`：绑定薄层（detach GIL 已写，补齐类型转换细节，如有）
- `bench/bench_compare.py`：临时生成小 wav → 对比 Python 参考与 Rust 产物的一致性 + 计时

语义基准：`reapeaks-knowledge/reapeaks_generate.py` 与
`reapeaks-knowledge/reapeaks.txt`（格式规范）。**逐字节兼容 wave 层，spectral ±1，
loudness 1ulp**（见 golden-verification.md §2）。

完成自证：
```bash
CARGO_TARGET_DIR=.cargo-target cargo test          # 全部绿（含去 ignore 的契约测试）
CARGO_TARGET_DIR=.cargo-target cargo clippy --all-targets -- -D warnings
CARGO_TARGET_DIR=.cargo-target cargo fmt --check
# 若 py feature 可编（需 python3-dev），验证：
CARGO_TARGET_DIR=.cargo-target cargo check --features py
```

### Subagent B：Python 参考 + fixture 设施（feat/python-ref）

读 `docs/golden-verification.md` §2-L2/L3、§3、决策记录 §9，实现：
- `tests/python_ref/reapeaks_generate.py`：从 `reapeaks-knowledge/reapeaks_generate.py`
  **独立重写**（作者同源，可参考但代码结构重写），并补 `features` / `mipmap_levels`
  开关（语义与 Rust `StreamerOptions` 同构，见 architecture.md §5）：
  - `Streamer(sr, ch, divs=None, features=("wave",), mipmap_levels=1)` 构造等价物
  - 输出与 Rust 契约一致：默认 300/20/1 层、wave 层过滤、mipmap_levels 截断
- `tests/test_data/gen_fixtures.py`：独立重写（信号设计与 FIXTURES.md 一致，代码自写，
  固定种子），生成 tone30 / tone_dual / tone_48k 三个 wav（*.wav 不入库）
- `tests/test_data/FIXTURES.md`：独立编写（内容规格与 MAW 等价，措辞自写）
- 自测：参考实现自身"生成 → 解析"往返（可临时用 `reapeaks-knowledge/reapeaks.py`
  的 `ReaPeaksFile` 验证；解析器已在 MAW 验证过）

完成自证：
```bash
.venv/bin/python -c "import sys; sys.path.insert(0,'tests/python_ref'); import reapeaks_generate as r; print(r)"  # 可导入
.venv/bin/python tests/python_ref/self_check.py   # 自测脚本（往返 + 开关语义，B 自建）
```

## 6. PR 模板

```markdown
## 摘要
（一句话：实现了什么）

## 契约符合性
- [ ] src/ 签名未改动（仅填实现）
- [ ] 契约测试已去 #[ignore] 并全绿
- [ ] cargo test / clippy / fmt 通过
- [ ] 无 AI 署名

## 自证
（粘贴验证命令输出摘要）
```

## 7. 迭代规则（阶段 4+）

- 主 agent 合并后跑全量绿灯；红灯按**失败归属**分发：
  - Rust 侧 → `send_message` 续接 A（保留其 worktree 与上下文）
  - Python 参考/fixture 侧 → `send_message` 续接 B
  - 跨层集成 / 一行级小修 → 主 agent 自己收敛
- 每轮 3-5 个问题一批，修完立即回归；循环到全绿。
- 全部绿灯后：小文件 bench → push main → 按需发布。