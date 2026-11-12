"""The lift is skipped only when the game, the tools and the settings are the ones it was made from.

python -I -B tests/test_lift_cache.py
"""
import importlib.util
import os
import tempfile
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("make_game_under_test", ROOT / "nx2" / "make_game.py")
make_game = importlib.util.module_from_spec(spec)
spec.loader.exec_module(make_game)


def check(what, ok):
    if not ok:
        raise SystemExit("FAIL: " + what)


def main():
    with tempfile.TemporaryDirectory(prefix="hookshot-lift-key-") as d:
        d = Path(d)
        exefs, work = d / "exefs", d / "work"
        (exefs).mkdir()
        (work / "image").mkdir(parents=True)
        (exefs / "main").write_bytes(b"NSO0 one")
        (work / "hot_funcs.txt").write_text("f_80000000\n")
        (work / "image" / "chunks.txt").write_text("80001000\n")
        env = {"A64_HOT_CHUNK": "1", "OOT_WORK": str(work)}
        with mock.patch.dict(os.environ, {k: v for k, v in os.environ.items() if not k.startswith(("A64_", "OOT_", "RX_"))}, clear=True):
            key = make_game.lift_key(str(exefs), str(work), env)
            check("the same inputs give the same key", key == make_game.lift_key(str(exefs), str(work), dict(env)))
            (exefs / "main").write_bytes(b"NSO0 two")
            check("another game module changes the key", make_game.lift_key(str(exefs), str(work), env) != key)
            (exefs / "main").write_bytes(b"NSO0 one")
            (work / "hot_funcs.txt").write_text("f_80000004\n")
            check("another hot list changes the key", make_game.lift_key(str(exefs), str(work), env) != key)
            (work / "hot_funcs.txt").write_text("f_80000000\n")
            (work / "image" / "chunks.txt").write_text("80002000\n")
            check("other chunk boundaries change the key", make_game.lift_key(str(exefs), str(work), env) != key)
            (work / "image" / "chunks.txt").write_text("80001000\n")
            check("another lift setting changes the key", make_game.lift_key(str(exefs), str(work), dict(env, A64_HOT_CHUNK="")) != key)
            with mock.patch.dict(os.environ, {"A64_KEEP_FLAGS": "1"}):
                check("a lifter switch in the environment changes the key", make_game.lift_key(str(exefs), str(work), env) != key)
            real_open = open
            changed = str(ROOT / "nx2" / "lift.py")

            def edited(path, *a, **k):   # (the translator as if one of its files had changed)
                f = real_open(path, *a, **k)
                if os.path.abspath(path) == changed:
                    f.close()
                    import io
                    return io.BytesIO(b"# an edit\n")
                return f
            with mock.patch("builtins.open", edited):
                check("a change to the translator changes the key", make_game.lift_key(str(exefs), str(work), env) != key)
            check("the inputs back as they were give the key again", make_game.lift_key(str(exefs), str(work), env) == key)
            import standalone
            names = standalone.implemented()
            with mock.patch.object(standalone, "implemented", lambda host=None: names | {"imp_new_hle"}):
                check("a new HLE function in host/ changes the key", make_game.lift_key(str(exefs), str(work), env) != key)
            host_c = str(ROOT / "host" / "hos_main.c")

            def host_edited(path, *a, **k):   # (host/ code changed, its HLE functions the same)
                if os.path.abspath(path) == host_c:
                    import io
                    return io.StringIO("/* an edit */\n")
                return real_open(path, *a, **k)
            with mock.patch("builtins.open", host_edited):
                check("other host/ code does not change the key", make_game.lift_key(str(exefs), str(work), env) == key)

        gen, image = work / "sa" / "gen", work / "sa" / "image"
        check("no lift yet: not lifted before", not make_game.lifted_before(str(work), key))
        gen.mkdir(parents=True)
        image.mkdir(parents=True)
        make_game.write_lift_key(str(work), key)
        check("a key without the lift's files is not trusted", not make_game.lifted_before(str(work), key))
        (gen / "a64_table.c").write_text("")
        (image / "image.bin").write_bytes(b"")
        check("a key written before the lift's files is not trusted", not make_game.lifted_before(str(work), key))
        make_game.write_lift_key(str(work), key)
        check("the finished lift with this key is reused", make_game.lifted_before(str(work), key))
        (gen / "code_000.c").write_text("/* another builder's lift */")
        check("C changed by another builder's lift is not", not make_game.lifted_before(str(work), key))
        (gen / "code_000.c").unlink()
        check("a key in the old one-line form is not", (gen / make_game.LIFT_KEY).write_text(key + "\n") and
              not make_game.lifted_before(str(work), key))
        make_game.write_lift_key(str(work), key)
        check("a finished lift with another key is not", not make_game.lifted_before(str(work), "0" * 40))
    check_key_after_lift()
    print("ok: the lift key follows the game, the tools, the hot list, the chunks and the settings")


def check_key_after_lift():
    """A fresh work folder has no image/chunks.txt until its first lift's chunks.write saves one, and the key hashes it: the key
    the lift is stored under is taken after the lift, from what the next build reads, or that build translates the game again
    (a no-op rebuild of a new work folder: 319 s on Windows, 290 s of it the lift). make_game.py's lift block, read as text"""
    src = (ROOT / "nx2" / "make_game.py").read_text(encoding="utf-8")
    start = src.index("    key = lift_key(exefs, work, env)")
    body = src[start:src.index("\n    if LINUX:\n        check_linux_compiler()", start)]
    lift = body.index('"nx2/lift.py"')
    split = body.index('"nx2/split_hot.py"')
    inputs = body.find("inputs = lift_key(exefs, work, env, planning=False)")
    after = body.find("after = lift_key(exefs, work, env)", split)
    guard = body.find("if lift_key(exefs, work, env, planning=False) == inputs:", split)
    taken = body.find("key = after", guard)
    write = body.find("write_lift_key(work, key)", split)
    check("what the lift reads and does not write is taken as it starts", 0 <= inputs < lift)
    check("the key is taken again after the lift and split_hot, then written", split < after < guard < taken < write)
    check("the key is taken before that comparison, so an edit between the two is caught by it", after < guard)
    with tempfile.TemporaryDirectory(prefix="hookshot-lift-key-after-") as d:
        d = Path(d)
        exefs, work = d / "exefs", d / "work"
        exefs.mkdir()
        (work / "image").mkdir(parents=True)
        (exefs / "main").write_bytes(b"NSO0 one")
        env = {"A64_HOT_CHUNK": "1"}
        with mock.patch.dict(os.environ, {k: v for k, v in os.environ.items() if not k.startswith(("A64_", "OOT_", "RX_"))}, clear=True):
            before = make_game.lift_key(str(exefs), str(work), env)
            (work / "image" / "chunks.txt").write_text("80003000\n")   # (what the first lift's chunks.write saves)
            after = make_game.lift_key(str(exefs), str(work), env)
            check("a fresh folder's key before its first lift is not the next build's", before != after)
            check("the key after the lift is the next build's", after == make_game.lift_key(str(exefs), str(work), dict(env)))
            inputs = make_game.lift_key(str(exefs), str(work), env, planning=False)
            (work / "image" / "chunks.txt").write_text("80004000\n")
            check("the lift's own file is not among the other inputs",
                  make_game.lift_key(str(exefs), str(work), env, planning=False) == inputs)
            (exefs / "main").write_bytes(b"NSO0 two")
            check("the game's code is", make_game.lift_key(str(exefs), str(work), env, planning=False) != inputs)
            (exefs / "main").write_bytes(b"NSO0 one")
            (work / "hot_funcs.txt").write_text("f_80000004\n")
            check("the hot list is", make_game.lift_key(str(exefs), str(work), env, planning=False) != inputs)


if __name__ == "__main__":
    main()
