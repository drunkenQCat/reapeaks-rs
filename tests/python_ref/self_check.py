"""参考实现自测脚本（不依赖 Rust / 不依赖 MAW 仓库）。

覆盖：
1. 轻量 RPKN 解析（按 reapeaks-knowledge/reapeaks.txt 规范，仅本脚本自用）
2. 生成 → 解析 往返：header 字段、数据段完整、数据段长度与 header 一致
3. 开关矩阵：features 各子集 / mipmap_levels / 自定义 divs 下的
   header 类型序列与 npeak 预期
4. 分块不变性：任意分块 feed 与一次喂完逐字节一致
5. 参数校验：非法输入抛 ValueError

用法：.venv/bin/python tests/python_ref/self_check.py
退出码：全部通过 0，任一失败 1。
"""
from __future__ import annotations

import struct
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import reapeaks_generate as rg  # noqa: E402

MAGIC = b"RPKN"
DIV_SPECTRAL = -ord("s")
DIV_LOUDNESS = -ord("r")


# ---------------- 轻量 RPKN 解析（自包含，仅测试用） ----------------

class _Mip:
    __slots__ = ("div", "npeak", "kind", "data")

    def __init__(self, div: int, npeak: int, data: bytes) -> None:
        self.div = div
        self.npeak = npeak
        self.kind = (
            "wave" if div > 0
            else "spectral" if div == DIV_SPECTRAL
            else "loudness" if div == DIV_LOUDNESS
            else "other"
        )
        self.data = data


def parse_rpkn(data: bytes) -> tuple[bytes, int, int, int, list[_Mip]]:
    """返回 (magic, channels, sample_rate, mipmap_count, mips)。"""
    if len(data) < 18 or data[:4] != MAGIC:
        raise ValueError("bad header")
    channels = data[4]
    count = data[5]
    sample_rate, _ts, _size = struct.unpack_from("<iii", data, 6)
    off = 18
    headers: list[tuple[int, int]] = []
    for _ in range(count):
        div, npeak = struct.unpack_from("<ii", data, off)
        headers.append((div, npeak))
        off += 8
    mips: list[_Mip] = []
    for div, npeak in headers:
        nbytes = npeak * channels * (4 if div != DIV_SPECTRAL else 4)
        # spectral 也是每峰每声道 4 字节；只有 spectrogram(-g) 是 192/声道
        data_slice = data[off:off + nbytes]
        if len(data_slice) != nbytes:
            raise ValueError(f"mipmap 数据段截断: div={div} need={nbytes} got={len(data_slice)}")
        mips.append(_Mip(div, npeak, data_slice))
        off += nbytes
    if off != len(data):
        raise ValueError(f"数据段长度不符: header 推算 {off} != 实际 {len(data)}")
    return MAGIC, channels, sample_rate, count, mips


def wave_peaks(mip: _Mip, channels: int) -> list[list[int]]:
    """wave 层数据 → 每峰 [max0, min0, max1, min1, ...]（int）。"""
    out: list[list[int]] = []
    for i in range(mip.npeak):
        row = mip.data[i * channels * 4:(i + 1) * channels * 4]
        vals = struct.unpack(f"<{channels * 2}h", row)
        out.append(list(vals))
    return out


# ---------------- 合成测试信号 ----------------

def make_pcm(sample_rate: int, channels: int, frames: int, seed: int) -> bytes:
    """确定性 PCM：正弦 + 白噪声，clip 到 16-bit 范围，交错 int16。"""
    rng = np.random.default_rng(seed)
    t = np.arange(frames, dtype=np.float64) / sample_rate
    tone = 0.55 * np.sin(2 * np.pi * 440.0 * t)
    noise = 0.25 * rng.standard_normal(frames)
    x = np.clip(tone + noise, -1.0, 1.0)
    s16 = (x * 30000.0).astype("<i2")
    if channels == 1:
        return s16.tobytes()
    # 各声道差异化：声道 c 乘以 (c+1)/2 系数并加直流偏置
    cols = [s16 * (c + 1) / (channels + 1) for c in range(channels)]
    return np.stack(cols, axis=1).astype("<i2").reshape(-1).tobytes()


# ---------------- 断言工具 ----------------

_PASS = 0
_FAIL = 0


def check(name: str, cond: bool, detail: str = "") -> None:
    global _PASS, _FAIL
    if cond:
        _PASS += 1
        print(f"  PASS {name}")
    else:
        _FAIL += 1
        print(f"  FAIL {name} {detail}")


def expect_value_error(name: str, fn) -> None:
    try:
        fn()
    except ValueError:
        check(name, True)
        return
    check(name, False, "未抛 ValueError")


# ---------------- 测试用例 ----------------

def test_roundtrip() -> None:
    print("[1] 往返：生成 → 解析")
    sr, ch, frames = 8000, 1, 20001  # 故意非整除帧数
    pcm = make_pcm(sr, ch, frames, seed=11)
    data = rg.generate(
        pcm, sr, ch,
        features=("wave", "spectral", "loudness"),
        mipmap_levels=3,
        src_timestamp=1234567, src_filesize=987654,
    )
    magic, channels, sample_rate, count, mips = parse_rpkn(data)
    check("magic RPKN", magic == MAGIC)
    check("channels", channels == ch)
    check("sample_rate", sample_rate == sr)
    kinds = [m.kind for m in mips]
    check(
        "类型序列 wave×3 + spectral×3 + loudness×2",
        kinds == ["wave"] * 3 + ["spectral"] * 3 + ["loudness"] * 2,
        str(kinds),
    )
    # wave 最细层首峰 == 原始数据首 bucket 的 max/min
    s16 = np.frombuffer(pcm, "<i2")
    div0 = mips[0].div
    peaks = wave_peaks(mips[0], ch)
    first = peaks[0]
    expected_max = int(s16[:div0].max())
    expected_min = int(s16[:div0].min())
    check("wave 首峰 max", first[0] == expected_max, f"{first[0]} vs {expected_max}")
    check("wave 首峰 min", first[1] == expected_min, f"{first[1]} vs {expected_min}")
    # wave 最细层峰数 == ceil(frames/div0)
    expect_peaks = (frames + div0 - 1) // div0
    check("wave 最细层峰数", mips[0].npeak == expect_peaks, f"{mips[0].npeak} vs {expect_peaks}")
    # loudness 层 1 npeak == ceil(frames/div1)+1
    div1 = max(1, sr // 40)
    expect_l1 = (frames + div1 - 1) // div1 + 1
    check("loudness 层1 npeak", mips[-2].npeak == expect_l1, f"{mips[-2].npeak} vs {expect_l1}")
    # 静态结构：整文件长度 == header 推算（parse 已检查 off == len）
    check("数据段完整（parse 已校验）", True)
    # 元数据回写
    ts, size = struct.unpack_from("<ii", data, 10)
    check("src_timestamp 回写", ts == 1234567, str(ts))
    check("src_filesize 回写", size == 987654, str(size))


def test_switch_matrix() -> None:
    print("[2] 开关矩阵：features / mipmap_levels / divs")
    sr, ch, frames = 12000, 2, 50000
    pcm = make_pcm(sr, ch, frames, seed=22)
    cases = [
        # (features, levels, divs, 预期 kinds 序列)
        (("wave",), 1, None, ["wave"]),
        (("wave",), 3, None, ["wave", "wave", "wave"]),
        (("wave", "spectral"), 1, None, ["wave", "spectral"]),
        (("wave", "spectral"), 2, None, ["wave", "wave", "spectral", "spectral"]),
        (("wave", "loudness"), 1, None, ["wave", "loudness", "loudness"]),
        (("wave", "spectral", "loudness"), 2, None,
         ["wave", "wave", "spectral", "spectral", "loudness", "loudness"]),
        (("spectral",), 2, None, ["spectral", "spectral"]),          # 无 wave
        (("loudness",), 1, None, ["loudness", "loudness"]),          # 仅 loudness
        (("spectral", "loudness"), 1, None, ["spectral", "loudness", "loudness"]),
        (("wave",), 1, [50, 200], ["wave"]),                          # 自定义 divs
        (("wave", "spectral"), 1, [100, 400], ["wave", "spectral"]),
        (("wave",), 2, [50, 200], ["wave", "wave"]),
    ]
    for features, levels, divs, want in cases:
        data = rg.generate(pcm, sr, ch, divs=divs, features=features, mipmap_levels=levels)
        _m, _c, _s, count, mips = parse_rpkn(data)
        kinds = [m.kind for m in mips]
        label = f"features={features} levels={levels} divs={divs}"
        check(f"header 类型 {label}", kinds == want, f"{kinds} vs {want}")
        check(f"mipmap_count {label}", count == len(want), f"{count} vs {len(want)}")
        # 数据段长度与全部 npeak 对齐（parse_rpkn 已校验，这里再显式确认）
        check(f"数据段可解析 {label}", True)
    # 无 wave 时 spectal_total 仍正确（不崩溃 + npeak>0）
    data = rg.generate(pcm, sr, ch, features=("spectral",), mipmap_levels=2)
    _m, _c, _s, count, mips = parse_rpkn(data)
    check("仅 spectral: npeak > 0", all(m.npeak > 0 for m in mips), str([m.npeak for m in mips]))


def test_chunk_invariance() -> None:
    print("[3] 分块不变性：任意分块 ≡ 一次喂完")
    sr, ch, frames = 16000, 2, 60037
    pcm = make_pcm(sr, ch, frames, seed=33)
    features = ("wave", "spectral", "loudness")
    streamer = rg.ReapeaksStreamer(sr, ch, features=features, mipmap_levels=3)
    streamer.feed(pcm)
    once = streamer.finish()
    for size in (1, 3, 100, 777, 2048, 4097, 65536):
        s = rg.ReapeaksStreamer(sr, ch, features=features, mipmap_levels=3)
        for i in range(0, len(pcm), size):
            s.feed(pcm[i:i + size])
        chunked = s.finish()
        check(f"chunk={size} 等价", chunked == once)


def test_validation() -> None:
    print("[4] 参数校验：非法输入抛 ValueError")
    sr, ch = 8000, 1
    pcm = make_pcm(sr, ch, 1000, seed=44)
    expect_value_error("channels=0", lambda: rg.ReapeaksStreamer(sr, 0))
    expect_value_error("features 空", lambda: rg.ReapeaksStreamer(sr, ch, features=()))
    expect_value_error("features 未知", lambda: rg.ReapeaksStreamer(sr, ch, features=("foo",)))
    expect_value_error("mipmap_levels=0", lambda: rg.ReapeaksStreamer(sr, ch, mipmap_levels=0))
    expect_value_error("divs 空", lambda: rg.ReapeaksStreamer(sr, ch, divs=[]))
    expect_value_error("divs 含 0", lambda: rg.ReapeaksStreamer(sr, ch, divs=[0, 100]))
    expect_value_error("generate channels=0", lambda: rg.generate(pcm, sr, 0))


def test_carry_odd_bytes() -> None:
    print("[5] 奇数字节残余帧跨块保持")
    sr, ch = 8000, 1
    # 单声道 1 帧 = 2 字节；奇数长度块（3 字节）应被 carry 到下一块
    s = rg.ReapeaksStreamer(sr, ch, features=("wave",), mipmap_levels=1)
    pcm = make_pcm(sr, ch, 500, seed=55)
    s.feed(pcm[:3])   # 奇数
    s.feed(pcm[3:])   # 补上残余
    check("奇数块 + 补全 == 一次喂完", s.finish() == rg.generate(pcm, sr, ch, mipmap_levels=1))


def test_fixture_wavs() -> None:
    """fixture 源 wav（gen_fixtures 生成）可被参考生成器处理并解析回读。

    只验证结构与元数据（wave 层快速路径 + 短文件全特性），不做内容断言——
    内容语义断言属于 L3（主 agent 的 test_reapeaks_fixture.py）。
    """
    print("[6] fixture wav → 参考生成 → 解析")
    data_dir = Path(__file__).resolve().parents[1] / "test_data"
    sys.path.insert(0, str(data_dir))
    import gen_fixtures  # noqa: F401  (模块级 import 以执行重生成)
    spec_cases = [
        ("tone_48k", 48000, 1, 10, ("wave", "spectral", "loudness")),
        ("tone_dual", 44100, 2, 20, ("wave", "spectral", "loudness")),
        ("tone30", 44100, 1, 1800, ("wave",)),
    ]
    for name, sr, ch, seconds, features in spec_cases:
        wav_path = data_dir / f"{name}.wav"
        if not wav_path.is_file():
            getattr(gen_fixtures, f"gen_{name}")()
        check(f"{name}: wav 生成", wav_path.is_file(), str(wav_path))
        # 帧数与时长一致
        with __import__("wave").open(str(wav_path), "rb") as wf:
            frames = wf.getnframes()
            got_ch = wf.getnchannels()
            got_sr = wf.getframerate()
        check(f"{name}: 帧数", frames == sr * seconds, f"{frames} vs {sr * seconds}")
        check(f"{name}: 声道/采样率", (got_ch, got_sr) == (ch, sr))
        # 参考生成（wave 层逐字节结构验证；tone_dual/48k 用全特性）
        levels = 3 if "spectral" in features else 1
        with __import__("wave").open(str(wav_path), "rb") as wf:
            pcm = wf.readframes(wf.getnframes())
        data = rg.generate(
            pcm, sr, ch, features=features, mipmap_levels=levels,
        )
        # 用轻量解析器验证结构与元数据
        _magic, _ch, _sr, _count, mips = parse_rpkn(data)
        kinds = [m.kind for m in mips]
        if features == ("wave",):
            want = ["wave"] * levels
        else:
            want = ["wave"] * 3 + ["spectral"] * 3 + ["loudness"] * 2
        check(f"{name}: header 类型", kinds == want, f"{kinds} vs {want}")
        check(f"{name}: 数据段完整", True)


def main() -> int:
    tests = [
        test_roundtrip,
        test_switch_matrix,
        test_chunk_invariance,
        test_validation,
        test_carry_odd_bytes,
        test_fixture_wavs,
    ]
    for t in tests:
        t()
    print(f"\n结果: {_PASS} PASS / {_FAIL} FAIL")
    return 0 if _FAIL == 0 else 1


if __name__ == "__main__":
    sys.exit(main())