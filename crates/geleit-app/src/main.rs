//! `geleit-app` — application entrypoint.
//!
//! Scaffold placeholder (slice S0.1). The native Slint UI shell (ADR-0001) is added with
//! the UI spike in slice S0.3; for now this is a minimal binary that exercises the
//! `app → engine → core` dependency direction.

use geleit_engine::can_use_account;

fn main() {
    let demo = "user@example.com";
    println!("geleit scaffold — {demo} usable: {}", can_use_account(demo));
}

#[cfg(test)]
mod tests {
    use geleit_engine::can_use_account;

    #[test]
    fn app_reaches_engine() {
        assert!(can_use_account("user@example.com"));
    }
}
