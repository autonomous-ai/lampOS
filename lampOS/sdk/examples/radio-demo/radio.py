"""Lamp radio drama demo, through the lamp SDK.

Episodes of Autonomous Radio Drama play on the lamp's own speaker. The lamp
asks the backend with its own key (the key never leaves the lamp: the lamp
answers only the catalog and an episode's short-lived audio link); the Mac
decodes the episode with ffmpeg and streams it to the lamp's speaker
(`speaker_write`, the demo lamp-control). While someone talks to the lamp the
story pauses, and it picks up again a couple of seconds after the talk ends.
The ring glows in the story's colour, and the lamp names each episode in its
owner's language.

    python radio.py list
    python radio.py play --random [--genre horror]
    python radio.py play --story "vampire" [--episode 2]
"""

from __future__ import annotations

import argparse
import asyncio
import base64
import contextlib
import json
import os
import shlex
import random
import subprocess
import time
from pathlib import Path

from lampos import Lamp

LAMP_IP = "172.168.20.193"
LAMP_SSH = str(Path.home() / "Dropbox/autonomous/interns/lamp-v2-trial/l.sh")
RATE = 16_000
# The story plays this much under full scale, so the lamp still hears its name
# over it: at full scale its name finder scored 0.37, under its bar.
STORY_GAIN = 0.45
BLOCK = 320 * 2  # bytes of 20 ms
PROGRESS = Path(__file__).with_name("radio-progress.json")

# Asked on the lamp, with only the standard library: the key stays there.
ON_LAMP = r'''
import json, sys, urllib.request
base = "https://campaign-api.autonomous.ai/api/v1/ai/v1/radio-drama"
key = open("/etc/lampOS/voice/credential").read().strip()
path, method = sys.argv[1], sys.argv[2]
req = urllib.request.Request(base + path, method=method, data=b"{}" if method == "POST" else None,
    headers={"Authorization": "Bearer " + key, "User-Agent": "AutonomousLamp/1.0 (radio demo)",
             "Accept": "application/json", "Content-Type": "application/json"})
try:
    with urllib.request.urlopen(req, timeout=15) as r:
        print(json.dumps({"status": r.status, "body": json.load(r)}))
except urllib.error.HTTPError as e:
    print(json.dumps({"status": e.code, "body": {}}))
'''

LINES = {
    "en": {
        "playing": "Here is {title}, episode {episode}.",
        "resuming": "Picking up {title}, episode {episode}.",
        "locked": "That episode needs a Radio Drama subscription.",
        "finished": "That's the end of {title}.",
        "paused": "The story is stopped.",
        "off": "Radio off. Say continue next time.",
    },
    "vi": {
        "playing": "Mời bạn nghe {title}, tập {episode}.",
        "resuming": "Nghe tiếp {title}, tập {episode} nhé.",
        "locked": "Tập này cần gói Radio Drama mới nghe được.",
        "finished": "Hết truyện {title} rồi.",
        "paused": "Đã dừng truyện.",
        "off": "Đã tắt radio. Lần sau mình nghe tiếp nhé.",
    },
}

# The ring's mood by genre words: a base colour, a brightness (0-100, kept
# low so it never dazzles) and whether it flickers like a dying candle.
MOODS = [
    (("horror", "gothic", "kinh dị", "revenge", "venganza", "poison", "dark"), ((150, 8, 0), 15, True)),
    (("romance", "romántic", "romantic", "love"), ((255, 60, 120), 40, False)),
    (("fantasy", "dragon", "werewolf", "lobo", "alpha"), ((0, 170, 160), 40, False)),
]
CALM = ((255, 150, 60), 40, False)

# The body while it listens: small clips that fit the story, now and then,
# like someone caught up in it, never a constant sway (CLAUDE.md rule 8).
GESTURES = [
    (("horror", "gothic", "kinh dị", "revenge", "venganza", "poison", "dark"), ["surprised", "curious", "confused"]),
    (("romance", "romántic", "romantic", "love"), ["shy", "caring", "happy"]),
    (("fantasy", "dragon", "werewolf", "lobo", "alpha"), ["curious", "excited", "happy"]),
]
CALM_GESTURES = ["acknowledge", "curious", "caring"]
GESTURE_EVERY = (25.0, 45.0)  # seconds between two clips


# The lamp's SSH password comes from SSHPASS (sshpass -e), never from the code.
SSH = ["sshpass", "-e", "ssh", "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null",
       "-o", "LogLevel=ERROR", "-o", "ConnectTimeout=8", "-o", "ControlMaster=auto",
       "-o", f"ControlPath={Path.home()}/.ssh/cm-lamp-radio", "-o", "ControlPersist=1800", f"orangepi@{LAMP_IP}"]
SERIES: dict = {}  # each story's episodes, asked once


def backend(path: str, method: str = "GET") -> dict:
    """The lamp asks the backend with its own key, over one kept-open SSH connection."""
    env = dict(os.environ, PATH=f"{Path.home()}/.local/bin:/opt/homebrew/bin:" + os.environ.get("PATH", ""))
    for attempt in range(4):
        code = base64.b64encode(ON_LAMP.encode()).decode()
        remote = (f"echo {shlex.quote(os.environ.get('SSHPASS', ''))} | sudo -S -p '' python3 -c \"$(echo {code} | base64 -d)\" "
                  f"{json.dumps(path)} {method}")
        done = subprocess.run(SSH + [remote], capture_output=True, text=True, timeout=40, env=env)
        lines = [l for l in done.stdout.splitlines() if l.startswith("{")]
        if lines:
            return json.loads(lines[-1])
        time.sleep(1.0 * (attempt + 1))
    raise RuntimeError(f"the lamp did not answer: {done.stderr.strip()[-200:]}")


def series_detail(slug: str) -> dict:
    if slug not in SERIES:
        SERIES[slug] = backend(f"/series/{slug}")["body"]
    return SERIES[slug]


def mood_of(series: dict) -> tuple:
    words = f"{series.get('genre', '')} {' '.join(series.get('tropes') or [])} {series.get('title', '')}".lower()
    return next((mood for keys, mood in MOODS if any(key in words for key in keys)), CALM)


async def glow(lamp: Lamp, series: dict):
    rgb, brightness, flicker = mood_of(series)
    if flicker:
        return  # the flicker task keeps the ring
    with contextlib.suppress(Exception):
        await lamp.light_rgb(rgb, for_ms=3_000, brightness=brightness)


def gestures_of(series: dict) -> list[str]:
    words = f"{series.get('genre', '')} {' '.join(series.get('tropes') or [])} {series.get('title', '')}".lower()
    return next((clips for keys, clips in GESTURES if any(key in words for key in keys)), CALM_GESTURES)


async def move_along(lamp: Lamp, series: dict, stop: asyncio.Event):
    """Leans in as the story starts, then a small clip now and then."""
    clips = gestures_of(series)
    with contextlib.suppress(Exception):
        await lamp.gesture("curious")
    last = None
    while not stop.is_set():
        with contextlib.suppress(asyncio.TimeoutError):
            await asyncio.wait_for(stop.wait(), random.uniform(*GESTURE_EVERY))
        if stop.is_set():
            return
        clip = random.choice([c for c in clips if c != last] or clips)
        last = clip
        with contextlib.suppress(Exception):
            await lamp.gesture(clip)


async def flicker(lamp: Lamp, series: dict, stop: asyncio.Event):
    """A dim candle: each pixel of the ring drifts on its own, a few times a second."""
    rgb, brightness, flickers = mood_of(series)
    if not flickers:
        return
    pixels = lamp.ring_pixels or 32
    level = [0.7] * pixels
    while not stop.is_set():
        level = [min(1.0, max(0.35, l + random.uniform(-0.25, 0.25))) for l in level]
        ring = [tuple(int(c * l) for c in rgb) for l in level]
        with contextlib.suppress(Exception):
            await lamp.ring(ring, for_ms=2_000, brightness=brightness)
        with contextlib.suppress(asyncio.TimeoutError):
            await asyncio.wait_for(stop.wait(), random.uniform(0.25, 0.45))


def say_line(lamp: Lamp, key: str, **fields) -> str:
    language = (lamp.info.get("language") or "en").split("-")[0].lower()
    return LINES.get(language, LINES["en"])[key].format(**fields)


def progress() -> dict:
    with contextlib.suppress(Exception):
        return json.loads(PROGRESS.read_text())
    return {}


def save_progress(slug: str, episode: int, offset: float):
    data = progress()
    data[slug] = {"episode": episode, "offset_s": round(offset, 1), "at": time.time()}
    data["_last"] = slug
    PROGRESS.write_text(json.dumps(data, indent=1))


async def say_and_wait(lamp: Lamp, text: str, timeout: float = 15):
    done = asyncio.get_running_loop().create_future()

    def finished(event):
        if not done.done():
            done.set_result(event)

    subscription = lamp.on("done", finished)
    await subscription
    await lamp.say(text)
    with contextlib.suppress(asyncio.TimeoutError):
        await asyncio.wait_for(done, timeout)
    lamp.off("done", finished)


PLACE: dict = {}
LOADING = asyncio.Event()  # set once the story is about to sound


async def sweep(lamp: Lamp):
    """A soft white dot running round the ring while the story loads."""
    pixels = lamp.ring_pixels or 32
    at = 0
    while not LOADING.is_set():
        ring = [(0, 0, 0)] * pixels
        for tail, level in enumerate((255, 120, 50, 20)):
            ring[(at - tail) % pixels] = (level, level, level)
        with contextlib.suppress(Exception):
            await lamp.ring(ring, for_ms=1_000, brightness=25)
        at = (at + 2) % pixels
        await asyncio.sleep(0.12)  # where the story is now, saved when it is stopped
NAME_CALLED = asyncio.Event()
TAPPED = asyncio.Event()  # set by a tap on the lamp's head
RESUME_WITHIN = 120.0  # seconds a tap may bring the story back  # set when someone calls the lamp by name


async def talk_over(lamp: Lamp, most: float = 25.0):
    """Waits for the lamp's talk to end (2 s quiet), at most `most` seconds."""
    until = time.monotonic() + most
    quiet_since = None
    while time.monotonic() < until:
        if await someone_talking(lamp):
            quiet_since = None
        else:
            quiet_since = quiet_since or time.monotonic()
            if time.monotonic() - quiet_since >= 2.0:
                return
        await asyncio.sleep(0.5)


async def someone_talking(lamp: Lamp) -> bool:
    with contextlib.suppress(Exception):
        state = (await lamp.get("lamp")).get("lamp") or {}
        return bool(state.get("talking"))
    return False


async def play_episode(lamp: Lamp, series: dict, episode: dict, offset: float, grant: dict) -> tuple[str, float]:
    """Plays one episode from `offset`, its link asked for beforehand; how it ended and where."""
    if grant["status"] != 200 or not grant["body"].get("url"):
        return ("locked" if grant["status"] in (402, 403, 410) else f"http_{grant['status']}", offset)
    url = grant["body"]["url"]
    await glow(lamp, series)
    stop_flicker = asyncio.Event()
    flickering = asyncio.create_task(flicker(lamp, series, stop_flicker))
    moving = asyncio.create_task(move_along(lamp, series, stop_flicker))
    decoder = await asyncio.create_subprocess_exec(
        "ffmpeg", "-hide_banner", "-loglevel", "error", "-nostdin", "-user_agent", "AutonomousLamp/1.0 (radio demo)",
        "-ss", f"{offset:.1f}", "-i", url, "-af", f"volume={STORY_GAIN}",
        "-f", "s16le", "-ac", "1", "-ar", str(RATE), "-",
        stdout=asyncio.subprocess.PIPE)
    played = 0.0
    NAME_CALLED.clear()  # only a call while the story plays stops it
    TAPPED.clear()
    started = time.monotonic()
    next_check = 0.0
    try:
        while True:
            pcm = await decoder.stdout.readexactly(BLOCK) if True else b""
            # The lamp's name stops the sound at once (within 20 ms), so the
            # words after it reach the lamp without the story under them.
            if NAME_CALLED.is_set():
                NAME_CALLED.clear()
                return ("talk", offset + played)
            if TAPPED.is_set():
                TAPPED.clear()
                return ("tap", offset + played)
            await lamp.speaker_write(pcm)
            played += 0.02
            # Real time, a little ahead: the lamp keeps about 120 ms.
            ahead = played - (time.monotonic() - started)
            if ahead > 0.1:
                await asyncio.sleep(ahead - 0.1)
            if played >= next_check:
                next_check = played + 0.5
                PLACE.update(slug=series["slug"], episode=episode["number"], offset=offset + played)
                if TAPPED.is_set():
                    TAPPED.clear()
                    return ("tap", offset + played)
                if NAME_CALLED.is_set():
                    NAME_CALLED.clear()
                    return ("talk", offset + played)
                # A short glow, renewed: let go within 3 s of a pause, so the
                # skills' signs show on the ring.
                if int(played) % 2 == 0 and played - int(played) < 0.5:
                    await glow(lamp, series)
                if int(played) % 30 == 0 and played - int(played) < 0.5:
                    save_progress(series["slug"], episode["number"], offset + played)
    except asyncio.IncompleteReadError:
        return ("finished", offset + played)
    finally:
        stop_flicker.set()
        with contextlib.suppress(Exception):
            await flickering
        with contextlib.suppress(Exception):
            await moving
        with contextlib.suppress(ProcessLookupError):
            decoder.kill()


async def radio(lamp: Lamp, series: dict, number: int, offset: float, episodes_left: int = 5):
    detail = await asyncio.to_thread(series_detail, series["slug"])
    episodes = sorted(detail.get("episodes") or [], key=lambda e: e["number"])
    resumed = offset > 0
    while episodes_left > 0:
        episode = next((e for e in episodes if e["number"] == number), None)
        if episode is None:
            await say_and_wait(lamp, say_line(lamp, "finished", title=series["title"]))
            return
        print(f"{series['title']} — episode {number} ({episode.get('title')}) from {offset:.0f}s")
        # The link is asked for while the lamp names the episode.
        # No line of its own: the lamp has just said it is starting, so the
        # story sounds as soon as its link is in (speed first, the owner's ask).
        grant = await asyncio.to_thread(backend, f"/episodes/{episode['id']}/play", "POST")
        LOADING.set()
        how, offset = await play_episode(lamp, series, episode, offset, grant)
        save_progress(series["slug"], number, offset)
        if how == "talk":
            # The lamp was called: the story waits while it listens and answers,
            # then goes on a little before where it stopped. "Stop the radio
            # drama" (the stop skill's sign) cancels this run outright.
            print(f"paused at {offset:.0f}s: the lamp was called by name")
            await asyncio.sleep(3.0)
            await talk_over(lamp, 20.0)
            offset = max(0.0, offset - 2.0)
            resumed = True
            continue
        if how == "tap":
            # A tap on the head stops it; `continue` (by voice too) picks it up.
            print(f"stopped at {offset:.0f}s by a tap on the head")
            await say_and_wait(lamp, say_line(lamp, "paused"))
            return
        if how == "locked":
            await say_and_wait(lamp, say_line(lamp, "locked"))
            return
        if how != "finished":
            print("stopped:", how)
            return
        number, offset, resumed = number + 1, 0.0, False
        episodes_left -= 1
        save_progress(series["slug"], number, 0.0)


# The skills' signs on the ring (skills/radio-*): a named colour at level 1.
SIGNS = {
    "purple": ("play", "horror"), "pink": ("play", "romance"), "cyan": ("play", "fantasy"),
    "green": ("play", None), "blue": ("stop", None), "yellow": ("continue", None),
}
GENRE_WORDS = {"horror": "gothic", "romance": "romance", "fantasy": "fantasy"}


def pick(catalog: list, genre: str | None) -> dict:
    choices = [s for s in catalog if s.get("language") == "en"] or catalog
    if genre:
        word = GENRE_WORDS.get(genre, genre)
        choices = [s for s in choices if word in f"{s.get('genre', '')} {' '.join(s.get('tropes') or [])}".lower()] or choices
    return random.choice(choices)


async def serve(host: str):
    """Waits for the skills' signs and plays, stops or continues the story."""
    catalog = backend("/series")["body"].get("series") or []
    for item in catalog:
        if item.get("language") == "en":
            with contextlib.suppress(Exception):
                series_detail(item["slug"])
    async with await Lamp.connect(host=host) as lamp:
        await lamp.on("name.heard", lambda event: NAME_CALLED.set())
        await lamp.on("touch", lambda event: event.get("kind") == "tap" and TAPPED.set())
        print("radio: waiting for a story to be asked for")
        playing: asyncio.Task | None = None
        last_sign, last_at = None, 0.0

        async def stop_playing():
            nonlocal playing
            if playing and not playing.done():
                playing.cancel()
                with contextlib.suppress(BaseException):
                    await playing
                if PLACE:
                    save_progress(PLACE["slug"], PLACE["episode"], PLACE["offset"])
            playing = None

        while True:
            await asyncio.sleep(0.3)
            with contextlib.suppress(Exception):
                shows = ((await lamp.get("light")).get("light") or {}).get("shows") or {}
                sign = SIGNS.get(shows.get("colour")) if shows.get("level") == 1 and shows.get("on") else None
                # Only a sign that has just appeared: one held for seconds is one ask.
                appeared = sign is not None and sign != last_sign
                if sign != last_sign and (sign is not None or time.monotonic() - last_at > 1.0):
                    last_sign = sign
                if sign is not None:
                    last_at = time.monotonic()
                if not appeared:
                    continue
                what, genre = sign
                print(f"sign: {what} {genre or ''}")
                await stop_playing()
                if what == "stop":
                    continue
                if what == "continue":
                    saved = progress()
                    slug = saved.get("_last")
                    series = next((x for x in catalog if x["slug"] == slug), None)
                    if not series:
                        continue
                    number, offset = saved[slug]["episode"], max(0.0, saved[slug]["offset_s"] - 3)
                else:
                    series, number, offset = pick(catalog, genre), 1, 0.0
                LOADING.clear()
                loading = asyncio.create_task(sweep(lamp))
                playing = asyncio.create_task(radio(lamp, series, number, offset))
                playing.add_done_callback(lambda _: LOADING.set())


async def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("list")
    play = sub.add_parser("play")
    play.add_argument("--random", action="store_true")
    play.add_argument("--genre", default=None)
    play.add_argument("--story", default=None)
    play.add_argument("--episode", type=int, default=None)
    play.add_argument("--host", default="127.0.0.1")
    sub.add_parser("continue").add_argument("--host", default="127.0.0.1")
    sub.add_parser("serve").add_argument("--host", default="127.0.0.1")
    args = parser.parse_args()
    if args.command == "serve":
        await serve(args.host)
        return

    catalog = backend("/series")["body"].get("series") or []
    if args.command == "list":
        for s in catalog:
            print(f"- {s['title']}  [{s.get('language')}, {s.get('genre')}, {s.get('episode_count')} eps]  {s['slug']}")
        return
    saved = progress()
    if args.command == "continue":
        slug = saved.get("_last")
        series = next((s for s in catalog if s["slug"] == slug), None)
        if not series:
            print("nothing to continue")
            return
        number, offset = saved[slug]["episode"], max(0.0, saved[slug]["offset_s"] - 3)
    else:
        choices = [s for s in catalog if s.get("language") == "en"] or catalog
        if args.genre:
            choices = [s for s in choices if args.genre.lower() in
                       f"{s.get('genre', '')} {' '.join(s.get('tropes') or [])} {s['title']}".lower()] or choices
        if args.story:
            choices = [s for s in catalog if args.story.lower() in s["title"].lower()] or choices
        series = random.choice(choices) if (args.random or not args.story) else choices[0]
        number = args.episode or 1
        offset = 0.0
    async with await Lamp.connect(host=args.host) as lamp:
        await lamp.on("name.heard", lambda event: NAME_CALLED.set())
        await lamp.on("touch", lambda event: event.get("kind") == "tap" and TAPPED.set())
        await radio(lamp, series, number, offset)


if __name__ == "__main__":
    asyncio.run(main())
