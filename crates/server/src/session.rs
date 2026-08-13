//! 1卓 = 1 tokio task の Actor。**唯一 I/O と時間を持つ層。**

#[path = "session_time.rs"]
mod time;

pub use time::{Clock, SeedSource};
