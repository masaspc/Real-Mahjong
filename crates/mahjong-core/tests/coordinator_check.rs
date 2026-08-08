//! コーディネータによる独立検証。
//!
//! エージェントが書いたテストとは別に、大量の手で不変条件を確かめる。
//! 完全にランダムな13枚はまずテンパイにならないため、**和了形を構成的に組み立て**、
//! そこから崩すことで意味のある検査量を確保する。

use mahjong_core::hand::HandCounts;
use mahjong_core::shanten::overall;
use mahjong_core::wait::waiting_tiles;
use protocol::notation::to_notation;
use protocol::tile::{Tile, TileKind};

/// 決定的な擬似乱数。外部クレートを足さずに再現可能な手を作る。
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

fn kind(index: u8) -> TileKind {
    TileKind::from_index(index).expect("範囲内")
}

/// 4面子1雀頭の和了形をランダムに組み立てる。1種4枚の制限を守る。
fn build_winning_hand(rng: &mut Rng) -> Option<HandCounts> {
    let mut counts = HandCounts::new();
    let take = |counts: &mut HandCounts, k: TileKind, n: u8| -> bool {
        if counts.get(k) + n > 4 {
            return false;
        }
        for _ in 0..n {
            counts.add(k);
        }
        true
    };

    for _ in 0..4 {
        let mut placed = false;
        for _ in 0..40 {
            if rng.below(2) == 0 {
                // 刻子
                let k = kind(rng.below(TileKind::COUNT) as u8);
                if take(&mut counts, k, 3) {
                    placed = true;
                    break;
                }
            } else {
                // 順子（数牌のみ、色をまたがない）
                let suit = rng.below(3) as u8;
                let start = rng.below(7) as u8;
                let base = suit * 9 + start;
                let run = [kind(base), kind(base + 1), kind(base + 2)];
                if run.iter().all(|k| counts.get(*k) < 4) {
                    for k in run {
                        counts.add(k);
                    }
                    placed = true;
                    break;
                }
            }
        }
        if !placed {
            return None;
        }
    }

    // 雀頭
    for _ in 0..40 {
        let k = kind(rng.below(TileKind::COUNT) as u8);
        if take(&mut counts, k, 2) {
            return Some(counts);
        }
    }
    None
}

fn as_tiles(counts: &HandCounts) -> Vec<Tile> {
    let mut out = Vec::new();
    for (k, n) in counts.kinds() {
        for _ in 0..n {
            out.push(Tile::from_kind(k));
        }
    }
    out
}

#[test]
fn constructed_winning_hands_are_recognised_as_complete() {
    let mut rng = Rng(0x2026_0808);
    let mut checked = 0;
    for _ in 0..4_000 {
        let Some(counts) = build_winning_hand(&mut rng) else {
            continue;
        };
        assert_eq!(counts.total(), 14, "組み立てた手が14枚でない");
        checked += 1;
        assert!(
            overall::is_complete(&counts, 0),
            "組み立てた和了形が和了と判定されない: {}",
            to_notation(&as_tiles(&counts))
        );
    }
    assert!(checked > 1_000, "検査量が足りない: {checked} 件");
    println!("和了形を {checked} 件検査した");
}

/// 和了形から1枚抜けばテンパイになり、抜いた牌が待ちに含まれる。
/// 向聴計算と待ち計算が食い違っていればここで落ちる。
#[test]
fn removing_one_tile_yields_tenpai_waiting_on_it() {
    let mut rng = Rng(0xDEAD_BEEF);
    let mut checked = 0;
    for _ in 0..3_000 {
        let Some(counts) = build_winning_hand(&mut rng) else {
            continue;
        };
        let present: Vec<TileKind> = counts.kinds().map(|(k, _)| k).collect();
        let removed = present[rng.below(present.len())];

        let mut hand = counts;
        assert!(hand.remove(removed));
        checked += 1;

        assert!(
            overall::is_tenpai(&hand, 0),
            "和了形から1枚抜いたのにテンパイでない: {} - index{}",
            to_notation(&as_tiles(&hand)),
            removed.index()
        );

        let waits = waiting_tiles(&hand, 0);
        assert!(
            waits.contains(&removed),
            "抜いた牌が待ちに含まれない: {} 待ち={:?} 抜いた={}",
            to_notation(&as_tiles(&hand)),
            waits.iter().map(|k| k.index()).collect::<Vec<_>>(),
            removed.index()
        );

        // 返された待ちはすべて実際に和了になる。
        for wait in waits {
            let mut probe = hand;
            probe.add(wait);
            assert!(
                overall::is_complete(&probe, 0),
                "待ち牌を足しても和了にならない: {} + index{}",
                to_notation(&as_tiles(&hand)),
                wait.index()
            );
        }
    }
    assert!(checked > 1_000, "検査量が足りない: {checked} 件");
    println!("テンパイを {checked} 件検査した");
}

/// 自分で4枚使い切っている牌は待ちに含めない。
#[test]
fn waits_never_include_a_tile_the_hand_already_holds_four_of() {
    let mut rng = Rng(0xC0FFEE);
    let mut checked = 0;
    for _ in 0..3_000 {
        let Some(counts) = build_winning_hand(&mut rng) else {
            continue;
        };
        let present: Vec<TileKind> = counts.kinds().map(|(k, _)| k).collect();
        let removed = present[rng.below(present.len())];
        let mut hand = counts;
        hand.remove(removed);
        checked += 1;

        for wait in waiting_tiles(&hand, 0) {
            assert!(
                hand.get(wait) < 4,
                "4枚持っている牌を待ちとして返した: {} index{}",
                to_notation(&as_tiles(&hand)),
                wait.index()
            );
        }
    }
    println!("待ちの妥当性を {checked} 件検査した");
}

/// 13枚の手の向聴は必ず 0..=8 に収まる。
#[test]
fn shanten_stays_within_its_declared_range() {
    let mut rng = Rng(0x1234_5678);
    let mut wall: Vec<Tile> = Vec::new();
    for index in 0..TileKind::COUNT as u8 {
        for _ in 0..4 {
            wall.push(Tile::from_kind(kind(index)));
        }
    }

    for _ in 0..3_000 {
        for i in (1..wall.len()).rev() {
            let j = rng.below(i + 1);
            wall.swap(i, j);
        }
        let counts = HandCounts::from_tiles(&wall[..13]);
        let value = overall::shanten(&counts, 0);
        assert!(
            (0..=8).contains(&value),
            "13枚の手の向聴が範囲外: {value}（{}）",
            to_notation(&wall[..13])
        );
    }
}
