"""Refuse before the expensive part (#612 follow-up).

Fix 1: HTTP routes check the batcher's room before decoding the image/audio.
Fix 2: stream sessions rest a full frame interval after a refusal, and the action
session checks the action batcher before running the person detector.
"""

import base64

import cv2
import numpy as np
import pytest
from fastapi import FastAPI
from fastapi.testclient import TestClient

from core.perception.action.session import ActionPerceptionSession
from core.perception.base.batching import BatcherOverloaded, InputBatcher
from core.perception.pose.session import PosePerceptionSession

BUSY = BatcherOverloaded("backlog full (32)", 2.0)


# --- batcher ---------------------------------------------------------------------

class _P:
    def is_ready(self):
        return True

    def predict(self, x, **kw):
        return x


@pytest.mark.asyncio
async def test_check_capacity_raises_only_when_full():
    b = InputBatcher(_P(), batch_size=1, max_pending=0)
    await b.start()
    with pytest.raises(BatcherOverloaded):
        b.check_capacity()
    await b.stop()
    InputBatcher(_P(), batch_size=1, max_pending=4).check_capacity()  # room: no raise


# --- Fix 1: routes refuse before decoding ------------------------------------------

def _jpeg_b64():
    return base64.b64encode(cv2.imencode(".jpg", np.zeros((8, 8, 3), np.uint8))[1].tobytes()).decode()


class _Full:
    """A perception whose batcher is full; any predict call is a test failure."""

    def is_ready(self):
        return True

    def has_preset(self, name):
        return True

    def check_capacity(self):
        raise BUSY

    async def predict_image(self, *a, **kw):
        raise AssertionError("predict_image must not run when full")

    async def predict_face(self, *a, **kw):
        raise AssertionError("predict_face must not run when full")

    async def predict_audio(self, *a, **kw):
        raise AssertionError("predict_audio must not run when full")


def _client(router, prefix="/api/dl"):
    app = FastAPI()
    app.include_router(router, prefix=prefix)
    return TestClient(app)


@pytest.mark.parametrize("path", ["/api/dl/owlv2", "/api/dl/object-detect/owlv2"])
def test_object_route_refuses_before_decoding(monkeypatch, path):
    from dlserver.routes import object as routes

    decoded = []
    monkeypatch.setattr(routes, "get_object_model", lambda name: _Full())
    monkeypatch.setattr(routes, "decode_image", lambda b64: decoded.append(1))
    r = _client(routes.http_router).post(path, json={"image_b64": _jpeg_b64(), "preset": "fire_hazard_v1"})
    assert r.status_code == 503 and r.headers["Retry-After"] == "2"
    assert decoded == []


def test_fer_route_refuses_before_decoding(monkeypatch):
    from dlserver.routes import facial_emotion as routes

    decoded = []
    monkeypatch.setattr(routes, "get_emotion_model", lambda: _Full())
    monkeypatch.setattr(routes, "decode_image", lambda b64: decoded.append(1))
    r = _client(routes.http_router).post("/api/dl/emotion-recognize", json={"image_b64": _jpeg_b64(), "raw": True})
    assert r.status_code == 503 and decoded == []


def test_ser_route_refuses_before_decoding(monkeypatch):
    from dlserver.routes import audio_emotion as routes

    decoded = []
    monkeypatch.setattr(routes, "get_audio_emotion_model", lambda: _Full())
    monkeypatch.setattr(routes, "decode_b64_wav", lambda b64: decoded.append(1))
    r = _client(routes.router).post("/api/dl/ser/recognize", json={"audio_b64": "UklGRg=="})
    assert r.status_code == 503 and decoded == []


# --- Fix 2: streams rest after a refusal ---------------------------------------------

class _FakeBatcher:
    def __init__(self, predictor=None, full=False):
        self.predictor = predictor
        self.full = full
        self.submits = 0

    def is_ready(self):
        return True

    def check_capacity(self, n=1):
        if self.full:
            raise BUSY

    async def submit(self, inputs, **kw):
        self.submits += 1
        if self.full:
            raise BUSY
        raise AssertionError("not expected in these tests")


class _ActionRecognizer:
    default_class_mask = np.ones(3, dtype=bool)
    max_frames = 4


FRAME = np.zeros((32, 32, 3), np.uint8)


@pytest.mark.asyncio
async def test_action_refused_skips_person_detector_and_rests():
    person = _FakeBatcher()
    action = _FakeBatcher(_ActionRecognizer(), full=True)
    s = ActionPerceptionSession(action, person)
    await s.start()
    with pytest.raises(BatcherOverloaded):
        await s.update(FRAME)
    assert person.submits == 0  # no GPU person detection for a frame that cannot run
    assert await s.update(FRAME) is None  # within the frame interval: rest, no retry
    assert person.submits == 0 and action.submits == 0


@pytest.mark.asyncio
async def test_pose_refused_rests_a_frame_interval():
    pose2d = _FakeBatcher(object(), full=True)
    s = PosePerceptionSession(pose2d)
    await s.start()
    with pytest.raises(BatcherOverloaded):
        await s.update(FRAME)
    assert await s.update(FRAME) is None  # within the frame interval: no second submit
    assert pose2d.submits == 1
