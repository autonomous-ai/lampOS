"""Overload surfaces as 503 + Retry-After on HTTP and as {"error": "busy"} on WS (#612)."""

import base64

import cv2
import numpy as np
import pytest
from fastapi import FastAPI
from fastapi.testclient import TestClient

from core.perception.base.batching import BatcherOverloaded
from dlserver.routes import object as object_routes


class _BusyPerception:
    def is_ready(self) -> bool:
        return True

    def has_preset(self, name: str) -> bool:
        return True

    def check_capacity(self) -> None:
        pass  # room at check time; the queue fills before submit

    async def predict_image(self, image, classes=None, preset=None):
        raise BatcherOverloaded("backlog full (32)", 2.0)


class _BusySession:
    async def update(self, frame):
        raise BatcherOverloaded("backlog full (32)", 2.0)

    async def stop(self):
        pass


class _BusyStreamPerception(_BusyPerception):
    async def create_session(self):
        return _BusySession()


def _image_b64() -> str:
    _, buf = cv2.imencode(".jpg", np.zeros((32, 32, 3), dtype=np.uint8))
    return base64.b64encode(buf.tobytes()).decode()


@pytest.fixture
def client(monkeypatch):
    monkeypatch.setattr(object_routes, "get_object_model", lambda name: _BusyPerception())
    app = FastAPI()
    app.include_router(object_routes.http_router, prefix="/api/dl")
    return TestClient(app)


@pytest.mark.parametrize("path", ["/api/dl/owlv2", "/api/dl/object-detect/owlv2"])
def test_http_overload_is_503_with_retry_after(client, path):
    r = client.post(path, json={"image_b64": _image_b64(), "preset": "fire_hazard_v1"})
    assert r.status_code == 503
    assert r.headers["Retry-After"] == "2"
    assert r.json()["detail"].startswith("Server busy")


def test_overloaded_http_rounds_retry_after_up():
    from dlserver.utils.common import overloaded_http

    exc = overloaded_http(BatcherOverloaded("x", 0.2))
    assert exc.status_code == 503 and exc.headers == {"Retry-After": "1"}


def test_ws_busy_keeps_socket_open(monkeypatch):
    async def _ok(websocket):
        return True

    monkeypatch.setattr(object_routes, "verify_ws_api_key", _ok)
    monkeypatch.setattr(object_routes, "get_object_model", lambda name: _BusyStreamPerception())
    app = FastAPI()
    app.include_router(object_routes.ws_router, prefix="/api/dl")
    frame = {"type": "frame", "task": "object", "frame_b64": _image_b64()}
    with TestClient(app).websocket_connect("/api/dl/object-detection/owlv2/ws") as ws:
        ws.send_json(frame)
        assert ws.receive_json() == {"error": "busy"}
        ws.send_json(frame)  # still open after the first busy reply
        assert ws.receive_json() == {"error": "busy"}
