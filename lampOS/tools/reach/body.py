"""The lamp's body as arithmetic, for the reach study (README.md): where its
centre of mass is (the body's anti-tip rule) and where each CAD part is (self-
collision and the table), for any servo counts. Offline; vectorised over poses.

Servo order everywhere here is the bus's: base yaw, shoulder, elbow, head roll,
head pitch (servo ids 1..5, `cog.rs`'s "by servo ID"). A move's order (pitch,
roll, yaw, shoulder, elbow) appears only where a move is read.
"""
from __future__ import annotations

import json
import os
import struct

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
LAMPOS = os.path.normpath(os.path.join(HERE, "..", ".."))
LEAN_JSON = os.path.join(LAMPOS, "deploy", "lamp-4ace", "lean-first-software.json")
MODEL_JSON = os.path.join(LAMPOS, "docs", "lamp.model.json")
COUNTS_A_TURN = 4096.0
JOINT_INDEX = {"base_yaw": 0, "shoulder": 1, "elbow": 2, "head_roll": 3, "head_pitch": 4}


# ---- rotations, batched: (..., 3, 3) ----

def rot_axis(axis, angle: np.ndarray) -> np.ndarray:
    """Rotation matrices about a unit `axis` by `angle` (radians, any shape)."""
    a = np.asarray(axis, float)
    a = a / np.linalg.norm(a)
    c, s = np.cos(angle)[..., None, None], np.sin(angle)[..., None, None]
    k = np.array([[0, -a[2], a[1]], [a[2], 0, -a[0]], [-a[1], a[0], 0]])
    return np.eye(3) * c + s * k + (1 - c) * np.outer(a, a)


def rot_quat(q) -> np.ndarray:
    """A quaternion [x, y, z, w] as a matrix."""
    x, y, z, w = np.asarray(q, float) / np.linalg.norm(q)
    return np.array([
        [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
        [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
        [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
    ])


# ---- the centre of mass: lamp-lean's Model::check_pose, as cog.rs builds it ----

class Lean:
    """`crates/lean/src/model.rs` `check_pose` on the body's own geometry
    (`cog.rs` BODY: deploy/lamp-4ace/lean-first-software.json, V1's estimated
    masses). URDF frame: z up, metres."""

    def __init__(self, path: str = LEAN_JSON):
        g = json.load(open(path))
        self.parts = []
        for link in g["links"]:
            r, p, y = link["rpy_rad"]
            fixed = rot_axis([0, 0, 1], np.array(y)) @ rot_axis([0, 1, 0], np.array(p)) @ rot_axis([1, 0, 0], np.array(r))
            self.parts.append(dict(index=JOINT_INDEX[link["joint"]], origin=np.array(link["origin_m"]), fixed=fixed,
                                   axis=np.array(link["axis"]), zero=link["angle_zero_rad"], mass=link["mass_kg"],
                                   com=np.array(link["com_m"])))
            if link["joint"] == "base_yaw":
                self.yaw_zero = link["angle_zero_rad"]
        self.base_moment = np.array(g["base"]["com_m"]) * g["base"]["mass_kg"]
        self.total = g["base"]["mass_kg"] + sum(p["mass"] for p in self.parts)

    def centre(self, radians: np.ndarray) -> np.ndarray:
        """(N, 5) servo angles (radians, lamp-lean's `from_encoder_counts`) to
        (N, 3) centres of mass."""
        n = len(radians)
        rot = np.broadcast_to(np.eye(3), (n, 3, 3)).copy()
        origin = np.zeros((n, 3))
        moment = np.broadcast_to(self.base_moment, (n, 3)).copy()
        for p in self.parts:
            origin = origin + rot @ p["origin"]
            rot = rot @ p["fixed"] @ rot_axis(p["axis"], radians[:, p["index"]] - p["zero"])
            moment += (origin + rot @ p["com"]) * p["mass"]
        return moment / self.total

    @staticmethod
    def radians(counts: np.ndarray) -> np.ndarray:
        return (np.asarray(counts, float) - 2048.0) * 2 * np.pi / COUNTS_A_TURN

    def off_axis_mm(self, counts: np.ndarray) -> np.ndarray:
        """`cog::off_axis_mm`: the centre of mass's distance from the base's axis."""
        c = self.centre(self.radians(counts))
        return np.hypot(c[:, 0], c[:, 1]) * 1000

    def lean_back_mm(self, counts: np.ndarray) -> np.ndarray:
        """`cog::lean_back_mm`: how far behind the axis, along the arm's heading."""
        r = self.radians(counts).copy()
        r[:, 0] = self.yaw_zero
        return -self.centre(r)[:, 0] * 1000


# ---- the CAD: robots/lamp/hardware/cad/glb/lamp.glb, skinned to its five bones ----

BONES = ["base_yaw", "base_pitch", "elbow_pitch", "wrist_pitch", "wrist_roll"]
# lamp.model.json's names for the bones, and each bone's servo (bus order index).
SERVO_OF = {"base_yaw": 0, "base_pitch": 1, "elbow_pitch": 2, "wrist_roll": 3, "wrist_pitch": 4}
# The joints turn the way their counts grow, but the head's pitch (up is fewer):
# crates/sim/src/pose.rs SIGN, in bus order.
COUNT_SIGN = np.array([1.0, 1.0, 1.0, 1.0, -1.0])
# lamp.model.json's head pitch used to turn the CAD the wrong way (its sign was "cad, until
# measured"); lamp-52e6's folded contact at 2,010 counts (docs/body.md, 2026-10-14) showed it
# and the model's sign is -1 now, so nothing is fixed here. Kept as a hook for a lamp's own sign.
SIGN_FIX: dict[str, float] = {}


def trs(node: dict) -> np.ndarray:
    m = np.eye(4)
    if "matrix" in node:
        return np.array(node["matrix"], float).reshape(4, 4).T
    r = rot_quat(node.get("rotation", [0, 0, 0, 1]))
    s = np.diag(node.get("scale", [1, 1, 1]))
    m[:3, :3] = r @ s
    m[:3, 3] = node.get("translation", [0, 0, 0])
    return m


class Cad:
    """The CAD's parts as point clouds, each in its bone's own frame, posed by
    lamp.model.json's chain (the same file the twin and the simulator draw
    with). glTF frame: y up, metres."""

    def __init__(self, glb: str, voxel_m: float = 0.006):
        data = open(glb, "rb").read()
        n = struct.unpack("<I", data[12:16])[0]
        self.gltf = g = json.loads(data[20:20 + n])
        at = 20 + n
        bin_len = struct.unpack("<I", data[at:at + 4])[0]
        self.bin = data[at + 8:at + 8 + bin_len]
        self.nodes = g["nodes"]
        self.by_name = {nd.get("name"): i for i, nd in enumerate(self.nodes)}
        self.parent = {c: i for i, nd in enumerate(self.nodes) for c in nd.get("children", [])}
        model = json.load(open(MODEL_JSON))
        self.joints = {j["bone"]: j for j in model["joints"]}
        # Bone world matrices at rest (the CAD's pose: the stood lamp).
        self.rest_world = {name: self.world(self.by_name[name]) for name in BONES}
        skin = g["skins"][0]
        ibm = self.accessor(skin["inverseBindMatrices"]).reshape(-1, 4, 4).transpose(0, 2, 1)
        bone_of = [self.nodes[i]["name"] for i in skin["joints"]]
        # Each vertex goes with the bone it is skinned to, whatever its mesh is named:
        # the CAD's "4_wrist_roll" mesh is bound to the wrist_pitch bone and
        # "5_wrist_pitch" (the shade) to wrist_roll. Every part's vertices are bound
        # wholly (weight 1) to one bone, but the cable ("6_wire"), which bends across
        # all five and is left out of collision (README.md).
        found: dict[str, list[np.ndarray]] = {name: [] for name in BONES}
        base = []
        for node in self.nodes:
            if "mesh" not in node or node.get("name") == "6_wire":
                continue
            for prim in g["meshes"][node["mesh"]]["primitives"]:
                a = prim["attributes"]
                p = self.accessor(a["POSITION"])
                if "JOINTS_0" not in a:
                    # Not skinned: the fixed base, placed by its node.
                    w = self.world(self.by_name[node["name"]])
                    base.append(p @ w[:3, :3].T + w[:3, 3])
                    continue
                j = self.accessor(a["JOINTS_0"])[:, 0].astype(int)
                for idx in np.unique(j):
                    m = ibm[idx]
                    found[bone_of[idx]].append(p[j == idx] @ m[:3, :3].T + m[:3, 3])
        self.points = {"base": self.voxel(np.concatenate(base), voxel_m)}
        for name, chunks in found.items():
            self.points[name] = self.voxel(np.concatenate(chunks), voxel_m)

    def accessor(self, i: int) -> np.ndarray:
        a = self.gltf["accessors"][i]
        v = self.gltf["bufferViews"][a["bufferView"]]
        comp = {5126: np.float32, 5123: np.uint16, 5121: np.uint8, 5125: np.uint32}[a["componentType"]]
        width = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}[a["type"]]
        start = v.get("byteOffset", 0) + a.get("byteOffset", 0)
        count = a["count"] * width
        stride = v.get("byteStride")
        item = np.dtype(comp).itemsize * width
        if stride and stride != item:
            raw = np.frombuffer(self.bin, np.uint8, a["count"] * stride, start).reshape(a["count"], stride)[:, :item]
            return np.frombuffer(raw.tobytes(), comp).reshape(a["count"], width).astype(float)
        return np.frombuffer(self.bin, comp, count, start).reshape(a["count"], width).astype(float)

    def world(self, i: int) -> np.ndarray:
        m = trs(self.nodes[i])
        while i in self.parent:
            i = self.parent[i]
            m = trs(self.nodes[i]) @ m
        return m

    @staticmethod
    def voxel(points: np.ndarray, size: float) -> np.ndarray:
        """One point per occupied voxel (its centroid): the part's shape at `size`."""
        keys = np.floor(points / size).astype(np.int64)
        _, inv = np.unique(keys, axis=0, return_inverse=True)
        out = np.zeros((inv.max() + 1, 3))
        np.add.at(out, inv.ravel(), points)
        return out / np.bincount(inv.ravel())[:, None]

    def bone_worlds(self, deg: np.ndarray) -> dict[str, np.ndarray]:
        """(N, 5) joint degrees, lamp.model.json's convention (bus order: degrees
        from each joint's home as body.state counts them), to each bone's (N, 4, 4)
        world matrix. Each bone turns about its axis, after its rest rotation, by
        sign * (deg - zero_deg): the twin's and pose.rs's rule, with SIGN_FIX."""
        n = len(deg)
        armature = self.world(self.parent[self.by_name["base_yaw"]])
        out, parent_m = {}, np.broadcast_to(armature, (n, 4, 4)).copy()
        for name in BONES:
            j = self.joints[name]
            local = np.broadcast_to(trs(self.nodes[self.by_name[name]]), (n, 4, 4)).copy()
            sign = j["sign"] * SIGN_FIX.get(name, 1.0)
            turn = rot_axis(j["axis"], np.radians(sign * (deg[:, SERVO_OF[name]] - j["zero_deg"])))
            local[:, :3, :3] = local[:, :3, :3] @ turn
            parent_m = parent_m @ local
            out[name] = parent_m
        return out

    def posed(self, deg: np.ndarray) -> dict[str, np.ndarray]:
        """Each part's points in the world, (N, K, 3), for (N, 5) joint degrees."""
        worlds = self.bone_worlds(deg)
        out = {"base": np.broadcast_to(self.points["base"], (len(deg),) + self.points["base"].shape)}
        for name in BONES:
            m = worlds[name]
            out[name] = np.einsum("nij,kj->nki", m[:, :3, :3], self.points[name]) + m[:, None, :3, 3]
        return out


def deg_from_counts(counts: np.ndarray, home: np.ndarray) -> np.ndarray:
    """Bus-order counts to body.state degrees from each joint's home
    (control/src/body_info.rs `from_home`): the arm's home is its folded pose."""
    return COUNT_SIGN * (np.asarray(counts, float) - home) * 360.0 / COUNTS_A_TURN
