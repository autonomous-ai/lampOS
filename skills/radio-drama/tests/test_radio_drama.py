"""Behavior tests for the radio-drama helper; no network, no HAL."""
import contextlib
import importlib.util
import io
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).parents[1] / "scripts/radio_drama.py"

CATALOG = [
    {"slug": "the-vampire-princes-deadly-bride", "title": "The Vampire Prince's Deadly Bride", "genre": "gothic romance",
     "tropes": ["arranged-marriage"], "episode_count": 3},
    {"slug": "the-devil-and-his-only-believer", "title": "The Devil and His Only Believer", "genre": "romantic comedy",
     "tropes": ["power-fueled-by-belief"], "episode_count": 3},
    {"slug": "moms-out-of-prison", "title": "Mom's Out of Prison", "genre": "family-revenge",
     "tropes": ["wrongly-convicted-mother-returns"], "episode_count": 3},
]


def series(slug):
    s = next(x for x in CATALOG if x["slug"] == slug)
    return {"slug": slug, "title": s["title"], "genre": s["genre"],
            "episodes": [{"id": f"{slug[:4]}{n:020d}", "number": n, "title": f"Ep {n}"} for n in (1, 2, 3)]}


class RadioDramaTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        env = {"RADIO_DRAMA_STATE_DIR": f"{self.tmp.name}/state", "RADIO_DRAMA_CACHE_DIR": f"{self.tmp.name}/cache",
               "RADIO_DRAMA_DEVICE_CONFIG": f"{self.tmp.name}/config.json"}
        with patch.dict(os.environ, env):
            spec = importlib.util.spec_from_file_location("radio_drama_skill", SCRIPT)
            self.m = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(self.m)
        self.played = []
        self.spoken = []
        self.real_fetch_grant = self.m.fetch_grant
        self.real_load_catalog = self.m.load_catalog
        self.m.load_catalog = lambda fresh=False, language="en": self.m.by_language(CATALOG, language)
        self.m.load_series = lambda slug, fresh=False: series(slug)
        self.m.fetch_grant = lambda episode_id: {"ok": True, "url": "https://cdn.example/a.m4a"}
        self.streamed = []
        self.m.stream_to = self.fake_stream
        self.m.trim_from = lambda src, offset: src.with_name(f"{src.stem}.from{int(offset)}{src.suffix}")
        self.m.hal = self.fake_hal
        self.popen = patch.object(self.m.subprocess, "Popen").start()

    def tearDown(self):
        patch.stopall()
        self.tmp.cleanup()

    def fake_stream(self, url, dest, offset_s):
        self.streamed.append((dest.name, offset_s))
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(b"x")
        self.m.done_marker(dest).touch()
        return dest

    def fake_hal(self, method, path, payload=None, timeout=10):
        if path == "/audio/play":
            self.played.append(payload["query"])
        if path == "/voice/speak":
            self.spoken.append(payload["text"])
        return 200, {}

    def run_cli(self, *argv):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.m.main(list(argv))
        return json.loads(out.getvalue().strip().splitlines()[-1])

    def finish_current(self, person=""):
        st = self.m.load_state(person)
        cur = st["current"]
        self.m.record_stop(st, cur["slug"], cur["episode"], 300, True, cur["episodes"])
        st["current"] = None
        st["last"] = {"slug": cur["slug"], "episode": cur["episode"], "stopped_by": "end"}
        self.m.save_state(person, st)

    def test_random_skips_stories_already_heard(self):
        seen = {self.run_cli("play", "--random")["story"] for _ in range(3)}
        self.assertEqual(len(seen), 3)
        self.assertEqual(self.run_cli("play", "--random")["reason"], "all_heard")
        self.assertTrue(self.run_cli("play", "--random", "--include-heard")["ok"])

    def test_genre_matches_hyphenated_and_spaced_forms(self):
        self.assertEqual(self.run_cli("play", "--random", "--genre", "family revenge")["story"], "Mom's Out of Prison")
        self.assertEqual(self.run_cli("play", "--random", "--genre", "vampire")["story"], "The Vampire Prince's Deadly Bride")
        self.assertEqual(self.run_cli("play", "--random", "--genre", "sci-fi")["reason"], "no_story_in_genre")

    def test_story_by_partial_title(self):
        out = self.run_cli("play", "--story", "devil believer")
        self.assertEqual((out["story"], out["episode"]), ("The Devil and His Only Believer", 1))
        self.assertEqual(self.run_cli("play", "--story", "spaceship")["reason"], "story_not_found")

    def test_continue_resumes_mid_episode_with_rewind(self):
        self.run_cli("play", "--story", "vampire")
        st = self.m.load_state("")
        cur = st["current"]
        self.m.record_stop(st, cur["slug"], cur["episode"], 100.0, False, cur["episodes"])
        st["current"] = None
        st["last"] = {"slug": cur["slug"], "episode": 1, "stopped_by": "tts"}
        self.m.save_state("", st)
        out = self.run_cli("continue")
        self.assertEqual((out["episode"], out["resumed_at_s"]), (1, 97))
        self.assertTrue(self.played[-1].endswith(".from97.aac"))

    def test_continue_after_finished_episode_goes_to_next_and_ends_story(self):
        self.run_cli("play", "--story", "vampire")
        self.finish_current()
        self.assertEqual(self.run_cli("continue")["episode"], 2)
        self.finish_current()
        self.assertEqual(self.run_cli("continue")["episode"], 3)
        self.finish_current()
        self.assertEqual(self.run_cli("continue")["reason"], "story_finished")
        self.assertEqual(self.run_cli("play", "--story", "vampire", "--restart")["episode"], 1)

    def test_progress_is_per_person(self):
        self.run_cli("--person", "alice", "play", "--story", "vampire")
        self.finish_current("alice")
        self.assertEqual(self.run_cli("--person", "alice", "continue")["episode"], 2)
        self.assertEqual(self.run_cli("--person", "bob", "continue")["reason"], "nothing_to_continue")

    def test_switching_story_keeps_place_of_the_previous_one(self):
        self.run_cli("play", "--story", "vampire", "--episode", "2")
        self.run_cli("play", "--story", "devil")
        p = self.m.load_state("")["series"]["the-vampire-princes-deadly-bride"]
        self.assertEqual(p["last_episode"], 2)
        self.assertNotIn(2, p["finished"])

    def test_locked_episode_is_reported(self):
        self.m.fetch_grant = lambda episode_id: {"ok": False, "reason": "locked", "open_through": 5}
        out = self.run_cli("play", "--story", "vampire", "--episode", "3")
        self.assertEqual((out["ok"], out["reason"], out["open_through"]), (False, "locked", 5))
        self.assertEqual(self.played, [])

    def test_cached_episode_replays_without_backend_call(self):
        self.run_cli("play", "--story", "vampire")
        self.m.fetch_grant = lambda episode_id: self.fail("cached episode must not need a new grant")
        self.assertTrue(self.run_cli("play", "--story", "vampire", "--restart")["ok"])
        self.assertEqual(len(self.streamed), 1)

    def test_stall_before_download_finishes_is_not_counted_as_finished(self):
        self.m.stream_to = lambda url, dest, offset_s: (dest.parent.mkdir(parents=True, exist_ok=True),
                                                        dest.write_bytes(b"x"), dest)[2]
        self.run_cli("play", "--story", "vampire", "--autoplay", "1")
        cur = self.m.load_state("")["current"]
        self.m.STALL_WAIT_S = 0
        self.m.WATCH_POLL_S = 0
        self.m.how_it_ended = lambda path, person: ("end", 40.0)
        polls = iter([{"playing": True, "title": Path(cur["file"]).name}])
        self.m.hal = lambda method, path, payload=None, timeout=10: (200, next(polls, {"playing": False}))
        with patch.object(self.m.time, "sleep"):
            self.m._watch(type("A", (), {"person": "", "file": cur["file"], "autoplay": 1})())
        p = self.m.load_state("")["series"]["the-vampire-princes-deadly-bride"]
        self.assertNotIn(1, p["finished"])
        self.assertEqual((p["last_episode"], round(p["offset_s"])), (1, 37))  # 40 s played, minus the resume rewind

    def test_cache_keeps_newest_episodes_within_limit(self):
        cache = Path(self.m.CACHE_DIR)
        cache.mkdir(parents=True)
        for i, name in enumerate(["a.aac", "b.aac", "c.aac"]):
            (cache / name).write_bytes(b"x" * 100)
            os.utime(cache / name, (i, i))
        self.m.CACHE_MAX_BYTES = 200
        self.m.prune_cache([cache / "a.aac"])
        self.assertEqual(sorted(f.name for f in cache.glob("*.aac")), ["a.aac", "c.aac"])

    def test_episode_is_announced_in_device_language_before_playback(self):
        self.write_config(stt_language="vi")
        out = self.run_cli("play", "--story", "vampire")
        self.assertTrue(out["announced"])
        self.assertEqual(self.spoken, ["Đây là The Vampire Prince's Deadly Bride, tập 1 nhé."])
        self.write_config(stt_language="ko-KR")
        self.run_cli("play", "--story", "devil", "--episode", "2")
        self.assertEqual(self.spoken[-1], "The Devil and His Only Believer, 2화예요.")

    def test_announcement_waits_for_a_filler_then_gives_up(self):
        replies = iter([409, 409, 200])
        self.m.hal = lambda method, path, payload=None, timeout=10: (
            (next(replies), {}) if path == "/voice/speak" else (200, {}))
        with patch.object(self.m.time, "sleep"):
            self.assertTrue(self.m.announce("new", "T", 1))
        self.m.hal = lambda method, path, payload=None, timeout=10: (409, {})
        self.m.ANNOUNCE_RETRY_S = 0
        self.assertFalse(self.m.announce("new", "T", 1))

    def test_unknown_language_falls_back_to_english(self):
        self.write_config(stt_language="multi")
        self.run_cli("play", "--story", "vampire")
        self.assertEqual(self.spoken[-1], "Here's The Vampire Prince's Deadly Bride, episode 1.")

    def write_config(self, **cfg):
        Path(self.m.DEVICE_CONFIG).write_text(json.dumps(cfg))

    def test_backend_call_uses_device_key_and_hides_it(self):
        self.write_config(llm_base_url="https://be.example/api/v1/ai/v1/", llm_api_key="dk-secret-key")
        sent = {}

        def fake_http(url, headers=None, timeout=15, method="GET", payload=None):
            sent.update(url=url, method=method, auth=(headers or {}).get("Authorization"))
            return 200, {"episode_id": "abc", "url": "https://cdn.example/a.m4a"}

        self.m.http_json = fake_http
        self.assertTrue(self.real_fetch_grant("abc")["ok"])
        self.assertEqual(sent, {"url": "https://be.example/api/v1/ai/v1/radio-drama/episodes/abc/play",
                                "method": "POST", "auth": "Bearer dk-secret-key"})
        self.assertNotIn("dk-secret-key", json.dumps(self.run_cli("status")))

    def test_backend_base_url_wins_over_llm_base(self):
        self.write_config(backend_base_url="https://be2.example/v1", backend_api_key="k2",
                          llm_base_url="https://llm.example/v1", llm_api_key="k1")
        self.assertEqual(self.m.backend(), ("https://be2.example/v1/radio-drama", "k2"))

    def test_subscription_required_maps_to_locked_with_subscribe_link(self):
        self.write_config(llm_base_url="https://be.example/v1", llm_api_key="k")
        self.m.http_json = lambda *a, **k: (403, {"reason": "subscription_required", "free_episodes": 5,
                                                  "subscribe_url": "https://www.autonomous.ai/radio-drama"})
        self.assertEqual(self.real_fetch_grant("abc"), {"ok": False, "reason": "locked", "open_through": 5,
                                                        "subscribe_url": "https://www.autonomous.ai/radio-drama"})

    def test_gone_episode_maps_to_locked(self):
        self.write_config(llm_base_url="https://be.example/v1", llm_api_key="k")
        self.m.http_json = lambda *a, **k: (410, {"error": "this episode is not available in the app yet"})
        self.assertEqual(self.real_fetch_grant("abc"), {"ok": False, "reason": "locked", "open_through": 5})

    def test_episode_marked_locked_is_not_requested(self):
        locked = series("the-vampire-princes-deadly-bride")
        locked["episodes"][2]["locked"] = True
        self.m.load_series = lambda slug, fresh=False: locked
        self.m.fetch_grant = lambda episode_id: self.fail("locked episode must not be requested")
        out = self.run_cli("play", "--story", "vampire", "--episode", "3")
        self.assertEqual((out["reason"], out["open_through"]), ("locked", 5))

    def test_unconfigured_device_reports_unavailable(self):
        self.m.load_catalog = self.real_load_catalog
        out = self.run_cli("play", "--random")
        self.assertEqual(out["reason"], "unavailable")

    def test_random_stays_in_language_but_title_search_spans_all(self):
        es = {"slug": "destinada-al-alfa", "title": "Destinada al alfa", "genre": "romance de hombres lobo",
              "tropes": [], "episode_count": 3, "language": "es"}
        CATALOG.append(es)
        try:
            self.assertEqual(self.run_cli("play", "--random", "--genre", "hombres lobo")["reason"], "no_story_in_genre")
            self.assertEqual(self.run_cli("play", "--random", "--language", "es")["story"], "Destinada al alfa")
            self.assertEqual(self.run_cli("play", "--story", "destinada", "--restart")["story"], "Destinada al alfa")
        finally:
            CATALOG.remove(es)

if __name__ == "__main__":
    unittest.main()
