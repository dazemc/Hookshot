//! Interrupt polls in lifted code: `A64_POLL(c);` before every backward
//! goto, so a guest fiber's loop gives its core back. Ported from nx2/poll.py.

use regex::Regex;
use serde_json::{Value, json};

/// Add a poll before every backward goto (the loop's back-edge).
/// Returns the text and how many polls were added.
pub fn add_polls(text: &str) -> (String, usize) {
    let label = Regex::new(r"(?m)^L_([0-9a-f]{8}):;").unwrap();
    let goto = Regex::new(r"goto L_([0-9a-f]{8});").unwrap();
    let labels: std::collections::HashMap<String, usize> = label
        .captures_iter(text)
        .map(|cap| (cap[1].to_owned(), cap.get(0).unwrap().end()))
        .collect();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut count = 0;
    for found in goto.find_iter(text) {
        let cap = goto.captures(found.as_str()).unwrap();
        let Some(at) = labels.get(&cap[1]) else {
            continue;
        };
        if *at > found.start() {
            continue;
        }
        out.push_str(&text[last..found.start()]);
        out.push_str("{ A64_POLL(c); ");
        out.push_str(found.as_str());
        out.push_str(" }");
        last = found.end();
        count += 1;
    }
    if count == 0 {
        return (text.to_owned(), 0);
    }
    out.push_str(&text[last..]);
    (out, count)
}

pub fn json(text: &str) -> Value {
    let (lifted, polls) = add_polls(text);
    json!({"text": lifted, "polls": polls})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polls_only_backward_gotos() {
        let text = "L_80001000:;\n x = 1;\n goto L_80001000;\n goto L_80002000;\nL_80002000:;\n";
        let (out, n) = add_polls(text);
        assert_eq!(n, 1);
        assert!(out.contains("{ A64_POLL(c); goto L_80001000; }"));
        assert!(out.contains("goto L_80002000;"));
    }

    #[test]
    fn leaves_a_loopless_function_alone() {
        let text = "void f_1(Ctx *c) { goto L_80003000; }\nL_80003000:;\n";
        let (out, n) = add_polls(text);
        assert_eq!(n, 0);
        assert_eq!(out, text);
    }
}
