//! One HTTP client for every provider call. `Client::new()` per request built
//! a fresh connection pool and did a new TCP + TLS handshake each time, with
//! no bound on how long a dead host could hold the connect open.

use std::sync::LazyLock;
use std::time::Duration;

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .tcp_keepalive(Duration::from_secs(30))
        .pool_idle_timeout(Duration::from_secs(90))
        .build()
        .unwrap_or_default()
});

/// The shared client (cheap to clone: it is a handle onto one pool).
pub fn client() -> reqwest::Client {
    CLIENT.clone()
}
