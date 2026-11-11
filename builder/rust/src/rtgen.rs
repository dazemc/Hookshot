//! a64-kit's guest model with a 64-bit guest window, written next to the
//! lifted code. Ported from nx2/rtgen.py: the substitutions are exact, so a
//! refreshed a64-kit that moved a line fails loudly instead of silently
//! keeping the 32-bit window.

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

const POLL_DECL: &str = r#"
/* (nx2/poll.py) loop back-edge interrupt polls: A64_POLLING builds clear c->poll and call a64_poll_cur, the backend's,
   when it is set. The call is handed nothing on purpose: the context parameter is noescape (A64_NOESC) and never
   captured, so a call with no pointer argument cannot touch *c, and the compiler keeps guest registers in host registers
   across the loop. (a64_poll(c), the old form, made every loop body reload and store them: a call that takes c may change
   any of it.) The poll sits before the backward goto, after the body's stores: at the loop header, a call C cannot mark
   as returning (LLVM's willreturn) left the stores after it not guaranteed to execute, and nothing was promoted.
   a64_poll_cur finds the thread's context itself (host/mkw_glue.c, hos_ctx). A64_POLL_CTX keeps the old form for the
   emulator-hosted DLL (nx2/rt/eden_shim.c), whose host's poll takes the context. */
void a64_poll(Ctx *A64_NOESC c);
void a64_poll_cur(void);
#if defined(A64_POLLING) && defined(A64_POLL_CTX)
#define A64_POLL(c) do { if (__builtin_expect(__atomic_load_n(&(c)->poll, __ATOMIC_RELAXED) != 0, 0)) a64_poll(c); } while (0)
#elif defined(A64_POLLING)
#define A64_POLL(c) do { if (__builtin_expect(__atomic_load_n(&(c)->poll, __ATOMIC_RELAXED) != 0, 0)) { \
    __atomic_store_n(&(c)->poll, 0, __ATOMIC_RELAXED); a64_poll_cur(); } } while (0)
#else
#define A64_POLL(c) ((void)0)
#endif"#;
const ADDR64: &str = r#"#ifndef A64_ADDR64
#define A64_ADDR64 1   /* (nx2/rtgen.py: 64-bit guest window) */
#endif
#if A64_ADDR64
#define A64_PTR(a) (A64_MEM + (uint64_t)(a))
#define A64_DIRTY_PAGES (1ull << 27)   /* (nx2/rtgen.py) A64_WRITE_TRACK: a mark per 4 KB page of the 512 GB window */
#else
#define A64_DIRTY_PAGES (1ull << 20)
#define A64_PTR(a) (A64_MEM + (uint32_t)(a))
#endif"#;
const RET_MISMATCH: &str = r#"void a64_ret_mismatch(Ctx *A64_NOESC c, uint64_t pc);         /* RET to something other than the caller (debug builds) */"#;
const THREAD: &str = r#"    void *thread;               /* the OS layer's thread object */
} GuestContext;"#;
const LOOKUP: &str = r#"    uint32_t a = (uint32_t)addr, lo = 0, hi = a64_fn_count;"#;
const MARK_NATIVE: &str = r#"extern uint8_t a64_dirty[1u << 20];
/* (Tried 2026-10-21: reading a mark first and writing it only when clear, against cache lines moving between cores:
   game threads +1.1 ms a frame, the code 18 MB bigger. Not kept.) */
#define A64_MARK(a, n) (__atomic_store_n(&a64_dirty[(uint32_t)(a) >> 12], 1, __ATOMIC_RELEASE), \
                        __atomic_store_n(&a64_dirty[(uint32_t)((a) + (n) - 1) >> 12], 1, __ATOMIC_RELEASE))"#;
const MARK_NATIVE_64: &str = r#"extern uint8_t a64_dirty[A64_DIRTY_PAGES];
/* (Tried 2026-10-21: reading a mark first and writing it only when clear, against cache lines moving between cores:
   game threads +1.1 ms a frame, the code 18 MB bigger. Not kept.) */
#define A64_DIRTY_AT(a) (((uint64_t)(a) >> 12) & (A64_DIRTY_PAGES - 1))
#define A64_MARK(a, n) (__atomic_store_n(&a64_dirty[A64_DIRTY_AT(a)], 1, __ATOMIC_RELEASE), \
                        __atomic_store_n(&a64_dirty[A64_DIRTY_AT((a) + (n) - 1)], 1, __ATOMIC_RELEASE))"#;

fn substitutions() -> Vec<(&'static str, Vec<(String, String)>)> {
    let thread = THREAD.replace(
        "\n} GuestContext;",
        "\n    uint32_t poll;              /* (nx2/rtgen.py) interrupt request: A64_POLL clears it and calls a64_poll_cur when set */\n} GuestContext;",
    );
    vec![
        (
            "a64.h",
            vec![
                (THREAD.to_owned(), thread),
                (
                    RET_MISMATCH.to_owned(),
                    format!("{RET_MISMATCH}{POLL_DECL}"),
                ),
                (
                    "#define A64_PTR(a) (A64_MEM + (uint32_t)(a))".to_owned(),
                    ADDR64.to_owned(),
                ),
                (MARK_NATIVE.to_owned(), MARK_NATIVE_64.to_owned()),
            ],
        ),
        (
            "a64_rt.c",
            vec![
                (
                    LOOKUP.to_owned(),
                    format!(
                        "#if A64_ADDR64\n    if (addr >> 32) return NULL;   /* (code lies below 4 GB: never a truncated match) */\n#endif\n{LOOKUP}"
                    ),
                ),
                (
                    "uint8_t a64_dirty[1u << 20];".to_owned(),
                    "uint8_t a64_dirty[A64_DIRTY_PAGES];".to_owned(),
                ),
            ],
        ),
    ]
}

/// Rewrite the vendored runtime into `out_dir`.
pub fn write(out_dir: &Path, kit: &Path) -> Result<Value, String> {
    fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    for (name, edits) in substitutions() {
        let source = kit.join("rt").join(name);
        let mut text =
            fs::read_to_string(&source).map_err(|e| format!("{}: {e}", source.display()))?;
        for (old, new) in edits {
            let count = text.matches(&old).count();
            if count != 1 {
                return Err(format!(
                    "rtgen: {:?} found {count} times in a64-kit rt/{name} (expected once): update the Rust rtgen",
                    &old[..old.len().min(60)]
                ));
            }
            text = text.replacen(&old, &new, 1);
        }
        let head = format!(
            "/* Generated by mkw-recomp nx2/rtgen.py from a64-kit rt/{name} (64-bit guest window). Do not edit. */\n"
        );
        let path = out_dir.join(name);
        fs::write(&path, format!("{head}{text}"))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(json!({"out": out_dir.to_string_lossy()}))
}

pub fn write_json(out: &str, kit: &str) -> Result<Value, String> {
    write(Path::new(out), Path::new(kit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_the_64bit_runtime() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let out = std::env::temp_dir().join(format!("hookshot-rust-rt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&out);
        write(&out, &root.join("third_party/a64-kit")).unwrap();
        let header = fs::read_to_string(out.join("a64.h")).unwrap();
        assert!(header.contains("A64_ADDR64"));
        assert!(header.contains("uint32_t poll;"));
        let runtime = fs::read_to_string(out.join("a64_rt.c")).unwrap();
        assert!(runtime.contains("a64_dirty[A64_DIRTY_PAGES]"));
        assert!(runtime.contains("if (addr >> 32) return NULL;"));
        let _ = fs::remove_dir_all(out);
    }

    #[test]
    fn refuses_a_runtime_that_moved() {
        let root =
            std::env::temp_dir().join(format!("hookshot-rust-rt-bad-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("rt")).unwrap();
        fs::write(root.join("rt/a64.h"), "nothing here").unwrap();
        fs::write(root.join("rt/a64_rt.c"), "nothing here").unwrap();
        assert!(write(&root.join("out"), &root).is_err());
        let _ = fs::remove_dir_all(root);
    }
}
