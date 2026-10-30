pub mod aggregate;
pub mod model;
pub mod redact;
pub mod rules;
pub mod store;
pub fn now() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.
}

pub mod migration;
