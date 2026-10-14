fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&protocol::protocol_schema()).unwrap()
    );
}
