#[path = "../tests/support/mod.rs"]
mod support;

fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&support::all_opening_probes())
            .expect("opening probes must serialize")
    );
}
