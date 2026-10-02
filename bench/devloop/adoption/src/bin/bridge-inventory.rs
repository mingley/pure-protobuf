fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&pbrs_adoption_corpus::bridge::inventory()).unwrap()
    );
}
