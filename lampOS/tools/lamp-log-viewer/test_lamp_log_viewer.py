"""Checks for the lamp log viewer (run: python3 -m unittest test_lamp_log_viewer, from this folder).

Every journal line here is a fixture with made-up timings and no words a person said; no lamp is
needed.
"""
import json
import tempfile
import threading
import unittest
import urllib.error
import urllib.request
from http.server import ThreadingHTTPServer
from pathlib import Path

import lamp_log_viewer as v

T0 = 1_800_000_000_000  # a made-up start, in ms


def line(at_ms, message):
    """One `journalctl -o json` line of lamp-voice at `at_ms`."""
    return json.dumps({"__REALTIME_TIMESTAMP": str(at_ms * 1000), "MESSAGE": f"lamp-voice: {message}"})


def chat(at_ms, first, end, prompt=1000, cached=800, completion=50):
    return line(at_ms, f"chat answer: its first words or call {first} ms and its end {end} ms after it was asked; "
                       f"{prompt} prompt tokens, {cached} from the cache, {prompt - cached} not; {completion} completion tokens")


def request_lines(n, round_at, piece_at):
    return [
        line(round_at - 900, f"request {n}: speech to text kept ready in 120 ms"),
        line(round_at - 800, f"request {n} heard 4 words, the last 310 ms after the line ended"),
        line(round_at, f"request {n} chat round 1 first event in 420 ms"),
        line(piece_at, f"request {n} first piece to the voice 1500 ms after the words were heard"),
        line(piece_at + 10, f"request {n} ElevenLabs piece 1: 60 chars, sent 1510 ms after the words were heard on a warm connection; its first audio byte 380 ms after it went"),
        line(piece_at + 400, f"request {n} first answer audio 1900 ms after the words were heard"),
    ]


class Parser(unittest.TestCase):
    def test_every_timing_of_one_request_is_read(self):
        lines = request_lines(7, T0 + 1000, T0 + 2500) + [chat(T0 + 2000, 400, 1400, completion=50)]
        (row,) = v.parse_metrics(lines)
        self.assertEqual(row["request"], 7)
        self.assertEqual((row["stt_ready_ms"], row["words"], row["stt_last_ms"]), (120, 4, 310))
        self.assertEqual(row["llm_first_event_ms"], 420)
        self.assertEqual((row["first_piece_ms"], row["piece_at"]), (1500, T0 + 2500))
        self.assertEqual((row["tts_chars"], row["tts_conn"], row["tts_first_byte_ms"]), (60, "warm", 380))
        self.assertEqual(row["first_audio_ms"], 1900)
        self.assertNotIn("round_at", row)

    def test_a_request_with_no_first_piece_is_left_out(self):
        self.assertEqual(v.parse_metrics([line(T0, "request 3 chat round 1 first event in 300 ms")]), [])

    def test_lines_that_are_not_json_or_not_the_voices_are_skipped(self):
        other = json.dumps({"__REALTIME_TIMESTAMP": str(T0 * 1000), "MESSAGE": "lamp-body: request 9 first piece to the voice 5 ms after the words were heard"})
        self.assertEqual(v.parse_metrics(["not json", other, json.dumps({"MESSAGE": ["bytes"]})]), [])


class Pairing(unittest.TestCase):
    def test_a_chat_answer_pairs_with_the_request_whose_gap_it_matches(self):
        # round 1 at T0+1000, the answer line at T0+2000: (answer_at - round_at) = 1000 = end - first = 1400 - 400
        lines = request_lines(1, T0 + 1000, T0 + 2500) + [chat(T0 + 2000, 400, 1400, completion=50)]
        (row,) = v.parse_metrics(lines)
        self.assertEqual((row["llm_ttft_ms"], row["llm_end_ms"]), (400, 1400))
        self.assertEqual((row["prompt_tokens"], row["cached_tokens"], row["completion_tokens"]), (1000, 800, 50))
        self.assertEqual(row["gen_tps"], 50.0)  # 50 tokens over (1400 - 400) ms

    def test_a_side_call_pairs_with_nothing(self):
        # A side call (a summary) ends 2 s after its request's round with its own first/end gap of 600 ms: no match.
        lines = request_lines(1, T0 + 1000, T0 + 2500) + [chat(T0 + 3000, 300, 900, completion=20)]
        (row,) = v.parse_metrics(lines)
        self.assertNotIn("llm_ttft_ms", row)
        self.assertNotIn("gen_tps", row)

    def test_a_side_call_does_not_take_the_answer_that_is_the_requests(self):
        lines = request_lines(1, T0 + 1000, T0 + 2500) + [chat(T0 + 1500, 100, 1100, completion=9), chat(T0 + 2000, 400, 1400, completion=50)]
        (row,) = v.parse_metrics(lines)
        self.assertEqual(row["completion_tokens"], 50)

    def test_two_requests_each_take_their_own_answer(self):
        lines = (request_lines(1, T0 + 1000, T0 + 2500) + request_lines(2, T0 + 20_000, T0 + 21_500)
                 + [chat(T0 + 2000, 400, 1400, completion=50), chat(T0 + 20_700, 200, 900, completion=30)])
        rows = {r["request"]: r for r in v.parse_metrics(lines)}
        self.assertEqual((rows[1]["completion_tokens"], rows[1]["llm_end_ms"]), (50, 1400))
        self.assertEqual((rows[2]["completion_tokens"], rows[2]["llm_end_ms"]), (30, 900))

    def test_the_gap_must_match_within_the_slack(self):
        near = v.parse_metrics(request_lines(1, T0 + 1000, T0 + 2500) + [chat(T0 + 2000 + v.PAIR_SLACK_MS - 1, 400, 1400)])
        far = v.parse_metrics(request_lines(1, T0 + 1000, T0 + 2500) + [chat(T0 + 2000 + v.PAIR_SLACK_MS + 1, 400, 1400)])
        self.assertIn("llm_ttft_ms", near[0])
        self.assertNotIn("llm_ttft_ms", far[0])

    def test_an_answer_before_the_round_or_long_after_it_pairs_with_nothing(self):
        before = v.parse_metrics(request_lines(1, T0 + 1000, T0 + 2500) + [chat(T0 + 500, 400, 1400)])
        after = v.parse_metrics(request_lines(1, T0 + 1000, T0 + 2500) + [chat(T0 + 1000 + v.PAIR_WITHIN_MS + 1000, 400, 1400)])
        self.assertNotIn("llm_ttft_ms", before[0])
        self.assertNotIn("llm_ttft_ms", after[0])


class TurnMatch(unittest.TestCase):
    ROWS = [{"request": 1, "piece_at": T0 + 2500}, {"request": 2, "piece_at": T0 + 21_500}]

    def test_the_turn_takes_the_request_whose_first_piece_time_is_nearest(self):
        self.assertEqual(v.match_turn(T0 + 2504, self.ROWS)["request"], 1)
        self.assertEqual(v.match_turn(T0 + 21_320, self.ROWS)["request"], 2)

    def test_a_turn_past_the_window_has_no_timings(self):
        self.assertIsNone(v.match_turn(T0 + 2500 + 301, self.ROWS))
        self.assertIsNone(v.match_turn(T0, []))


class Pieces(unittest.TestCase):
    def test_the_lamp_argument_is_a_user_at_a_host_and_never_an_ssh_option(self):
        for good in ["orangepi@lamp-ab12.local", "orangepi@10.0.0.7"]:
            self.assertTrue(v.LAMP_ARGUMENT.match(good), good)
        for bad in ["", "lamp-ab12.local", "-oProxyCommand=x@y", "a@-b", "a b@c", "a@b;c", "a@b\nc"]:
            self.assertFalse(v.LAMP_ARGUMENT.match(bad), repr(bad))

    def test_the_lamps_name_comes_from_its_argument_and_the_cache_is_where_it_is_told(self):
        with tempfile.TemporaryDirectory() as folder:
            lamp = v.Lamp("orangepi@lamp-ab12.local", Path(folder) / "cache")
            self.assertEqual(lamp.name, "lamp-ab12")
            lamp.cache_merge([{"request": 1, "piece_at": T0, "first_piece_ms": 5}])
            self.assertEqual(lamp.cache.parent, Path(folder) / "cache")
            self.assertEqual(list(json.loads(lamp.cache.read_text())), [str(T0)])

    def test_no_lamp_is_written_into_the_code(self):
        here = Path(__file__).parent
        for name in ["lamp_log_viewer.py", "index.html"]:
            self.assertNotIn("75c9", (here / name).read_text(), name)

    def test_a_stream_line_is_the_pages_shape_and_bytes_are_decoded(self):
        raw = json.dumps({"__REALTIME_TIMESTAMP": "1800000000123000", "_SYSTEMD_UNIT": "lamp-voice.service", "PRIORITY": "4", "MESSAGE": list(b"hi")})
        self.assertEqual(v.stream_line(raw), {"t": 1_800_000_000_123, "unit": "lamp-voice", "p": 4, "m": "hi"})
        self.assertIsNone(v.stream_line("not json"))


class MemoryRead(unittest.TestCase):
    """The Conversation tab's one read of lamp-memory's store: read only, active exchanges only."""

    def test_the_query_opens_the_store_read_only_and_takes_active_exchanges_only(self):
        self.assertIn("?mode=ro", v.CONVO_READ)
        self.assertIn("uri=True", v.CONVO_READ)
        self.assertIn("active = 1", v.CONVO_READ)
        self.assertIn("kind = 'exchange'", v.CONVO_READ)
        self.assertEqual(v.CONVO_READ.lower().count("select"), 1)

    def test_nothing_that_writes_or_restarts_is_in_the_query_or_the_remote_command(self):
        lamp = v.Lamp("orangepi@lamp-ab12.local", Path("/nonexistent"))
        seen = []
        real = v.subprocess.run

        def fake(cmd, **kwargs):
            seen.append((cmd, kwargs.get("input")))

            class Done:
                returncode = 0
                stdout = "[]"
                stderr = ""
            return Done()

        v.subprocess.run = fake
        try:
            lamp.conversation(0, 10)
        finally:
            v.subprocess.run = real
        ((cmd, sent),) = seen
        remote = cmd[-1]
        self.assertEqual(remote, "sudo -n python3 -I - 0 10")  # only the store reader runs under sudo
        for text in (remote, sent):
            for word in ["insert", "update", "delete", "drop", "alter", "create", "replace", "pragma", "systemctl", "rm ", "kill", "reboot", "write", "open(", "executescript", "commit"]:
                self.assertNotIn(word, text.lower(), word)


class Server(unittest.TestCase):
    def test_only_its_own_host_name_is_served_and_the_page_learns_the_lamps_name(self):
        with tempfile.TemporaryDirectory() as folder:
            lamp = v.Lamp("orangepi@lamp-ab12.local", Path(folder))
            server = ThreadingHTTPServer(("127.0.0.1", 0), v.make_handler(lamp, 0))
            port = server.server_address[1]
            server.RequestHandlerClass = v.make_handler(lamp, port)
            threading.Thread(target=server.serve_forever, daemon=True).start()
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{port}/info") as ok:
                    self.assertEqual(json.load(ok), {"lamp": "lamp-ab12"})
                rebind = urllib.request.Request(f"http://127.0.0.1:{port}/info", headers={"Host": "evil.example"})
                with self.assertRaises(urllib.error.HTTPError) as caught:
                    urllib.request.urlopen(rebind)
                self.assertEqual(caught.exception.code, 403)
            finally:
                server.shutdown()
                server.server_close()


if __name__ == "__main__":
    unittest.main()
