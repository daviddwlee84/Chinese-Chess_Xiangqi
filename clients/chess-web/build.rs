fn main() {
    println!("cargo:rerun-if-env-changed=CHESS_WEB_HOSTING");
    println!("cargo:rerun-if-env-changed=CHESS_WEB_BASE_PATH");

    let hosting = std::env::var("CHESS_WEB_HOSTING").unwrap_or_else(|_| "server".to_string());
    let hosting = match hosting.as_str() {
        "static" => "static",
        _ => "server",
    };

    let raw_base = std::env::var("CHESS_WEB_BASE_PATH").unwrap_or_default();
    let base = normalize_base_path(&raw_base);

    println!("cargo:rustc-env=CHESS_WEB_HOSTING={hosting}");
    println!("cargo:rustc-env=CHESS_WEB_BASE_PATH={base}");
}

fn normalize_base_path(raw: &str) -> String {
    let trimmed = raw.trim().trim_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("/{trimmed}")
    }
}
