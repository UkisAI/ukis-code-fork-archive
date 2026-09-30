"""Regenerate tests/fixtures/bc_gpqa: two tiny GPQA-shaped Benchmark_configs run dirs (base, current).

raw/ rows mimic common.chat() + gpqa.ask() output. score_gpqa.json is written by the REAL
Benchmark_configs `common.report()` (imported, nothing is served or run), so the validator's
runner-summary check is tested against the actual writer, not a hand-typed copy.

Usage: python make_bc_fixture.py /path/to/Benchmark_configs
"""
import json, shutil, sys, types
from pathlib import Path

sys.path.insert(0, str(Path(sys.argv[1]) / "benchmarks"))
import common  # noqa: E402

HERE = Path(__file__).parent / "bc_gpqa"
SEEDS, ITEMS = [0, 1, 2, 3, 4], 8


def base(i, s):
    """(status, correct): 4/8 right; seed 1 item 7 truncated after a right answer; seed 4 item 3 errors."""
    if s == 1 and i == 7:
        return "length", True
    if s == 4 and i == 3:
        return "error", False
    return "stop", i < 4


def current(i, s):
    """6/8 right, plus item 6 on even seeds."""
    return "stop", i < 6 or (i == 6 and s % 2 == 0)


def raw_row(key, seed, i, finish, right, tokens):
    gold = "ABCD"[i % 4]
    wrong = "ABCD"[(i + 1) % 4]
    if finish == "error":
        return dict(key=key, content="", reasoning="", finish_reason="error",
                    error="URLError: <urlopen error [Errno 111] Connection refused>", seconds=0.1, seed=seed, gold=gold, pred=None)
    pred = gold if right else wrong
    return dict(key=key, content=f"Reasoned.\nAnswer: {pred}", reasoning="thinking...", finish_reason=finish,
                prompt_tokens=412, completion_tokens=tokens, seconds=12.5, seed=seed, gold=gold, pred=pred)


def build(arm, fn, tokens):
    out = HERE / arm
    shutil.rmtree(out, ignore_errors=True)
    (out / "raw").mkdir(parents=True)
    rows = []
    for s in SEEDS:
        for i in range(ITEMS):
            key = f"s{s}_{i:03}"
            finish, right = fn(i, s)
            x = raw_row(key, s, i, finish, right, tokens + 10 * i)
            (out / "raw" / f"{key}.json").write_text(json.dumps(x, ensure_ascii=False))
            rows.append(x)
    a = types.SimpleNamespace(name="gpqa", model=f"toy-{arm}", out=out)
    # Exactly gpqa.py's grading line.
    common.report(a, [dict(x, correct=common.ok(x) and x["pred"] == x["gold"]) for x in rows])


build("base", base, 1000)
build("current", current, 800)
