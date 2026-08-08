use serde::{Deserialize, Serialize};

/// 卓上の絶対的な席位置（0..=3）。自風は局と席から導出する。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Seat(u8);

impl Seat {
    pub const ALL: [Seat; 4] = [Seat(0), Seat(1), Seat(2), Seat(3)];

    pub fn new(index: u8) -> Self {
        assert!(index < 4, "席は 0..=3 のみ有効（{index} が渡された）");
        Seat(index)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// 下家（次にツモる席）。
    pub fn next(self) -> Seat {
        Seat((self.0 + 1) % 4)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Wind {
    East,
    South,
    West,
    North,
}

/// 東1局なら `Round { wind: Wind::East, number: 1 }`。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Round {
    pub wind: Wind,
    pub number: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_wraps_around_the_table() {
        assert_eq!(Seat::new(0).next(), Seat::new(1));
        assert_eq!(Seat::new(3).next(), Seat::new(0));
    }

    #[test]
    fn all_lists_every_seat_once() {
        let indices: Vec<usize> = Seat::ALL.iter().map(|s| s.index()).collect();
        assert_eq!(indices, vec![0, 1, 2, 3]);
    }

    #[test]
    #[should_panic]
    fn rejects_out_of_range_seats() {
        let _ = Seat::new(4);
    }
}
