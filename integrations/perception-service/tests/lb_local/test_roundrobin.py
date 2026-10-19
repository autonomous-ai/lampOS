"""Tests for the RoundRobin class."""

import threading

import pytest
from fastapi import HTTPException

from lbserver.utils import RoundRobin


class TestRoundRobin:
    def test_cycles_through_backends(self):
        rr = RoundRobin(["a", "b", "c"])
        assert rr.next() == "a"
        assert rr.next() == "b"
        assert rr.next() == "c"
        assert rr.next() == "a"

    def test_single_backend(self):
        rr = RoundRobin(["only"])
        for _ in range(5):
            assert rr.next() == "only"

    def test_empty_backends_raises_503(self):
        rr = RoundRobin([])
        with pytest.raises(HTTPException) as exc_info:
            rr.next()
        assert exc_info.value.status_code == 503

    def test_marked_down_backend_is_skipped_then_returns(self, monkeypatch):
        import lbserver.utils.roundrobin as rr_mod

        now = [100.0]
        monkeypatch.setattr(rr_mod.time, "monotonic", lambda: now[0])
        rr = RoundRobin(["a", "b"], down_for_s=10.0)
        rr.mark_down("a")
        assert [rr.next() for _ in range(3)] == ["b", "b", "b"]
        now[0] += 10.1
        assert sorted({rr.next() for _ in range(4)}) == ["a", "b"]

    def test_all_down_still_returns_a_backend(self):
        rr = RoundRobin(["a"])
        rr.mark_down("a")
        assert rr.next() == "a"

    def test_thread_safety(self):
        backends = ["a", "b", "c"]
        rr = RoundRobin(backends)
        results: list[str] = []
        lock = threading.Lock()

        def pick(n: int) -> None:
            for _ in range(n):
                val = rr.next()
                with lock:
                    results.append(val)

        threads = [threading.Thread(target=pick, args=(100,)) for _ in range(10)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert len(results) == 1000
        assert set(results) == set(backends)
