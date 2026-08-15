"""REAPER .ReaPeaks（RPKN v1.1）生成器 —— 带生成开关的参考实现。

本模块是 reapeaks-rust（PyO3 内核）的**语义基准**：给定相同输入与相同
开关，Rust 输出与本实现的契约为：

- wave 层：逐字节一致（纯整数 min/max 运算）
- spectral 层：±1 容差（FFT 舍入顺序差异）
- loudness 层：1 ulp 容差（f64 求和路径差异）

相对 MAW 早期版本（reapeaks_generate.py），本文件按相同算法不变式
**独立重写**（结构与措辞重新组织），并新增两个生成开关：

- ``features``：``("wave",)`` / ``("wave","spectral")`` /
  ``("wave","loudness")`` 等任一子集（默认只生成 wave）。
  spectral 层组镜像 wave 层的 division factors；loudness 固定两层
  （div = sr//40、sr//2），由 loudness 开关独立控制。
- ``mipmap_levels``：保留最细的 N 层（默认 1），截断 wave/spectral 层组；
  loudness 层数不受影响。

接口保持流式：``feed()`` 接收交错 int16 字节，``finish()`` 输出完整
.ReaPeaks 字节。另有 ``generate()`` 便捷函数（与 Rust 侧 ``generate``
对应）。参考格式规范见 ``reapeaks-knowledge/reapeaks.txt``。
"""
from __future__ import annotations

import math
import struct
from typing import Iterable, Sequence

import numpy as np

MAGIC = b"RPKN"  # v1.1
FFT_SIZE = 2048          # 频谱窗口长度
HALF_FFT = FFT_SIZE // 2
SPEC_TRIM_TAIL = 1280  # 对齐原始参考（reapeaks-knowledge）：fine_div*fine_npeak - 1280

# 特性规范顺序（与 Rust Feature::ALL 同构：wave → spectral → loudness）
FEATURE_ORDER = ("wave", "spectral", "loudness")

# loudness 两层以秒为粒度的分母：div = max(1, sr // 40) 与 max(1, sr // 2)
LOUDNESS_DIVISORS = (40, 2)


def choose_division_factors(sample_rate: int) -> list[int]:
    """默认 wave 层定义：约 300 / 20 / 1 峰每秒（REAPER 风格）。"""
    return [
        max(1, sample_rate // 300),
        max(1, sample_rate // 20),
        sample_rate,
    ]


def _loudness_divisors(sample_rate: int) -> list[int]:
    return [max(1, sample_rate // d) for d in LOUDNESS_DIVISORS]


def _normalize_features(features: Iterable[str] | None) -> list[str]:
    """校验、去重并按规范顺序排列特性名。"""
    if features is None:
        return ["wave"]
    names = list(features)
    if not names:
        raise ValueError("features 不能为空")
    unknown = [n for n in names if n not in FEATURE_ORDER]
    if unknown:
        raise ValueError(f"未知特性名: {', '.join(unknown)}")
    # 去重并保持 FEATURE_ORDER 顺序（与 Rust 契约同构）
    return [name for name in FEATURE_ORDER if name in names]


def _spec_buffer(seg: np.ndarray, fftn: int = FFT_SIZE) -> np.ndarray:
    """构造 fftn 长度、Hanning 加窗、居中放置 seg 的频谱缓冲。"""
    buf = np.zeros(fftn, dtype=np.float64)
    seg_f = np.asarray(seg, dtype=np.float64) / 32768.0
    n = len(seg_f)
    # 与 MAW 原始参考 _spec_buf 一致：总是对有效段加 Hanning 窗（即使段长 == fftn），
    # 居中放置、其余零填充。
    n = min(n, fftn)
    start = (fftn - n) // 2
    win = np.hanning(n)
    buf[start:start + n] = seg_f[:n] * win
    return buf


def _freq_density(seg: np.ndarray, sample_rate: int, fftn: int = FFT_SIZE) -> tuple[float, float]:
    """主频（Hz）与密度（0..16383）：单次 FFT 的 argmax + 抛物线插值 + 平坦度。

    - freq：非 DC 峰 argmax 的 bin 频率 + 抛物线插值修正
    - density：对非 DC bin 的谱平坦度做对数映射，clamp 到 [1, 16383]
    """
    n = len(seg)
    if n < 8:
        return 0.0, 0.0
    buf = _spec_buffer(seg, fftn)
    spec = np.abs(np.fft.rfft(buf))
    bins = spec[1:]  # 丢弃 DC
    if bins.size == 0:
        freq = 0.0
    else:
        idx = int(np.argmax(bins)) + 1
        if idx <= 0 or idx >= len(spec) - 1:
            freq = 0.0
        else:
            y0, y1, y2 = float(spec[idx - 1]), float(spec[idx]), float(spec[idx + 1])
            den = y0 - 2.0 * y1 + y2
            delta = 0.5 * (y0 - y2) / den if abs(den) > 1e-12 else 0.0
            freq = (idx + delta) * (sample_rate / fftn)

    if bins.size == 0 or bins.sum() <= 0.0:
        density = 0.0
    else:
        geo_mean = np.exp(np.mean(np.log(np.maximum(bins, 1e-12))))
        arith_mean = float(np.mean(bins))
        flatness = geo_mean / arith_mean if arith_mean > 0.0 else 0.0
        if flatness <= 0.0:
            density = 0.0
        else:
            density = -2961.5 * math.log(flatness) + 3995.3
            density = max(1.0, min(16383.0, density))
    return freq, density


def _spectral_code(win: np.ndarray, sample_rate: int) -> int:
    """单窗口 32-bit 频谱码：freq(15 bits) | density(15 bits)。"""
    freq, density = _freq_density(win, sample_rate)
    if freq <= 0.0 or density <= 0.0:
        return 0
    f = int(round(freq))
    d = int(round(density))
    if f > 0x7FFF:
        f = 0x7FFF
    if d > 0x3FFF:
        d = 0x3FFF
    return f | (d << 15)


class ReapeaksStreamer:
    """增量构建 .ReaPeaks 字节的流式内核。

    构造参数与 Rust ``StreamerOptions`` 同构：

    :param sample_rate: 源采样率（Hz）
    :param channels: 声道数（>= 1）
    :param divs: 自定义 wave 层 division factors（升序，最细在前）；
        ``None`` 时取 ``choose_division_factors(sample_rate)``
    :param features: 启用特性子集（默认 ``("wave",)``），规范顺序输出
    :param mipmap_levels: 保留最细的 N 层 wave/spectral（默认 1）
    """

    def __init__(
        self,
        sample_rate: int,
        channels: int,
        divs: Sequence[int] | None = None,
        features: Iterable[str] | None = ("wave",),
        mipmap_levels: int = 1,
    ) -> None:
        if channels < 1:
            raise ValueError(f"channels 必须 >= 1，收到 {channels}")
        if divs is not None:
            divs = list(divs)
            if not divs:
                raise ValueError("divs 不能为空")
            if any(d < 1 for d in divs):
                raise ValueError("divs 必须 >= 1")
        else:
            divs = choose_division_factors(sample_rate)
        if mipmap_levels < 1:
            raise ValueError(f"mipmap_levels 必须 >= 1，收到 {mipmap_levels}")
        feats = _normalize_features(features)

        self.sample_rate = sample_rate
        self.channels = channels
        self.divs = list(divs)
        self.features = feats
        self.mipmap_levels = mipmap_levels

        # 参与生成的 wave/spectral 层（截断到最细 N 层）
        self._layer_divs = self.divs[:mipmap_levels]

        self._wave_on = "wave" in feats
        self._spectral_on = "spectral" in feats
        self._loudness_on = "loudness" in feats

        # wave 层：每层每声道部分 bucket 累加器 (maxs, mins, count) 或 None
        self._w_acc: list[tuple[np.ndarray, np.ndarray, int] | None] = [
            None for _ in self._layer_divs
        ]
        self._w_out = [bytearray() for _ in self._layer_divs]

        # spectral：上一块尾部 2048 样本 + 每层下一个窗口中心
        self._hist: np.ndarray | None = None
        self._spec_next = [0] * len(self._layer_divs)
        self._spec_out = [bytearray() for _ in self._layer_divs]

        # loudness：两层各自的平方和 / 计数 / 输出
        self._loud_sq: list[np.ndarray | None] = [None, None]
        self._loud_cnt = [0, 0]
        self._loud_out = [bytearray(), bytearray()]

        self._total = 0          # 已消费帧数（每声道）
        self._carry = b""        # 跨块不足一帧的残余字节

    # ---------------- wave ----------------

    def _flush_wave_layer(self, li: int, maxs: np.ndarray, mins: np.ndarray) -> None:
        """把一层的一个完整 bucket 追加为交错 min/max 对的 int16 字节。"""
        rows = np.empty((self.channels, 2), dtype="<i2")
        rows[:, 0] = maxs
        rows[:, 1] = mins
        self._w_out[li] += rows.reshape(-1).tobytes()

    def _feed_wave(self, block: np.ndarray) -> None:
        """逐层分桶归约交错 int16 块。block 形状为 (n, channels)。"""
        n = len(block)
        for li, div in enumerate(self._layer_divs):
            acc = self._w_acc[li]
            start = 0
            if acc is not None:
                maxs, mins, count = acc
                take = min(div - count, n)
                part = block[:take]
                maxs = np.maximum(maxs, part.max(axis=0))
                mins = np.minimum(mins, part.min(axis=0))
                count += take
                start = take
                if count >= div:
                    self._flush_wave_layer(li, maxs, mins)
                    self._w_acc[li] = None
                else:
                    self._w_acc[li] = (maxs, mins, count)
                    continue
            rest = block[start:]
            full = len(rest) // div * div
            if full:
                buckets = rest[:full].reshape(-1, div, self.channels)
                maxs = buckets.max(axis=1)
                mins = buckets.min(axis=1)
                rows = np.empty((len(maxs), self.channels, 2), dtype="<i2")
                rows[:, :, 0] = maxs
                rows[:, :, 1] = mins
                self._w_out[li] += rows.reshape(-1).tobytes()
            tail = rest[full:]
            if len(tail):
                self._w_acc[li] = (tail.max(axis=0), tail.min(axis=0), len(tail))

    # ---------------- spectral ----------------

    def _feed_spectral(self, block: np.ndarray) -> None:
        """逐层在窗口中心处计算 32-bit 频谱码；跨块窗口经 _hist 衔接。"""
        n = len(block)
        total_after = self._total + n
        hist = self._hist
        stream = np.concatenate([hist, block]) if hist is not None else block
        base = self._total - (len(hist) if hist is not None else 0)
        for li, div in enumerate(self._layer_divs):
            center = self._spec_next[li]
            out = self._spec_out[li]
            codes: list[int] = []
            chunk_start = len(out)
            while center + HALF_FFT <= total_after:
                s0 = max(0, center - HALF_FFT)
                win = stream[s0 - base: center + HALF_FFT - base]
                for c in range(self.channels):
                    codes.append(_spectral_code(win[:, c], self.sample_rate))
                center += div
            self._spec_next[li] = center
            if codes:
                out += np.array(codes, dtype="<i4").tobytes()
        self._hist = block[-FFT_SIZE:].copy()

    # ---------------- loudness ----------------

    def _flush_loudness_layer(self, li: int, sq: np.ndarray, count: int) -> None:
        """把一层的一个完整 bucket 追加为每声道 RMS f32 字节。"""
        rms = np.sqrt(sq / count) / 32768.0
        self._loud_out[li] += np.asarray(rms, dtype="<f4").tobytes()

    def _feed_loudness(self, block: np.ndarray) -> None:
        """按 sr//40、sr//2 两层累积平方和，逐 bucket 输出 RMS。"""
        n = len(block)
        divs = _loudness_divisors(self.sample_rate)
        for li, div in enumerate(divs):
            sq = self._loud_sq[li]
            if sq is None:
                sq = np.zeros(self.channels, dtype=np.float64)
                self._loud_sq[li] = sq
            cnt = self._loud_cnt[li]
            start = 0
            if cnt > 0:
                take = min(div - cnt, n)
                sq += np.square(block[:take].astype(np.float64)).sum(axis=0)
                cnt += take
                start = take
                if cnt >= div:
                    self._flush_loudness_layer(li, sq, cnt)
                    sq = np.zeros(self.channels, dtype=np.float64)
                    self._loud_sq[li] = sq
                    cnt = 0
            rest = block[start:]
            full = len(rest) // div * div
            if full:
                buckets = rest[:full].reshape(-1, div, self.channels)
                sqsum = np.square(buckets.astype(np.float64)).sum(axis=1)
                rms = np.sqrt(sqsum / div) / 32768.0
                self._loud_out[li] += np.asarray(rms, dtype="<f4").tobytes()
            tail = rest[full:]
            if len(tail):
                sq += np.square(tail.astype(np.float64)).sum(axis=0)
                cnt = len(tail)
            self._loud_sq[li] = sq
            self._loud_cnt[li] = cnt

    # ---------------- 公共接口 ----------------

    def feed(self, interleaved: bytes | bytearray | memoryview) -> None:
        """喂入一块交错 int16 小端字节（声道交错；任意分块均可）。"""
        raw = bytes(interleaved)
        if self._carry:
            raw = self._carry + raw
            self._carry = b""
        n_frames = len(raw) // (2 * self.channels)
        if n_frames == 0:
            self._carry = raw
            return
        head = raw[: n_frames * 2 * self.channels]
        block = np.frombuffer(head, dtype="<i2").reshape(n_frames, self.channels)
        if self._wave_on:
            self._feed_wave(block)
        if self._spectral_on:
            self._feed_spectral(block)
        if self._loudness_on:
            self._feed_loudness(block)
        self._total += n_frames
        self._carry = raw[n_frames * 2 * self.channels:]

    def _finest_npeak(self) -> int:
        """最细 wave 层实际（或等效）峰数——spectral trim 用。"""
        if self._wave_on:
            return len(self._w_out[0]) // (self.channels * 4)
        finest_div = self.divs[0]
        return (self._total + finest_div - 1) // finest_div

    def _spectral_total(self) -> int:
        """谱窗中心可达范围的上界：最细层峰数 × 最细 div − 1280。"""
        return self.divs[0] * self._finest_npeak() - SPEC_TRIM_TAIL

    def finish(self, src_timestamp: int = 0, src_filesize: int = 0) -> bytes:
        """收尾：冲刷残留、按层 trim/pad，组装完整 .ReaPeaks 字节。"""
        if self._wave_on:
            for li in range(len(self._layer_divs)):
                acc = self._w_acc[li]
                if acc is not None:
                    self._flush_wave_layer(li, acc[0], acc[1])
                    self._w_acc[li] = None
        if self._spectral_on:
            # 谱峰按 c_total // div 截断（与 header 的 npeak 一致）
            c_total = self._spectral_total()
            for li, div in enumerate(self._layer_divs):
                limit = max(0, c_total // div) * self.channels * 4
                del self._spec_out[li][limit:]
        if self._loudness_on:
            self._finalize_loudness()
        return self._assemble(src_timestamp, src_filesize)

    def _finalize_loudness(self) -> None:
        """loudness 收尾：层1 ceil+1 并 pad；层2 floor 截断。"""
        divs = _loudness_divisors(self.sample_rate)
        # 层1：残留冲刷后 pad 到 npeak1 = ceil(total/div) + 1
        if self._loud_cnt[0] > 0:
            sq = self._loud_sq[0]
            assert sq is not None
            self._flush_loudness_layer(0, sq, self._loud_cnt[0])
        npeak1 = (self._total + divs[0] - 1) // divs[0] + 1
        limit1 = npeak1 * self.channels * 4
        out1 = self._loud_out[0]
        if len(out1) < limit1:
            out1 += b"\x00" * (limit1 - len(out1))
        # 层2：floor，残留丢弃，无需 pad
        limit2 = self._total // divs[1] * self.channels * 4
        del self._loud_out[1][limit2:]

    def _assemble(self, src_timestamp: int, src_filesize: int) -> bytes:
        """按 RPKN v1.1 布局输出：头 + mipmap headers + 各层数据。"""
        wave_headers: list[tuple[int, int]] = []
        if self._wave_on:
            wave_headers = [
                (div, len(self._w_out[li]) // (self.channels * 4))
                for li, div in enumerate(self._layer_divs)
            ]
        spec_headers: list[tuple[int, int]] = []
        if self._spectral_on:
            c_total = self._spectral_total()
            spec_headers = [(-ord("s"), c_total // div) for div in self._layer_divs]
        loud_headers: list[tuple[int, int]] = []
        if self._loudness_on:
            divs = _loudness_divisors(self.sample_rate)
            loud_headers = [
                (-ord("r"), (self._total + divs[0] - 1) // divs[0] + 1),
                (-ord("r"), self._total // divs[1]),
            ]
        all_headers = wave_headers + spec_headers + loud_headers

        out = bytearray()
        out += MAGIC
        out += bytes([self.channels])
        out += bytes([len(all_headers)])
        out += struct.pack("<iii", self.sample_rate, src_timestamp, src_filesize)
        for div, npeak in all_headers:
            out += struct.pack("<ii", div, npeak)
        for buf in self._w_out:
            out += buf
        for buf in self._spec_out:
            out += buf
        for buf in self._loud_out:
            out += buf
        return bytes(out)


def generate(
    interleaved: bytes | bytearray | memoryview,
    sample_rate: int,
    channels: int,
    divs: Sequence[int] | None = None,
    features: Iterable[str] | None = ("wave",),
    mipmap_levels: int = 1,
    src_timestamp: int = 0,
    src_filesize: int = 0,
) -> bytes:
    """一次性生成：构造流式内核 → 整块喂入 → 收尾。与 Rust ``generate`` 对应。"""
    streamer = ReapeaksStreamer(
        sample_rate,
        channels,
        divs=divs,
        features=features,
        mipmap_levels=mipmap_levels,
    )
    streamer.feed(interleaved)
    return streamer.finish(src_timestamp=src_timestamp, src_filesize=src_filesize)