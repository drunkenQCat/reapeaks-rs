"""生成 .ReaPeaks fixture 的源 wav（供 REAPER 生成 .ReaPeaks 用）。

只生成 wav；``*.wav`` 已被 gitignore，不入库。``.ReaPeaks`` 由用户在
REAPER 中打开对应 wav 后生成、复制回 ``tests/test_data/``（可提交）。
内容设计见 ``FIXTURES.md``。

信号设计（时间轴）与 MAW 的 fixture 等价：tone30 / tone_dual / tone_48k
三个文件覆盖长时长、多声道、多采样率三个维度；wav 字节数与 MAW 完全一致
（同采样率/时长/声道 → 文件名长度一致），使 L3 的 src_filesize 断言成立。
代码独立编写；随机种子固定，生成结果可复现。
"""
from __future__ import annotations

import wave
from pathlib import Path

import numpy as np

OUT = Path(__file__).resolve().parent

SR_44K = 44100
SR_48K = 48000


def _write_wav(path: Path, sample_rate: int, samples: np.ndarray) -> None:
    """float 样本（[-1, 1]）写为 16-bit PCM WAV。

    ``samples`` 为 (n,)（单声道）或 (n, channels)（交错）。
    量化与 MAW 相同：``np.clip(np.round(x*32767), -32768, 32767)``。
    """
    s16 = np.clip(np.round(samples * 32767.0), -32768, 32767).astype("<i2")
    with wave.open(str(path), "wb") as wf:
        wf.setnchannels(1 if samples.ndim == 1 else samples.shape[1])
        wf.setsampwidth(2)
        wf.setframerate(sample_rate)
        wf.writeframes(s16.tobytes())


def _tone(freq: float, sample_rate: int, n: int, amp: float = 0.8) -> np.ndarray:
    """固定幅度正弦。"""
    t = np.arange(n, dtype=np.float64) / sample_rate
    return amp * np.sin(2.0 * np.pi * freq * t)


def _pink_noise(n: int, rng: np.random.Generator) -> np.ndarray:
    """频域 1/sqrt(f) 加权生成粉噪声（确定性）。

    DC bin 置 1.0 避免除零（保持其原值）；归一化到峰值 1.0。
    """
    spectrum = np.fft.rfft(rng.standard_normal(n))
    freqs = np.fft.rfftfreq(n)
    freqs[0] = 1.0
    signal = np.fft.irfft(spectrum / np.sqrt(freqs), n)
    peak = np.max(np.abs(signal))
    return signal / peak if peak > 0 else signal


def gen_tone30() -> None:
    """30 分钟主 fixture：静音/200Hz/粉噪声/1kHz/3kHz/静音 + 噪声尾叠加。

    时间轴（便于按段断言）：
      0-10s 静音 / 10-600s 200Hz / 600-900s 粉噪声 / 900-1350s 1kHz /
      1350-1790s 3kHz / 1790-1800s 静音；
    每 5 分钟段（0-300s/300-600s/600-900s/900-1200s/1200-1500s）的最后
    30s 叠加白噪声，最后一个 5 分钟段（1500-1800s）保持纯音与末尾静音。
    """
    sr = SR_44K
    n = sr * 1800
    rng = np.random.default_rng(42)
    samples = np.zeros(n, dtype=np.float64)
    samples[10 * sr:600 * sr] = _tone(200.0, sr, (600 - 10) * sr)
    samples[600 * sr:900 * sr] = _pink_noise(300 * sr, rng) * 0.8
    samples[900 * sr:1350 * sr] = _tone(1000.0, sr, 450 * sr)
    samples[1350 * sr:1790 * sr] = _tone(3000.0, sr, 440 * sr)
    for start in range(0, 1500, 300):
        seg = slice((start + 270) * sr, (start + 300) * sr)
        samples[seg] += rng.uniform(-0.15, 0.15, size=seg.stop - seg.start)
    _write_wav(OUT / "tone30.wav", sr, samples)
    print("wrote", OUT / "tone30.wav")


def gen_tone_dual() -> None:
    """双声道 fixture：左 1kHz 纯音，右 500Hz 纯音 + 白噪声叠加。"""
    sr = SR_44K
    n = sr * 20
    rng = np.random.default_rng(7)
    left = _tone(1000.0, sr, n)
    right = _tone(500.0, sr, n) * 0.6 + rng.uniform(-0.15, 0.15, size=n)
    _write_wav(OUT / "tone_dual.wav", sr, np.stack([left, right], axis=1))
    print("wrote", OUT / "tone_dual.wav")


def gen_tone_48k() -> None:
    """48kHz fixture：前 5s 440Hz 纯音，后 5s 白噪声。"""
    sr = SR_48K
    n = sr * 10
    rng = np.random.default_rng(99)
    samples = np.zeros(n, dtype=np.float64)
    samples[:5 * sr] = _tone(440.0, sr, 5 * sr)
    samples[5 * sr:] = rng.uniform(-0.5, 0.5, size=5 * sr)
    _write_wav(OUT / "tone_48k.wav", sr, samples)
    print("wrote", OUT / "tone_48k.wav")


def main() -> None:
    gen_tone30()
    gen_tone_dual()
    gen_tone_48k()


if __name__ == "__main__":
    main()