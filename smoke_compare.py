"""临时 smoke test：新版参考 vs 原版参考（reapeaks-knowlege）逐字节对比。"""
import importlib.util
import sys

import numpy as np

sys.path.insert(0, "tests/python_ref")
import reapeaks_generate as new  # 新版（我的）

# 按路径加载原版（避免与新版同名冲突）
spec = importlib.util.spec_from_file_location(
    "reapeaks_generate_orig", "reapeaks-knowlege/reapeaks_generate.py"
)
orig = importlib.util.module_from_spec(spec)
spec.loader.exec_module(orig)


def make_pcm(sr, ch, n, seed):
    rng = np.random.default_rng(seed)
    t = np.arange(n, dtype=np.float64) / sr
    x = 0.6 * np.sin(2 * np.pi * 440 * t) + 0.2 * rng.standard_normal(n)
    x = np.clip(x, -1, 1)
    x16 = (x * 30000).astype("<i2")
    if ch == 1:
        return x16.tobytes()
    return np.stack([x16 * (c + 1) for c in range(ch)], axis=1).reshape(-1).tobytes()


def gen_orig(pcm, sr, ch, divs=None):
    s = orig._ReaPeaksStreamer(sr, ch, divs=divs)
    for i in range(0, len(pcm), 7776):
        s.feed(pcm[i:i + 7777])
    return s.finish()


def gen_new(pcm, sr, ch, divs=None, features=("wave", "spectral", "loudness"), levels=3):
    s = new.ReapeaksStreamer(sr, ch, divs=divs, features=features, mipmap_levels=levels)
    for i in range(0, len(pcm), 3334):
        s.feed(pcm[i:i + 3333])
    return s.finish()


cases = [
    (8000, 1, 20000, 1, None),
    (44100, 2, 100000, 2, None),
    (48000, 1, 50000, 3, [100, 1000, 48000]),
    (16000, 2, 30000, 4, None),
]
ok = True
for sr, ch, n, seed, divs in cases:
    pcm = make_pcm(sr, ch, n, seed)
    a = gen_orig(pcm, sr, ch, divs)
    b = gen_new(pcm, sr, ch, divs)
    if a == b:
        print(f"PASS byte-identical sr={sr} ch={ch} n={n} divs={divs} len={len(a)}")
    else:
        ok = False
        print(f"FAIL sr={sr} ch={ch} n={n}: orig={len(a)}B new={len(b)}B")
        for i, (x, y) in enumerate(zip(a, b)):
            if x != y:
                print(f"  first diff at byte {i}: orig={x} new={y}")
                break

print("ALL OK" if ok else "FAILED")
sys.exit(0 if ok else 1)
