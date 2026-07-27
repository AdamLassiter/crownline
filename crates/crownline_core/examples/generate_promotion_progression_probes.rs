#[path = "../tests/support/promotion.rs"]
mod support;

fn main() {
    let rendered = serde_json::to_string_pretty(&support::all_promotion_progression_probes())
        .expect("promotion progression probes must serialize");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/playtests/automated-promotion-progression.json");
    std::fs::write(&path, format!("{rendered}\n")).expect("probe archive must be writable");
    println!("wrote {}", path.display());
}
