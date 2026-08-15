# `.ReaPeaks` Fixture 规格

> 目标：以 **REAPER 真机生成**的 `.ReaPeaks` 作为解析测试的真实基准，
> 验证 reapeaks-rs 生成器/解析器与 REAPER 的字节级格式兼容。
>
> 源 wav 由 `gen_fixtures.py` 确定性生成（`*.wav` 被 gitignore，不入库）；
> `.ReaPeaks` 由用户在 REAPER 中打开对应 wav 生成后放回本目录（可提交）。
> fixture 缺失时对应测试自动跳过，放回后自动启用。

## 生成流程

1. 运行 `.venv/bin/python tests/test_data/gen_fixtures.py`，生成 3 个 wav 到本目录。
2. 在 REAPER 中依次打开这 3 个 wav（REAPER 自动生成 `<name>.ReaPeaks`）。
3. 将生成的 `.ReaPeaks` 复制回 `tests/test_data/`。
4. 运行 unittest（`-m unittest discover -s tests -p "test_*.py"`），fixture 测试类应通过。

> 若 REAPER 未自动生成 peaks，可在 REAPER 中加载项目后触发一次峰值构建
> （选中素材并播放 / 构建 peaks）。

## Fixture 清单

### 1. `tone30.wav.ReaPeaks`（主 fixture）

- **时长**：30 分钟（1800 s），覆盖长时间媒体的流式生成压力路径
- **采样率**：44.1 kHz，单声道
- **内容时间轴**（各段边界可对齐断言）：

| 时间段 | 内容 | 断言用途 |
|--------|------|---------|
| 0–10s | 静音 | 波形振幅≈0 的边界 |
| 10–600s | 200 Hz 纯音 | 低频段波形/频谱 |
| 600–900s | 粉噪声 | 宽频、非纯音频谱密度 |
| 900–1350s | 1 kHz 纯音 | 中频段 |
| 1350–1790s | 3 kHz 纯音 | 高频段 |
| 1790–1800s | 静音 | 末尾边界 |

- **叠加**：每 5 分钟（300 s）段最后 30 s 叠加白噪声（跨频段验证噪声尾），
  最后一个 5 分钟段保留纯音与末尾静音。

### 2. `tone_dual.wav.ReaPeaks`（双声道）

- **时长**：20 s
- **采样率**：44.1 kHz，**双声道**
- **内容**：左声道 1 kHz 纯音；右声道 500 Hz 纯音 + 白噪声叠加
- **断言用途**：多声道 wave/spectral 数据交错布局、每声道独立峰值

### 3. `tone_48k.wav.ReaPeaks`（采样率维度）

- **时长**：10 s
- **采样率**：**48 kHz**（视频标准），单声道
- **内容**：前 5 s 440 Hz 纯音，后 5 s 白噪声
- **断言用途**：48 kHz 下 division factor 计算路径（与 44.1 kHz 不同）

## 版本说明

REAPER 当前版本通常只产一种 `.ReaPeaks` 版本（RPKN v1.1 或 RPKL v1.2）。
fixture 只覆盖"当前 REAPER 的真实产物"；其余版本（RPKM v1.0 / 未覆盖的
v1.2）由合成构造补足，无需切换 REAPER 版本手动生成。

## 与生成器的对比口径

L3 生成对比断言（Rust/Python 参考生成 vs fixture）：

- 头部前 10 字节相等（magic + channels + mipmap 数 + 采样率）
- `src_filesize` 字段相等（`gen_fixtures.py` 保证 wav 字节数与 MAW 一致）
- 各 mipmap 的 division factor 逐一相等
- 数据段长度差异 < 10%；`src_timestamp` 不比较（生成时刻不同）