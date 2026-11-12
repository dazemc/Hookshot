use std::io::{self, Read, Write};

fn main() {
    let stream = std::env::args().skip(1).any(|arg| arg == "--stream");
    let mut request = String::new();
    if io::stdin().read_to_string(&mut request).is_err() || request.trim().is_empty() {
        eprintln!("usage: hookshot_engine < request.json");
        eprintln!("       hookshot_engine --stream < request.json");
        std::process::exit(2);
    }
    if stream {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        let code = hookshot_engine::run_stream(request.trim(), &mut out);
        let _ = out.flush();
        std::process::exit(code);
    }
    let response = hookshot_engine::dispatch(request.trim());
    println!("{response}");
    if serde_json::from_str::<serde_json::Value>(&response).is_err() {
        std::process::exit(1);
    }
}
