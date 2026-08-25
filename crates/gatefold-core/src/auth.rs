use librespot::core::SessionConfig;

pub const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";

pub const SCOPES: &[&str] = &["streaming", "user-read-email", "user-read-private"];

pub fn client_id() -> String {
    SessionConfig::default().client_id
}
