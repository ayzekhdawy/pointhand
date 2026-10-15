"""End-to-end scenarios against a real window. Run with: POINTHAND_E2E=1 pytest tests/e2e

These move the real mouse and type on the real desktop for a few seconds, so
they are skipped unless explicitly enabled.
"""

import json
import os
import subprocess
import sys
import time
from pathlib import Path

import pytest

pytestmark = pytest.mark.skipif(os.environ.get("POINTHAND_E2E") != "1", reason="set POINTHAND_E2E=1 to run")

TARGET = Path(__file__).with_name("target_app.py")
TITLE = "pointhand-e2e-target"


def read_events(path: Path) -> list[dict]:
    if not path.exists():
        return []
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def wait_for(predicate, timeout: float = 8.0, interval: float = 0.05):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(interval)
    raise AssertionError("condition not met in time")


@pytest.fixture(scope="module")
def target(tmp_path_factory):
    os.environ["POINTHAND_OVERLAY"] = "0"
    from pointhand import native

    native.enable_dpi_awareness()
    log_path = tmp_path_factory.mktemp("e2e") / "events.jsonl"
    process = subprocess.Popen([sys.executable, str(TARGET), str(log_path)])
    try:
        ready = wait_for(lambda: next((e for e in read_events(log_path) if e["kind"] == "ready"), None))
        yield {"log": log_path, "ready": ready}
    finally:
        process.kill()


def run(actions):
    from pointhand.server import run_actions

    results = run_actions(actions)
    assert not any("error" in line for line in results), results
    return results


def mark(log_path: Path) -> int:
    return len(read_events(log_path))


def events_after(log_path: Path, start: int, kind: str) -> list[dict]:
    return [e for e in read_events(log_path)[start:] if e["kind"] == kind]


def last_text(log_path: Path):
    texts = [e for e in read_events(log_path) if e["kind"] == "text"]
    return texts[-1]["value"] if texts else None


def test_focus_click_and_unicode_typing(target):
    log_path, ready = target["log"], target["ready"]
    run([{"action": "focus_window", "title": TITLE}])
    ex, ey = ready["entry"]
    run([{"action": "click", "x": ex, "y": ey}, {"action": "type", "text": "Merhaba ğüşiöçİ 123"}])
    wait_for(lambda: last_text(log_path) == "Merhaba ğüşiöçİ 123")


def test_select_all_and_replace(target):
    log_path = target["log"]
    run([{"action": "key", "keys": "ctrl+a"}, {"action": "type", "text": "replaced"}])
    wait_for(lambda: last_text(log_path) == "replaced")


def test_click_lands_on_exact_pixel(target):
    log_path, ready = target["log"], target["ready"]
    left, top, width, height = ready["canvas"]
    point = (left + width // 2, top + height // 2)
    start = mark(log_path)
    run([{"action": "click", "x": point[0], "y": point[1]}])
    hit = wait_for(lambda: events_after(log_path, start, "click") or None)[0]
    assert abs(hit["x"] - point[0]) <= 1 and abs(hit["y"] - point[1]) <= 1


def test_double_and_right_click(target):
    log_path, ready = target["log"], target["ready"]
    left, top, _w, _h = ready["canvas"]
    start = mark(log_path)
    run([{"action": "double_click", "x": left + 60, "y": top + 60},
         {"action": "right_click", "x": left + 120, "y": top + 60}])
    wait_for(lambda: events_after(log_path, start, "double") and events_after(log_path, start, "right"))


def test_drag_and_draw_produce_motion_and_release(target):
    log_path, ready = target["log"], target["ready"]
    left, top, _w, _h = ready["canvas"]
    start = mark(log_path)
    run([{"action": "drag", "x1": left + 50, "y1": top + 50, "x2": left + 250, "y2": top + 150}])
    release = wait_for(lambda: events_after(log_path, start, "release") or None)[-1]
    assert abs(release["x"] - (left + 250)) <= 1 and abs(release["y"] - (top + 150)) <= 1
    assert len(events_after(log_path, start, "motion")) >= 5

    start = mark(log_path)
    points = [[left + 80, top + 200], [left + 200, top + 120], [left + 320, top + 200]]
    run([{"action": "draw", "points": points, "step_px": 10}])
    wait_for(lambda: events_after(log_path, start, "release"))
    assert len(events_after(log_path, start, "motion")) >= 20


def test_scroll_wheel_ticks(target):
    log_path, ready = target["log"], target["ready"]
    left, top, _w, _h = ready["canvas"]
    start = mark(log_path)
    run([{"action": "scroll", "x": left + 100, "y": top + 100, "ticks": -3}])
    wheel = wait_for(lambda: events_after(log_path, start, "wheel") or None)
    assert sum(e["delta"] for e in wheel) == -360


def test_batch_of_many_actions_is_fast(target):
    log_path, ready = target["log"], target["ready"]
    left, top, _w, _h = ready["canvas"]
    start = mark(log_path)
    actions = [{"action": "click", "x": left + 20 + i * 5, "y": top + 20} for i in range(30)]
    began = time.perf_counter()
    run(actions)
    elapsed = time.perf_counter() - began
    wait_for(lambda: len(events_after(log_path, start, "press")) >= 30)
    print(f"30 clicks in one batch: {elapsed * 1000:.0f} ms")
    assert elapsed < 3.0


def test_two_servers_do_not_interleave(target):
    """Two processes hammer the shared lock; every 5-click burst must stay contiguous."""
    log_path, ready = target["log"], target["ready"]
    left, top, _w, _h = ready["canvas"]
    code = (
        "import os\n"
        "os.environ['POINTHAND_OVERLAY']='0'\n"
        "from pointhand import native\n"
        "native.enable_dpi_awareness()\n"
        "from pointhand.server import run_actions\n"
        f"for _ in range(6): run_actions([{{'action':'click','x':{left + 300},'y':{top + 30}}}]*5)\n"
    )
    start = mark(log_path)
    other = subprocess.Popen([sys.executable, "-c", code])
    for _ in range(6):
        run([{"action": "click", "x": left + 20, "y": top + 300}] * 5)
    other.wait(timeout=60)
    clicks = events_after(log_path, start, "press")
    assert len(clicks) == 60
    runs, last, length = [], None, 0
    for click in clicks:
        side = click["x"] > left + 200
        if last is not None and side != last:
            runs.append(length)
            length = 0
        last, length = side, length + 1
    runs.append(length)
    assert all(r % 5 == 0 for r in runs), runs
