"""A compile or link that failed forgets the lift, unless every file that failed is a source of the repository: a typo in host/
is not a translation gone wrong, and translating the game again cost ~5 minutes on Windows (#2675). Game-free.

python -I -B tests/test_failed_compile_keeps_lift.py
"""
import contextlib
import importlib.util
import io
import os
from pathlib import Path
import re
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("make_game_under_test", ROOT / "nx2" / "make_game.py")
make_game = importlib.util.module_from_spec(spec)
spec.loader.exec_module(make_game)


def work_folder(root):
    """A work folder after a finished lift (its key), with the objects of the compile before it"""
    work = Path(root) / "work"
    (work / "sa" / "gen").mkdir(parents=True)
    (work / "sa" / "build" / "obj").mkdir(parents=True)
    (work / "objcache").mkdir()
    (work / "sa" / "gen" / make_game.LIFT_KEY).write_text("k\n")
    (work / "objcache" / ("0" * 40 + ".o")).write_bytes(b"object")
    (work / "sa" / "build" / "obj" / "code_000.o").write_bytes(b"object")
    return work


class FailedBuild(unittest.TestCase):
    def test_a_source_that_failed_keeps_the_translation(self):
        with tempfile.TemporaryDirectory() as d:
            work = work_folder(d)
            with contextlib.redirect_stdout(io.StringIO()) as out:
                make_game.forget_failed_build(str(work), linked=False, failed=["hos_update.c"])
            self.assertTrue((work / "sa" / "gen" / make_game.LIFT_KEY).exists())
            self.assertTrue((work / "objcache" / ("0" * 40 + ".o")).exists())
            self.assertTrue((work / "sa" / "build" / "obj" / "code_000.o").exists())
            self.assertIn("keeps the translation", out.getvalue())
            self.assertIn("hos_update.c", out.getvalue())

    def test_a_lifted_or_generated_file_that_failed_forgets_it(self):
        for name in ("code_021.c", "code_hot_3.c", "hos_image.c", "a64_rt.c"):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as d:
                work = work_folder(d)
                (work / "sa" / "gen" / "hos_image.c").write_text("")   # (the lift's own files, beside the lifted C)
                (work / "sa" / "build" / "rt64").mkdir(parents=True)
                (work / "sa" / "build" / "rt64" / "a64_rt.c").write_text("")
                with contextlib.redirect_stdout(io.StringIO()) as out:
                    make_game.forget_failed_build(str(work), linked=False, failed=["hos_update.c", name])
                self.assertFalse((work / "sa" / "gen" / make_game.LIFT_KEY).exists())
                self.assertFalse((work / "objcache").exists())
                self.assertFalse((work / "sa" / "build" / "obj").exists())
                self.assertIn("nothing of this one is reused", out.getvalue())

    def test_a_failed_link_forgets_it(self):
        with tempfile.TemporaryDirectory() as d:
            work = work_folder(d)
            with contextlib.redirect_stdout(io.StringIO()):
                make_game.forget_failed_build(str(work), linked=True, failed=[])
            self.assertFalse((work / "sa" / "gen" / make_game.LIFT_KEY).exists())
            self.assertFalse((work / "objcache").exists())

    def test_a_stopped_build_without_a_failed_line_forgets_it(self):
        with tempfile.TemporaryDirectory() as d:
            work = work_folder(d)
            with contextlib.redirect_stdout(io.StringIO()):
                make_game.forget_failed_build(str(work), linked=False)
            self.assertFalse((work / "sa" / "gen" / make_game.LIFT_KEY).exists())


class Lines(unittest.TestCase):
    def test_make_game_reads_build_pys_failed_lines(self):
        src = (ROOT / "nx2" / "make_game.py").read_text(encoding="utf-8")
        build = (ROOT / "nx2" / "build.py").read_text(encoding="utf-8")
        self.assertIn('print("FAILED %s (%d MB, %.0fs, alone too):\\n%s" % (os.path.basename(src), size, secs, err), flush=True)',
                      build)
        pattern = re.search(r're\.match\(r"(FAILED [^"]+)", line\)', src).group(1)
        line = "FAILED %s (%d MB, %.0fs, alone too):\n%s" % ("hos_update.c", 0, 0.4, "error: ...")
        self.assertEqual(re.match(pattern, line).group(1), "hos_update.c")
        self.assertIsNone(re.match(pattern, "not compiled yet: hos_update.c (0 MB, 0s), tried again alone at the end:"))

    def test_the_compile_failure_calls_it_with_the_failed_names(self):
        src = (ROOT / "nx2" / "make_game.py").read_text(encoding="utf-8")
        self.assertIn("forget_failed_build(work, bool(linked), failed)", src)
        self.assertIn('re.match(r"compiled in \\d+s, 0 failed$", line)', src)
        self.assertIn('run(PY + ["nx2/build.py", "--standalone", "--jobs", str(a.jobs)], env, compiled)', src)


if __name__ == "__main__":
    unittest.main()
