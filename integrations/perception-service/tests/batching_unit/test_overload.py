"""InputBatcher under overload: refuse past the backlog cap, drop stale and cancelled items (#612)."""

import asyncio
import threading

import pytest

from core.perception.base.batching import BatcherOverloaded, InputBatcher


class _SlowPredictor:
    """Blocks predict() until released; records every input it actually ran."""

    def __init__(self):
        self.ran: list[int] = []
        self.release = threading.Event()

    def is_ready(self) -> bool:
        return True

    def predict(self, inputs, **kwargs):
        self.release.wait(5)
        self.ran.extend(inputs)
        return [x * 10 for x in inputs]


def _batcher(pred, **kw) -> InputBatcher:
    return InputBatcher(pred, batch_size=1, batch_timeout=0.001, **kw)


async def _until(cond, timeout=2.0):
    end = asyncio.get_running_loop().time() + timeout
    while not cond():
        assert asyncio.get_running_loop().time() < end, "condition not reached"
        await asyncio.sleep(0.005)


@pytest.mark.asyncio
async def test_normal_flow_unchanged():
    pred = _SlowPredictor()
    pred.release.set()
    b = _batcher(pred)
    await b.start()
    futs = await b.submit([1])
    assert await futs[0] == 10
    await b.stop()


@pytest.mark.asyncio
async def test_refuses_past_backlog_cap():
    pred = _SlowPredictor()  # stays blocked: the first item occupies the worker
    b = _batcher(pred, max_pending=2, max_wait_s=60)
    await b.start()
    await b.submit([1])
    await _until(lambda: pred.ran == [] and b._backlog() == 0)  # item 1 is computing
    await b.submit([2])
    await b.submit([3])
    with pytest.raises(BatcherOverloaded) as exc:
        await b.submit([4])
    assert exc.value.retry_after_s == 2.0
    pred.release.set()
    await b.stop()


@pytest.mark.asyncio
async def test_item_older_than_max_wait_is_dropped_before_gpu():
    pred = _SlowPredictor()
    b = _batcher(pred, max_wait_s=0.05)
    await b.start()
    first = await b.submit([1])
    stale = await b.submit([2])
    await asyncio.sleep(0.1)  # item 2 waits past max_wait_s behind item 1
    pred.release.set()
    assert await first[0] == 10
    with pytest.raises(BatcherOverloaded):
        await stale[0]
    assert pred.ran == [1]
    await b.stop()


@pytest.mark.asyncio
async def test_cancelled_item_is_never_computed():
    pred = _SlowPredictor()
    b = _batcher(pred, max_wait_s=60)
    await b.start()
    first = await b.submit([1])
    gone = await b.submit([2])
    gone[0].cancel()
    pred.release.set()
    assert await first[0] == 10
    await asyncio.sleep(0.05)
    assert pred.ran == [1]
    await b.stop()


def test_defaults_come_from_settings():
    b = InputBatcher(_SlowPredictor(), batch_size=4)
    assert b._max_wait_s == 8.0
    assert b._max_pending == 32           # max(32, 4 * 8)
    assert InputBatcher(_SlowPredictor(), batch_size=16)._max_pending == 128
