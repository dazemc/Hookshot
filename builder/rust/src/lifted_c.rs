//! Where the lifted functions begin in the lift's C (code_*.c): each definition
//! is one line, `void f_<address>(Ctx *__restrict A64_NOESC c) {`. Ported
//! from nx2/lifted_c.py.

pub const DEFINITION: &str = "void f_";
pub const DEFINITION_END: &str = "(Ctx *__restrict A64_NOESC c) {";

/// The address of a definition line, if this is one.
pub fn definition(line: &str) -> Option<&str> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let rest = line.strip_prefix(DEFINITION)?;
    let (address, tail) = rest.split_once(DEFINITION_END)?;
    if tail != "" {
        return None;
    }
    if address.is_empty() || !address.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(address)
}

/// Where each definition starts, for split_hot's pieces.
pub struct Span<'a> {
    pub address: &'a str,
    pub start: usize,
    pub end: usize,
}

pub fn spans(text: &str) -> Vec<Span<'_>> {
    let mut starts = Vec::new();
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let clean = line.trim_end_matches(['\n', '\r']);
        if let Some(address) = definition(clean) {
            starts.push((address, at));
        }
        at += line.len();
    }
    starts
        .iter()
        .enumerate()
        .map(|(i, (address, start))| Span {
            address,
            start: *start,
            end: starts
                .get(i + 1)
                .map(|(_, next)| *next)
                .unwrap_or(text.len()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_cover_each_definition() {
        let text = "preamble\nvoid f_80001234(Ctx *__restrict A64_NOESC c) {\n  x();\n}\nvoid f_80005678(Ctx *__restrict A64_NOESC c) {\n}\n";
        let found = spans(text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].address, "80001234");
        assert!(text[found[0].start..found[0].end].contains("x();"));
        assert_eq!(
            text[found[1].start..found[1].end]
                .matches("void f_")
                .count(),
            1
        );
    }

    #[test]
    fn reads_definition_lines() {
        let text = "/* a note */\nvoid f_80001234(Ctx *__restrict A64_NOESC c) {\n  x();\n}\nvoid f_80005678(Ctx *__restrict A64_NOESC c) {\n}\n";
        let found = definitions(text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].0, "80001234");
        assert_eq!(found[0].1, Some("/* a note */"));
        assert_eq!(found[1].0, "80005678");
        assert_eq!(found[1].1, None);
        assert_eq!(
            definition("void f_xyz(Ctx *__restrict A64_NOESC c) {"),
            None
        );
    }
}
