import json, sys, pathlib, unittest
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import review_checks as r


class T(unittest.TestCase):
    def test_ffprobe(self):
        j = json.dumps({"format": {"duration": "61.5"}, "streams": [
            {"codec_type": "video", "codec_name": "h264", "width": 1920, "height": 1080, "r_frame_rate": "30000/1001", "pix_fmt": "yuv420p", "duration": "61.5"},
            {"codec_type": "audio", "codec_name": "aac", "duration": "61.0"}]})
        f = r.parse_ffprobe(j)
        self.assertEqual((f["width"], f["acodec"]), (1920, "aac"))
        self.assertAlmostEqual(f["fps"], 29.97, 2)

    def test_loudnorm_and_checks(self):
        err = 'junk\n{\n "input_i" : "-20.1",\n "input_tp" : "0.4",\n "input_lra" : "5"\n}\n'
        l = r.parse_loudnorm(err)
        self.assertEqual(l["lufs"], -20.1)
        sev = {(f["check"], f["severity"]) for f in r.check_loudness(l)}
        self.assertIn(("loudness", "blocker"), sev)
        self.assertIn(("audio", "blocker"), sev)
        self.assertEqual(r.check_loudness({"lufs": -14.3, "true_peak": -2}), [])

    def test_detect_parsers(self):
        s = "silence_start: 3.5\nsilence_end: 9.1 | silence_duration: 5.6\n"
        self.assertEqual(r.parse_silencedetect(s), [(3.5, 9.1)])
        self.assertEqual(r.parse_blackdetect("black_start:10 black_end:12.5 black_duration:2.5"), [(10.0, 12.5)])
        self.assertEqual(r.parse_scene_times("pts_time:1.5 x pts_time:4"), [1.5, 4.0])
        fz = "lavfi.freezedetect.freeze_start: 2\nlavfi.freezedetect.freeze_end: 8\n"
        self.assertEqual(r.parse_freezedetect(fz), [(2.0, 8.0)])

    def test_gaps(self):
        f = r.check_gaps("black", [(30, 32)], 100)
        self.assertEqual(f[0]["severity"], "blocker")
        self.assertEqual(r.check_gaps("silence", [(5, 6)], 100), [])
        self.assertEqual(r.check_gaps("silence", [(5, 7.5)], 100)[0]["severity"], "info")
        self.assertEqual(r.check_gaps("silence", [(5, 11)], 100)[0]["severity"], "warn")

    def test_format_sync_flash_text(self):
        facts = {"width": 1280, "height": 720, "fps": 30, "vcodec": "h264", "pix_fmt": "yuv420p", "acodec": None, "duration": 100}
        names = {(f["check"], f["severity"]) for f in r.check_format(facts, {"width": 1920, "height": 1080, "fps": 30}, "TikTok")}
        self.assertIn(("format", "blocker"), names)
        self.assertIn(("track", "blocker"), names)
        self.assertEqual(r.check_sync(10, 10.05), [])
        self.assertEqual(r.check_sync(10, 11)[0]["severity"], "blocker")
        strobe = [0, 200] * 30
        self.assertGreater(r.flashes_per_second(strobe, 30), 3)
        self.assertEqual(r.flashes_per_second([100] * 60, 30), 0)
        self.assertEqual(r.check_text("hello TODO", "")[0]["check"], "placeholder")
        self.assertEqual(r.check_text("hi", "Sponsored by Acme Corp")[0]["check"], "sponsor")

    def test_routing_and_loop_guard(self):
        a = r.finding("loudness", "blocker", 0, "x")
        b = r.finding("overlay", "blocker", 12, "covers face")
        w = r.finding("silence", "warn", 4, "gap")
        d = r.decide([a, b], [], 0)
        self.assertEqual((d["action"], d["node"]), ("reopen", "motion_source"))  # earliest node in graph order first
        self.assertEqual(r.decide([w], [], 0)["action"], "pass")
        self.assertEqual(r.decide([], [], 0)["note"], "auto-review: clean")
        self.assertEqual(r.decide([a], [], r.MAX_AUTO_REVIEWS)["action"], "escalate")
        self.assertEqual(r.decide([a], [r.key(a)], 0)["action"], "clean")  # accepted by the human

    def test_samples(self):
        t = r.sample_times(10, [3.0])
        self.assertIn(0.0, t); self.assertIn(9.0, t); self.assertIn(3.05, t)
        self.assertLessEqual(len(r.sample_times(2000, [], cap=50)), 50)


if __name__ == "__main__":
    unittest.main()
