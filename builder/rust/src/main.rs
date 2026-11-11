use std::io::Read;

fn main() {
    let mut request = String::new();
    if std::io::stdin().read_to_string(&mut request).is_err() || request.trim().is_empty() {
        eprintln!("usage: hookshot-engine < request.json");
        std::process::exit(2);
    }
    let response = hookshot_engine::dispatch(request.trim());
    println!("{response}");
    if serde_json::from_str::<serde_json::Value>(&response).is_err() {
        std::process::exit(1);
    }
}
