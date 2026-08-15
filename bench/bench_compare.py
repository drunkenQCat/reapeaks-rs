"""reapeaks Rust 生成器基准：墙钟、吞吐、输出一致性。

用法：
    python bench/bench_compare.py [--seconds 60] [--sr 48000] [--channels 2]
        [--features wave,spectral,loudness] [--mipmap-levels 3]
        [--chunk 4194304] [--repeat 3] [--streaming] [--reference]

默认只测 Rust bulk `generate`；`--streaming` 追加测 chunked feed；
`--reference` 追加测 tests/python_ref 参考实现（需 numpy，建议仅用于小文件）。
输出 SHA-256 用于跨优化版本的一致性比对。
"""
from __future__ import annotations

import argparse
import hashlib
import sys
import time
from pathlib import Path

import numpy as np

import reapeaks


def gen_pcm(sr: int, channels: int, seconds: float, seed: int = 1) -> bytes:
    """确定性 s16le 交错 PCM：每声道不同频率正弦 + 固定种子噪声。"""
    rng = np.random.default_rng(seed)
    n = int(sr * seconds)
    t = np.arange(n, dtype=np.float64) / sr
    s16_parts: list[np.ndarray] = []
    for c in range(channels):
        tone = 0.55 * np.sin(2 * np.pi * (220.0 + 110.0 * c) * t)
        noise = 0.15 * rng.standard_normal(n)
        x = np.clip(tone + noise, -1.0, 1.0)
        s16_parts.append((x * 32767.0).astype("<i2"))
    if channels == 1:
        interleaved = s16_parts[0]
    else:
        interleaved = np.stack(s16_parts, axis=1).reshape(-1)
    return interleaved.tobytes()


def _best_ms(fn, repeat: int) -> float:
    best = float("inf")
    for _ in range(repeat):
        t0 = time.perf_counter()
        fn()
        dt = (time.perf_counter() - t0) * 1e3
        best = min(best, dt)
    return best


def bench(args: argparse.Namespace) -> None:
    pcm = gen_pcm(args.sr, args.channels, args.seconds)
    features = [f.strip() for f in args.features.split(",") if f.strip()]
    pcm_mb = len(pcm) / 1e6

    out = reapeaks.generate(
        pcm, args.sr, args.channels,
        features=features, mipmap_levels=args.mipmap_levels,
    )
    digest = hashlib.sha256(out).hexdigest()

    def do_generate() -> bytes:
        return reapeaks.generate(
            pcm, args.sr, args.channels,
            features=features, mipmap_levels=args.mipmap_levels,
        )

    ms = _best_ms(do_generate, args.repeat)
    thr = pcm_mb / (ms / 1e3)
    realtime = args.seconds / (ms / 1e3)

    print(f"[bulk generate] {pcm_mb:.2f} MB PCM, {args.sr}Hz/{args.channels}ch, "
          f"features={features}, levels={args.mipmap_levels}")
    print(f"  best of {args.repeat}: {ms:.1f} ms | {thr:.1f} MB/s | {realtime:.1f}x realtime")
    print(f"  output {len(out)} B  sha256={digest[:16]}")

    if args.streaming:
        def do_stream() -> bytes:
            s = reapeaks.ReapeaksStreamer(
                args.sr, args.channels,
                features=features, mipmap_levels=args.mipmap_levels,
            )
            for i in range(0, len(pcm), args.chunk):
                s.feed(pcm[i:i + args.chunk])
            return s.finish()

        out2 = do_stream()
        ms2 = _best_ms(do_stream, args.repeat)
        realtime2 = args.seconds / (ms2 / 1e3)
        print(f"[streaming chunk={args.chunk}] best of {args.repeat}: "
              f"{ms2:.1f} ms | {pcm_mb / (ms2 / 1e3):.1f} MB/s | {realtime2:.1f}x realtime")
        print(f"  streaming == bulk: {out2 == out}")

    if args.reference:
        sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tests" / "python_ref"))
        import reapeaks_generate as ref

        def do_ref() -> bytes:
            return ref.generate(
                pcm, args.sr, args.channels,
                features=tuple(features), mipmap_levels=args.mipmap_levels,
            )

        ms3 = _best_ms(do_ref, args.repeat)
        print(f"[python ref] best of {args.repeat}: {ms3:.1f} ms | "
              f"{args.seconds / (ms3 / 1e3):.1f}x realtime")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--seconds", type=float, default=60.0)
    ap.add_argument("--sr", type=int, default=48000)
    ap.add_argument("--channels", type=int, default=2)
    ap.add_argument("--features", default="wave")
    ap.add_argument("--mipmap-levels", type=int, default=1)
    ap.add_argument("--chunk", type=int, default=4 * 1024 * 1024)
    ap.add_argument("--repeat", type=int, default=3)
    ap.add_argument("--streaming", action="store_true")
    ap.add_argument("--reference", action="store_true")
    args = ap.parse_args()
    bench(args)
    return 0


if __name__ == "__main__":
    sys.exit(main())
