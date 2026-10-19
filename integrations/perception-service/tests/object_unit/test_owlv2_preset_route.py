"""HTTP `preset` on the OWLv2 detect routes: forwarded alone, wins over classes, 400 when unknown."""

import base64

import cv2
import numpy as np
import pytest
from fastapi import FastAPI
from fastapi.testclient import TestClient

from core.models.object import ObjectDetection, ObjectDetectionItem
from dlserver.routes import object as object_routes


class _FakePerception:
    def __init__(self, presets: frozenset[str]):
        self._presets = presets
        self.calls: list[dict] = []

    def is_ready(self) -> bool:
        return True

    def check_capacity(self) -> None:
        pass

    def has_preset(self, name: str) -> bool:
        return name in self._presets

    async def predict_image(self, image, classes=None, preset=None):
        self.calls.append({"classes": classes, "preset": preset})
        return ObjectDetection(
            detections=[ObjectDetectionItem(class_name="a single flame", xywh=[1.0, 2.0, 3.0, 4.0], confidence=0.9)]
        )


def _image_b64() -> str:
    _, buf = cv2.imencode(".jpg", np.zeros((32, 32, 3), dtype=np.uint8))
    return base64.b64encode(buf.tobytes()).decode()


@pytest.fixture
def client():
    app = FastAPI()
    app.include_router(object_routes.http_router, prefix="/api/dl")
    return TestClient(app)


@pytest.fixture
def owlv2(monkeypatch):
    fake = _FakePerception(frozenset({"fire_hazard_v1"}))
    monkeypatch.setattr(object_routes, "get_object_model", lambda name: fake if name == "owlv2" else None)
    return fake


ROUTES = ["/api/dl/owlv2", "/api/dl/object-detect/owlv2"]


def _detections(path: str, body):
    # The compat route returns a flat list; /object-detect returns {"detections": [...]}.
    return body if path == "/api/dl/owlv2" else body["detections"]


@pytest.mark.parametrize("path", ROUTES)
def test_preset_known_returns_200_and_forwards_preset_only(client, owlv2, path):
    r = client.post(path, json={"image_b64": _image_b64(), "preset": "fire_hazard_v1"})
    assert r.status_code == 200
    assert _detections(path, r.json())[0]["class_name"] == "a single flame"
    assert owlv2.calls == [{"classes": None, "preset": "fire_hazard_v1"}]


@pytest.mark.parametrize("path", ROUTES)
def test_unknown_preset_is_400(client, owlv2, path):
    r = client.post(path, json={"image_b64": _image_b64(), "preset": "nope"})
    assert r.status_code == 400
    assert "Unknown preset" in r.json()["detail"]
    assert owlv2.calls == []


@pytest.mark.parametrize("path", ROUTES)
def test_both_sent_preset_wins(client, owlv2, path):
    r = client.post(path, json={"image_b64": _image_b64(), "preset": "fire_hazard_v1", "classes": ["cat"]})
    assert r.status_code == 200
    assert owlv2.calls == [{"classes": None, "preset": "fire_hazard_v1"}]


@pytest.mark.parametrize("path", ROUTES)
def test_classes_only_unchanged(client, owlv2, path):
    r = client.post(path, json={"image_b64": _image_b64(), "classes": ["cat", "dog"]})
    assert r.status_code == 200
    assert owlv2.calls == [{"classes": ["cat", "dog"], "preset": None}]


@pytest.mark.parametrize("path", ["/api/dl/yoloworld", "/api/dl/object-detect/yoloworld"])
def test_yoloworld_rejects_preset(client, monkeypatch, path):
    fake = _FakePerception(frozenset())
    monkeypatch.setattr(object_routes, "get_object_model", lambda name: fake)
    r = client.post(path, json={"image_b64": _image_b64(), "preset": "fire_hazard_v1"})
    assert r.status_code == 400
    assert fake.calls == []


def test_preset_too_long_is_422(client, owlv2):
    r = client.post("/api/dl/owlv2", json={"image_b64": _image_b64(), "preset": "x" * 65})
    assert r.status_code == 422
