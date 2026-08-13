//! 卓 Actor に実時間と乱数を与える。
//!
//! **`std::time` を使ってはならない。**`tokio::time::Instant` だけが
//! `#[tokio::test(start_paused = true)]` で仮想化される。ここを取り違えると
//! テストが実時間で数十秒かかり、しかも結果が揺れる。

use mahjong_engine::wall::Seed;
use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};

/// 卓が生まれた瞬間からのミリ秒。
///
/// エンジンは `now_ms: u64` しか受け取らない。壁時計ではなく単調増加の
/// 経過時間を渡すことで、システム時刻が飛んでも局が壊れない。
pub struct Clock {
    origin: tokio::time::Instant,
}

impl Clock {
    pub fn start() -> Self {
        Clock {
            origin: tokio::time::Instant::now(),
        }
    }

    pub fn now_ms(&self) -> u64 {
        (tokio::time::Instant::now() - self.origin).as_millis() as u64
    }
}

/// 局のシードを繰り出す。
///
/// **卓ぜんぶが1本の 32 バイトから再現できる。**局ごとに OS 乱数を引くと
/// 牌譜を再生できなくなる。
pub struct SeedSource {
    rng: StdRng,
}

impl SeedSource {
    pub fn from_master(master: [u8; 32]) -> Self {
        SeedSource {
            rng: StdRng::from_seed(master),
        }
    }

    pub fn from_os() -> Self {
        let mut master = [0u8; 32];
        rand::rng().fill_bytes(&mut master);
        SeedSource::from_master(master)
    }

    /// clippy が `Iterator::next` と紛らわしいと言うので `next_seed`。
    pub fn next_seed(&mut self) -> Seed {
        let mut bytes = [0u8; 32];
        self.rng.fill_bytes(&mut bytes);
        Seed::new(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test(start_paused = true)]
    async fn the_clock_starts_at_zero() {
        let clock = Clock::start();
        assert_eq!(clock.now_ms(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn the_clock_follows_virtual_time() {
        let clock = Clock::start();
        tokio::time::sleep(Duration::from_millis(2_500)).await;
        assert_eq!(clock.now_ms(), 2_500);
    }

    #[test]
    fn the_same_master_gives_the_same_seeds() {
        let mut a = SeedSource::from_master([7u8; 32]);
        let mut b = SeedSource::from_master([7u8; 32]);
        for _ in 0..4 {
            assert_eq!(a.next_seed().to_hex(), b.next_seed().to_hex());
        }
    }

    #[test]
    fn a_different_master_gives_different_seeds() {
        let mut a = SeedSource::from_master([7u8; 32]);
        let mut b = SeedSource::from_master([8u8; 32]);
        assert_ne!(a.next_seed().to_hex(), b.next_seed().to_hex());
    }

    #[test]
    fn successive_seeds_differ() {
        let mut source = SeedSource::from_master([1u8; 32]);
        let first = source.next_seed().to_hex();
        let second = source.next_seed().to_hex();
        let third = source.next_seed().to_hex();
        assert_ne!(first, second);
        assert_ne!(second, third);
        assert_ne!(first, third);
    }

    #[test]
    fn a_seed_is_thirty_two_bytes() {
        let mut source = SeedSource::from_master([1u8; 32]);
        assert_eq!(source.next_seed().to_hex().len(), 64);
    }

    #[test]
    fn two_tables_from_the_operating_system_differ() {
        let mut a = SeedSource::from_os();
        let mut b = SeedSource::from_os();
        assert_ne!(a.next_seed().to_hex(), b.next_seed().to_hex());
    }
}
