#!/usr/bin/env python3
"""Autonomous Radio Drama helper: pick a story, play an episode through HAL, remember progress.

Every command prints one JSON object for the agent to read. Standard library only.
Stories and audio come from the Autonomous backend (`<backend>/radio-drama/...`) with this
device's key; the backend decides which episodes are open (subscription, or the free first ones).

  radio_drama.py genres [--language L]
  radio_drama.py list [--genre G] [--query Q] [--language L]
  radio_drama.py play --random [--genre G] [--include-heard] [--language L]
  radio_drama.py play --story "title words" [--episode N] [--restart]
  radio_drama.py continue            # the story last listened to, where it stopped
  radio_drama.py next | previous     # neighbour episode of the current story
  radio_drama.py stop
  radio_drama.py status
"""

import argparse
import json
import os
import random
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

# Device config holding the Autonomous backend URL and device key (read here, never printed).
DEVICE_CONFIG = Path(os.environ.get("RADIO_DRAMA_DEVICE_CONFIG", "/root/config/config.json"))
HAL_BASE = os.environ.get("RADIO_DRAMA_HAL_BASE", "http://127.0.0.1:5001")
STATE_DIR = Path(os.environ.get("RADIO_DRAMA_STATE_DIR", "/root/local/radio-drama"))
CACHE_DIR = Path(os.environ.get("RADIO_DRAMA_CACHE_DIR", "/tmp/radio-drama"))

CATALOG_TTL_S = 600
USER_AGENT = "AutonomousIntern/1.0 (radio-drama skill)"
# Rewind a little on resume so the listener catches the last line again.
RESUME_REWIND_S = 3.0
# How many episodes play back to back before the story pauses on its own.
DEFAULT_AUTOPLAY = 5
# Episodes per story open without a subscription (backend policy; used when it does not say).
FREE_EPISODES = 5
WATCH_POLL_S = 2.0
# Playback starts once this much audio is on disk (~3 s at the catalog's ~150 kbit/s AAC);
# the rest keeps downloading behind it, far faster than real time.
STREAM_HEAD_BYTES = 64 * 1024
STREAM_START_TIMEOUT_S = 20
# How long the watcher waits for a stalled download to finish before resuming the episode.
STALL_WAIT_S = 120
# /tmp is RAM (tmpfs) on the device: keep only the newest episodes, ~6.5 MB each.
CACHE_MAX_BYTES = 60 * 1024 * 1024
SLUG = re.compile(r"^[a-z0-9][a-z0-9-]{0,127}$")


# ---------- small utilities ----------

def emit(obj):
    print(json.dumps(obj, ensure_ascii=False))
    return 0 if obj.get("ok", True) else 1


def norm(text):
    return re.sub(r"[^a-z0-9]+", " ", (text or "").lower().replace("'", "")).strip()


def genre_key(genre):
    """'gothic-romance' and 'Gothic Romance' are one genre."""
    return norm(genre.replace("-", " ").replace("_", " "))


def http_json(url, headers=None, timeout=15, method="GET", payload=None):
    data = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(url, data=data, method=method, headers={
        "User-Agent": USER_AGENT, "Accept": "application/json", "Content-Type": "application/json", **(headers or {})})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as res:
            return res.status, json.loads(res.read().decode() or "{}")
    except urllib.error.HTTPError as e:
        try:
            body = json.loads(e.read().decode() or "{}")
        except Exception:
            body = {}
        return e.code, body


def read_json(path, default):
    try:
        return json.loads(Path(path).read_text())
    except Exception:
        return default


def write_json(path, data):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(json.dumps(data, ensure_ascii=False, indent=1))
    os.replace(tmp, path)


# ---------- state ----------

def person_key(person):
    return norm(person).replace(" ", "-") or "shared"


def state_path(person):
    return STATE_DIR / f"{person_key(person)}.json"


def load_state(person):
    st = read_json(state_path(person), {})
    st.setdefault("series", {})
    st.setdefault("current", None)
    return st


def save_state(person, st):
    write_json(state_path(person), st)


def backend():
    """(base URL of the radio-drama API, device key) from the device config.

    Same channel as os-server's backend calls: backend_base_url / backend_api_key
    when set, else the LLM proxy base and key. The key never leaves this process.
    """
    cfg = read_json(DEVICE_CONFIG, {})
    base = (os.environ.get("RADIO_DRAMA_API_BASE") or cfg.get("backend_base_url") or cfg.get("llm_base_url") or "").strip()
    key = (cfg.get("backend_api_key") or cfg.get("llm_api_key") or "").strip()
    if not base or not key:
        raise RuntimeError("device is not set up with the Autonomous backend")
    base = base.rstrip("/")
    return (base if base.endswith("/radio-drama") else base + "/radio-drama"), key


def api(path, method="GET", payload=None, timeout=15):
    base, key = backend()
    try:
        return http_json(base + path, headers={"Authorization": f"Bearer {key}"}, timeout=timeout,
                         method=method, payload=payload)
    except (urllib.error.URLError, TimeoutError, OSError) as e:
        raise RuntimeError(f"backend unreachable: {type(e).__name__}") from None


def save_access(body):
    """Remember the subscription state the backend reports with each story."""
    if "subscribed" in body:
        write_json(STATE_DIR / "access.json", {"subscribed": bool(body.get("subscribed")),
                                               "access_until": body.get("access_until"), "checked_at": time.time()})


# ---------- catalog ----------

def load_catalog(fresh=False, language="en"):
    """Series in one language ("all" for every language); cached for CATALOG_TTL_S."""
    cache = STATE_DIR / "catalog.json"
    cached = read_json(cache, None)
    if cached and not fresh and time.time() - cached.get("fetched_at", 0) < CATALOG_TTL_S:
        return by_language(cached["series"], language)
    series, cursor = [], ""
    for _ in range(20):
        qs = {"limit": "50", **({"cursor": cursor} if cursor else {})}
        status, body = api(f"/series?{urllib.parse.urlencode(qs)}")
        if status != 200:
            break
        series += [s for s in body.get("series", []) if SLUG.match(s.get("slug", ""))]
        cursor = body.get("next_cursor") or ""
        if not cursor:
            break
    if not series:
        if cached:
            return by_language(cached["series"], language)
        raise RuntimeError("catalog unavailable")
    keep = ("slug", "title", "synopsis", "genre", "tropes", "episode_count", "language")
    series = [{k: s.get(k) for k in keep} for s in series]
    write_json(cache, {"fetched_at": time.time(), "series": series})
    return by_language(series, language)


def by_language(series, language):
    if not language or language == "all":
        return series
    return [s for s in series if (s.get("language") or "en") == language]


def load_series(slug, fresh=False):
    """Episode list of one story; cached for CATALOG_TTL_S (the play grant still checks access)."""
    cache = STATE_DIR / "series" / f"{slug}.json"
    cached = read_json(cache, None)
    if cached and not fresh and time.time() - cached.get("fetched_at", 0) < CATALOG_TTL_S:
        return cached["series"]
    status, body = api(f"/series/{urllib.parse.quote(slug)}")
    if status == 404:
        raise RuntimeError(f"story not found: {slug}")
    if status != 200 or not body.get("episodes"):
        raise RuntimeError(f"story unavailable: {slug} (http {status})")
    save_access(body)
    episodes = sorted(body["episodes"], key=lambda e: e.get("number", 0))
    series = {"slug": slug, "title": body.get("title", slug), "genre": body.get("genre", ""),
              "episodes": [{"id": e["id"], "number": e["number"], "title": e.get("title", ""),
                            "locked": bool(e.get("locked"))} for e in episodes]}
    write_json(cache, {"fetched_at": time.time(), "series": series})
    return series


def matches_genre(series, genre):
    want = genre_key(genre)
    if not want:
        return True
    hay = " ".join([genre_key(series.get("genre", "")), norm(" ".join(series.get("tropes") or [])), norm(series.get("title", ""))])
    return all(word in hay for word in want.split())


def match_story(catalog, query):
    """Best title match: (series, alternatives). Words in the title count most, then slug and tropes."""
    words = [w for w in norm(query).split() if w not in {"the", "a", "an", "of", "and", "story", "drama"}]
    if not words:
        return None, []
    scored = []
    for s in catalog:
        title = set(norm(s["title"]).split())
        extra = set(norm(s["slug"].replace("-", " ")).split()) | set(norm(" ".join(s.get("tropes") or [])).split())
        score = sum(2 if w in title else 1 if w in extra else 0 for w in words)
        if norm(query) and norm(query) in norm(s["title"]):
            score += 5
        if score:
            scored.append((score, s))
    scored.sort(key=lambda x: -x[0])
    if not scored:
        return None, []
    best = scored[0]
    alts = [s["title"] for sc, s in scored[1:4] if sc == best[0]]
    return best[1], alts


# ---------- progress ----------

def progress_of(st, slug):
    return st["series"].setdefault(slug, {"finished": [], "last_episode": 0, "offset_s": 0.0, "done": False})


def heard(st, slug):
    p = st["series"].get(slug)
    return bool(p and (p.get("finished") or p.get("last_episode")))


def record_stop(st, slug, episode, offset_s, finished, total):
    p = progress_of(st, slug)
    p["last_episode"] = episode
    p["updated_at"] = time.time()
    if finished:
        if episode not in p["finished"]:
            p["finished"].append(episode)
        p["offset_s"] = 0.0
        p["done"] = episode >= total
    else:
        p["offset_s"] = max(0.0, offset_s - RESUME_REWIND_S)


def resume_point(st, slug, total):
    """(episode, offset) to continue a story: mid-episode where it stopped, else the next one."""
    p = st["series"].get(slug)
    if not p or not p.get("last_episode"):
        return 1, 0.0
    ep = p["last_episode"]
    if ep in p.get("finished", []):
        return (ep + 1, 0.0) if ep < total else (None, 0.0)
    return ep, float(p.get("offset_s") or 0.0)


# ---------- announcement ----------

# Spoken by the helper before the episode starts: the agent's own reply arrives
# after playback has begun, when HAL refuses speech, so the title would be lost.
ANNOUNCE = {
    "en": {"new": "Here's {title}, episode {n}.", "resume": "Back to {title}, episode {n}.", "chain": "Episode {n}."},
    "vi": {"new": "Đây là {title}, tập {n} nhé.", "resume": "Nghe tiếp {title}, tập {n} nhé.", "chain": "Tập {n}."},
    "ja": {"new": "{title}、第{n}話です。", "resume": "{title}、第{n}話の続きです。", "chain": "第{n}話。"},
    "ko": {"new": "{title}, {n}화예요.", "resume": "{title}, {n}화 이어서 들려드릴게요.", "chain": "{n}화."},
    "ar": {"new": "إليك {title}، الحلقة {n}.", "resume": "نكمل {title}، الحلقة {n}.", "chain": "الحلقة {n}."},
    "zh-CN": {"new": "这是{title}，第{n}集。", "resume": "继续播放{title}，第{n}集。", "chain": "第{n}集。"},
    "zh-TW": {"new": "這是{title}，第{n}集。", "resume": "繼續播放{title}，第{n}集。", "chain": "第{n}集。"},
}
# How long to keep retrying while a filler or another line is still being spoken.
ANNOUNCE_RETRY_S = 3.0


def device_language():
    raw = str(read_json(DEVICE_CONFIG, {}).get("stt_language") or "").strip().replace("_", "-").lower()
    if raw in ("zh-tw", "zh-hk", "zh-hant"):
        return "zh-TW"
    if raw.startswith("zh"):
        return "zh-CN"
    base = raw.split("-")[0]
    return base if base in ANNOUNCE else "en"


def announce(kind, title, episode):
    """Speak one short line naming the episode; True when HAL accepted it."""
    text = ANNOUNCE[device_language()][kind].format(title=title, n=episode)
    deadline = time.time() + ANNOUNCE_RETRY_S
    while True:
        status, _ = hal("POST", "/voice/speak", {"text": text})
        if status == 200:
            return True
        if status != 409 or time.time() >= deadline:
            return False
        time.sleep(0.3)


# ---------- playback ----------

def locked_result(body):
    """Episode needs a subscription: the free episodes are used up for this story."""
    out = {"ok": False, "reason": "locked",
           "open_through": body.get("free_episodes") or body.get("open_through") or body.get("openThrough") or FREE_EPISODES}
    if body.get("subscribe_url"):
        out["subscribe_url"] = body["subscribe_url"]
    return out


def fetch_grant(episode_id):
    """A short-lived signed audio URL for one episode, granted by the backend for this device."""
    status, body = api(f"/episodes/{urllib.parse.quote(episode_id)}/play", method="POST", payload={})
    if status == 200 and body.get("url"):
        return {"ok": True, "url": body["url"]}
    # The backend answers 410 "not available in the app yet" for an episode past the free ones.
    if status in (402, 403, 410):
        return locked_result(body)
    if status == 429:
        return {"ok": False, "reason": "busy"}
    return {"ok": False, "reason": body.get("reason") or f"http_{status}"}


def done_marker(path):
    return Path(f"{path}.done")


def complete(path):
    """True once the file holds the whole (remaining) episode, not a download still in progress."""
    return Path(path).exists() and done_marker(path).exists()


def stop_streamer():
    """Stop the download of the previous episode so it does not compete for bandwidth."""
    pid_file = STATE_DIR / "stream.pid"
    try:
        os.killpg(int(pid_file.read_text()), 15)
    except Exception:
        pass
    pid_file.unlink(missing_ok=True)


def prune_cache(keep):
    """Drop the oldest cached episodes beyond CACHE_MAX_BYTES; never the files in `keep`."""
    keep = {str(k) for k in keep}
    files = sorted((f for f in CACHE_DIR.glob("*.aac") if str(f) not in keep), key=lambda f: f.stat().st_mtime)
    total = sum(f.stat().st_size for f in CACHE_DIR.glob("*.aac"))
    for f in files:
        if total <= CACHE_MAX_BYTES:
            break
        total -= f.stat().st_size
        f.unlink(missing_ok=True)
        done_marker(f).unlink(missing_ok=True)


def stream_to(url, dest, offset_s):
    """Remux the episode from offset_s into an ADTS file that HAL can read while it grows.

    Returns once STREAM_HEAD_BYTES are on disk; ffmpeg keeps downloading in its own
    session and drops a .done marker when the file is complete.
    """
    dest.parent.mkdir(parents=True, exist_ok=True)
    stop_streamer()
    for f in (dest, done_marker(dest)):
        f.unlink(missing_ok=True)
    script = ('ffmpeg -hide_banner -loglevel error -nostdin -y -user_agent "$4" -ss "$2" -i "$1" '
              '-vn -c:a copy -f adts "$3" && touch "$3.done"')
    proc = subprocess.Popen(["sh", "-c", script, "sh", url, f"{offset_s:.1f}", str(dest), USER_AGENT],
                            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                            start_new_session=True)
    write_json(STATE_DIR / "stream.pid", proc.pid)
    deadline = time.time() + STREAM_START_TIMEOUT_S
    while time.time() < deadline:
        if complete(dest) or (dest.exists() and dest.stat().st_size >= STREAM_HEAD_BYTES):
            return dest
        if proc.poll() is not None:
            break
        time.sleep(0.05)
    stop_streamer()
    raise RuntimeError("episode audio did not start downloading")


def trim_from(src, offset_s):
    """A copy of a fully downloaded episode starting at offset_s (stream copy, no re-encode)."""
    out = src.with_name(f"{src.stem}.from{int(offset_s)}{src.suffix}")
    subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-ss", f"{offset_s:.1f}",
                    "-i", str(src), "-c", "copy", "-f", "adts", str(out)], check=True, timeout=60)
    done_marker(out).touch()
    return out


def hal(method, path, payload=None, timeout=10):
    data = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(f"{HAL_BASE}{path}", data=data, method=method,
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as res:
            return res.status, json.loads(res.read().decode() or "{}")
    except urllib.error.HTTPError as e:
        return e.code, {}
    except Exception:
        return 0, {}


def start_episode(person, series, episode, offset_s, autoplay, chained=False):
    """Fetch, cache and hand one episode to HAL; a detached watcher records how it ends."""
    total = len(series["episodes"])
    if episode < 1 or episode > total:
        return {"ok": False, "reason": "no_such_episode", "story": series["title"], "episodes": total}
    ep = series["episodes"][episode - 1]
    if ep.get("locked"):
        # The cached list may predate a new subscription; ask the backend again before refusing.
        series = load_series(series["slug"], fresh=True)
        ep = series["episodes"][episode - 1]
    if ep.get("locked"):
        return {**locked_result({}), "story": series["title"], "episode": episode, "episodes": total}
    full = CACHE_DIR / f"{series['slug']}__ep{episode:02d}.aac"
    try:
        if complete(full):
            path = trim_from(full, offset_s) if offset_s > 1 else full
        else:
            grant = fetch_grant(ep["id"])
            if not grant["ok"]:
                return {**grant, "story": series["title"], "episode": episode, "episodes": total}
            dest = full if offset_s <= 1 else full.with_name(f"{full.stem}.from{int(offset_s)}.aac")
            path = stream_to(grant["url"], dest, offset_s)
        prune_cache([path, full])
    except Exception as e:
        return {"ok": False, "reason": "download_failed", "detail": str(e)[:200], "story": series["title"], "episode": episode}

    # HAL refuses speech while anything plays; the episode would replace it anyway.
    if hal("GET", "/audio/status")[1].get("playing"):
        hal("POST", "/audio/stop", {})
    # HAL's file player waits for this line to finish before the episode starts.
    announced = announce("chain" if chained else "resume" if offset_s > 1 else "new", series["title"], episode)
    body = {"query": str(path)}
    if person:
        body["person"] = person
    status, _ = hal("POST", "/audio/play", body)
    if status != 200:
        return {"ok": False, "reason": "speaker_busy" if status == 409 else f"hal_{status}", "story": series["title"], "episode": episode}

    st = load_state(person)
    prev = st.get("current")
    if prev and prev.get("file") != str(path):
        # HAL just cut the previous episode off; keep its place before replacing it.
        record_stop(st, prev["slug"], prev["episode"], prev["offset_s"] + time.time() - prev["started_at"],
                    False, prev["episodes"])
    st["current"] = {"slug": series["slug"], "title": series["title"], "episode": episode, "episodes": total,
                     "offset_s": offset_s, "file": str(path), "started_at": time.time()}
    p = progress_of(st, series["slug"])
    p["last_episode"] = episode
    p["offset_s"] = offset_s
    p["updated_at"] = time.time()
    save_state(person, st)

    subprocess.Popen([sys.executable, os.path.abspath(__file__), "_watch", "--person", person or "",
                      "--file", str(path), "--autoplay", str(autoplay)],
                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                     start_new_session=True)
    return {"ok": True, "action": "playing", "story": series["title"], "genre": series["genre"],
            "episode": episode, "episode_title": ep["title"], "episodes": total,
            "resumed_at_s": round(offset_s) if offset_s > 1 else 0, "announced": announced}


def how_it_ended(path, person):
    """HAL's history line for this file: 'end' (finished), 'user', 'tts' or 'error'."""
    qs = urllib.parse.urlencode({"last": 20, **({"person": person} if person else {})})
    _, body = hal("GET", f"/audio/history?{qs}")
    for entry in reversed(body.get("entries", [])):
        if entry.get("query") == path:
            return entry.get("stopped_by"), float(entry.get("duration_s") or 0)
    return None, 0.0


def watch_log(msg):
    try:
        log = STATE_DIR / "watch.log"
        if log.exists() and log.stat().st_size > 200_000:
            log.unlink()
        with open(log, "a") as f:
            f.write(f"{time.strftime('%Y-%m-%d %H:%M:%S')} {msg}\n")
    except Exception:
        pass


def cmd_watch(args):
    """Detached: wait for the episode to end, save progress, chain the next episode while autoplay lasts."""
    try:
        return _watch(args)
    except Exception as e:
        watch_log(f"watch failed for {args.file}: {type(e).__name__}: {e}")
        return 1


def _watch(args):
    person, path = args.person, args.file
    deadline = time.time() + 30
    seen = False
    while True:
        _, status = hal("GET", "/audio/status")
        playing_ours = status.get("playing") and status.get("title") == Path(path).name
        if playing_ours:
            seen = True
        elif seen or time.time() > deadline:
            break
        time.sleep(WATCH_POLL_S)
    time.sleep(1.0)  # let HAL write its history line
    st = load_state(person)
    cur = st.get("current")
    if not cur or cur.get("file") != path:
        return 0  # another story took over
    stopped_by, played = how_it_ended(path, person)
    if stopped_by is None:
        played = time.time() - cur["started_at"]
    # HAL reads a file that may still be downloading; reaching its end early is a stall, not the episode's end.
    stalled = stopped_by == "end" and not complete(path)
    finished = stopped_by == "end" and not stalled
    watch_log(f"{cur['slug']} ep{cur['episode']} ended by {'stall' if stalled else stopped_by} after {played:.0f}s "
              f"(autoplay left {args.autoplay - 1})")
    record_stop(st, cur["slug"], cur["episode"], cur["offset_s"] + played, finished, cur["episodes"])
    st["current"] = None
    st["last"] = {"slug": cur["slug"], "episode": cur["episode"], "stopped_by": stopped_by or "unknown"}
    save_state(person, st)
    if stalled:
        deadline = time.time() + STALL_WAIT_S
        while not complete(path) and time.time() < deadline:
            time.sleep(WATCH_POLL_S)
        if complete(path):
            st = load_state(person)
            episode, offset = resume_point(st, cur["slug"], cur["episodes"])
            if episode is not None and st.get("current") is None:
                out = start_episode(person, load_series(cur["slug"]), episode, offset, args.autoplay)
                watch_log(f"resume after stall: {json.dumps(out, ensure_ascii=False)}")
        return 0
    if finished and args.autoplay > 1 and cur["episode"] < cur["episodes"]:
        series = load_series(cur["slug"])
        out = start_episode(person, series, cur["episode"] + 1, 0.0, args.autoplay - 1, chained=True)
        watch_log(f"autoplay ep{cur['episode'] + 1}: {json.dumps(out, ensure_ascii=False)}")
    return 0


# ---------- commands ----------

def summary(s, st):
    p = st["series"].get(s["slug"]) or {}
    return {"title": s["title"], "slug": s["slug"], "genre": s.get("genre"), "language": s.get("language") or "en",
            "episodes": s.get("episode_count"),
            "heard": heard(st, s["slug"]), "last_episode": p.get("last_episode") or None, "done": p.get("done", False)}


def cmd_genres(args):
    groups = {}
    for s in load_catalog(language=args.language):
        key = genre_key(s.get("genre", ""))
        if key:
            groups.setdefault(key, 0)
            groups[key] += 1
    return emit({"ok": True, "genres": [{"genre": g, "stories": n} for g, n in sorted(groups.items(), key=lambda x: -x[1])]})


def cmd_list(args):
    st = load_state(args.person)
    items = [s for s in load_catalog(language=args.language) if matches_genre(s, args.genre or "")]
    if args.query:
        best, _ = match_story(items, args.query)
        items = [best] if best else []
    return emit({"ok": True, "stories": [summary(s, st) for s in items]})


def cmd_play(args):
    st = load_state(args.person)
    # A title search looks in every language; random picks stay in the chosen one.
    catalog = load_catalog(language="all" if args.story else args.language)
    if args.story:
        best, alts = match_story(catalog, args.story)
        if not best:
            return emit({"ok": False, "reason": "story_not_found", "query": args.story})
        series = load_series(best["slug"])
        total = len(series["episodes"])
        if args.episode:
            episode, offset = args.episode, 0.0
        elif args.restart:
            episode, offset = 1, 0.0
        else:
            episode, offset = resume_point(st, best["slug"], total)
            if episode is None:
                return emit({"ok": False, "reason": "story_finished", "story": series["title"], "episodes": total})
        out = start_episode(args.person, series, episode, offset, args.autoplay)
        if alts:
            out["also_matched"] = alts
        return emit(out)

    pool = [s for s in catalog if matches_genre(s, args.genre or "")]
    if not pool:
        return emit({"ok": False, "reason": "no_story_in_genre", "genre": args.genre})
    fresh = [s for s in pool if not heard(st, s["slug"])]
    if not fresh and not args.include_heard:
        return emit({"ok": False, "reason": "all_heard", "genre": args.genre or None, "stories": len(pool)})
    pick = random.choice(fresh or pool)
    series = load_series(pick["slug"])
    return emit(start_episode(args.person, series, 1, 0.0, args.autoplay))


def last_slug(st):
    if st.get("current"):
        return st["current"]["slug"]
    if st.get("last"):
        return st["last"]["slug"]
    dated = [(p.get("updated_at", 0), slug) for slug, p in st["series"].items() if not p.get("done")]
    return max(dated)[1] if dated else None


def cmd_continue(args):
    st = load_state(args.person)
    slug = last_slug(st)
    if not slug:
        return emit({"ok": False, "reason": "nothing_to_continue"})
    series = load_series(slug)
    total = len(series["episodes"])
    if st.get("current"):
        stop_playback(args.person)
        st = load_state(args.person)
    episode, offset = resume_point(st, slug, total)
    if episode is None:
        return emit({"ok": False, "reason": "story_finished", "story": series["title"], "episodes": total})
    return emit(start_episode(args.person, series, episode, offset, args.autoplay))


def cmd_step(args, delta):
    st = load_state(args.person)
    slug = last_slug(st)
    if not slug:
        return emit({"ok": False, "reason": "nothing_playing"})
    cur_ep = (st.get("current") or {}).get("episode") or st["series"].get(slug, {}).get("last_episode") or 1
    if st.get("current"):
        stop_playback(args.person)
    series = load_series(slug)
    return emit(start_episode(args.person, series, cur_ep + delta, 0.0, args.autoplay))


def stop_playback(person):
    st = load_state(person)
    cur = st.get("current")
    hal("POST", "/audio/stop", {})
    stop_streamer()
    if not cur:
        return None
    # Record here too, in case the watcher already lost the race.
    played = time.time() - cur["started_at"]
    record_stop(st, cur["slug"], cur["episode"], cur["offset_s"] + played, False, cur["episodes"])
    st["current"] = None
    st["last"] = {"slug": cur["slug"], "episode": cur["episode"], "stopped_by": "user"}
    save_state(person, st)
    return cur


def cmd_stop(args):
    cur = stop_playback(args.person)
    if not cur:
        return emit({"ok": True, "action": "stopped", "story": None})
    return emit({"ok": True, "action": "stopped", "story": cur["title"], "episode": cur["episode"]})


def cmd_status(args):
    st = load_state(args.person)
    cur = st.get("current")
    out = {"ok": True, "playing": bool(cur)}
    if cur:
        out.update(story=cur["title"], episode=cur["episode"], episodes=cur["episodes"],
                   position_s=round(cur["offset_s"] + time.time() - cur["started_at"]))
    slug = last_slug(st)
    if slug and not cur:
        p = st["series"].get(slug, {})
        out["last_story"] = {"slug": slug, "episode": p.get("last_episode"), "offset_s": round(p.get("offset_s") or 0),
                             "done": p.get("done", False)}
    access = read_json(STATE_DIR / "access.json", {})
    if access:
        out["subscribed"] = access.get("subscribed", False)
        if access.get("access_until"):
            out["access_until"] = access["access_until"]
    return emit(out)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--person", default="", help="identified speaker; progress is kept per person")
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("genres")
    p.add_argument("--language", default="en", help="story language code, or 'all' (default en)")
    p = sub.add_parser("list")
    p.add_argument("--language", default="en")
    p.add_argument("--genre")
    p.add_argument("--query")
    p = sub.add_parser("play")
    p.add_argument("--random", action="store_true")
    p.add_argument("--story")
    p.add_argument("--genre")
    p.add_argument("--episode", type=int)
    p.add_argument("--restart", action="store_true")
    p.add_argument("--include-heard", action="store_true")
    p.add_argument("--language", default="en")
    p.add_argument("--autoplay", type=int, default=DEFAULT_AUTOPLAY)
    for name in ("continue", "next", "previous"):
        sp = sub.add_parser(name)
        sp.add_argument("--autoplay", type=int, default=DEFAULT_AUTOPLAY)
    sub.add_parser("stop")
    sub.add_parser("status")
    w = sub.add_parser("_watch")
    w.add_argument("--file", required=True)
    w.add_argument("--autoplay", type=int, default=1)
    w.add_argument("--person", dest="watch_person", default="")
    args = ap.parse_args(argv)
    if args.cmd == "_watch":
        args.person = args.watch_person
        return cmd_watch(args)
    try:
        return {
            "genres": cmd_genres, "list": cmd_list, "play": cmd_play, "continue": cmd_continue,
            "next": lambda a: cmd_step(a, 1), "previous": lambda a: cmd_step(a, -1),
            "stop": cmd_stop, "status": cmd_status,
        }[args.cmd](args)
    except RuntimeError as e:
        return emit({"ok": False, "reason": "unavailable", "detail": str(e)})


if __name__ == "__main__":
    sys.exit(main())
