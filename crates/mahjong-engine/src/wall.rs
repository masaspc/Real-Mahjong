//! 山の生成と管理。
//!
//! 乱数はシードから決定的に作る。`rand` を使わないのは、クレートの版が
//! 変わっても同じシードから同じ山が出ることを保証するためである。
//! これが崩れると牌譜の再現とシードコミットメントの検算が壊れる。

use protocol::ruleset::Ruleset;
use protocol::tile::{Tile, TileKind};
use sha2::{Digest, Sha256};

/// 山を決める32バイトの種。局開始時に永続化し、対局終了後に開示する。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Seed([u8; 32]);

impl Seed {
    pub fn new(bytes: [u8; 32]) -> Self {
        Seed(bytes)
    }

    pub fn from_hex(hex: &str) -> Option<Self> {
        if hex.len() != 64 {
            return None;
        }
        let mut bytes = [0u8; 32];
        for (index, chunk) in hex.as_bytes().chunks(2).enumerate() {
            let text = std::str::from_utf8(chunk).ok()?;
            bytes[index] = u8::from_str_radix(text, 16).ok()?;
        }
        Some(Seed(bytes))
    }

    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// 局開始時に配るハッシュ。開示後にプレイヤーがこれと照合する。
    pub fn commitment(&self) -> String {
        Sha256::digest(self.0)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

/// splitmix64。実装が短く、版によって挙動が変わらない。
struct Rng(u64);

impl Rng {
    fn from_seed(seed: &Seed) -> Self {
        let mut state = 0u64;
        for (index, byte) in seed.0.iter().enumerate() {
            state ^= (*byte as u64) << ((index % 8) * 8);
            state = state.rotate_left(7);
        }
        Rng(state | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

const TOTAL: usize = 136;
/// 生牌の終わり。ここから先が王牌14枚。
const DEAD_WALL_START: usize = 122;
const MAX_REPLACEMENTS: usize = 4;
const MAX_DORA: usize = 5;

pub struct Wall {
    tiles: Vec<Tile>,
    /// 次にツモる位置。
    next: usize,
    /// ツモれる牌の終わり。嶺上を引くたび1つ手前へ下がる。
    live_end: usize,
    replacements_taken: usize,
    /// 開示済みのドラ表示牌の枚数。1〜5。
    dora_revealed: usize,
    /// ドラ表示牌5枚。位置は固定なので生成時に確定する。
    dora: Vec<Tile>,
    /// 裏ドラ表示牌5枚。
    ura: Vec<Tile>,
}

/// ドラ表示牌の位置。`live_end` に依存させない。
fn dora_position(index: usize) -> usize {
    DEAD_WALL_START + index * 2
}

fn ura_position(index: usize) -> usize {
    DEAD_WALL_START + index * 2 + 1
}

fn replacement_position(index: usize) -> usize {
    DEAD_WALL_START + MAX_DORA * 2 + index
}

impl Wall {
    pub fn new(seed: &Seed, rules: &Ruleset) -> Self {
        let mut tiles = Vec::with_capacity(TOTAL);
        for index in 0..TileKind::COUNT as u8 {
            for _ in 0..4 {
                tiles.push(Tile::from_kind(
                    TileKind::from_index(index).expect("範囲内"),
                ));
            }
        }

        // 赤ドラ。5m/5p/5s の順に、Ruleset が指定した枚数だけ置き換える。
        // 候補は3つしかないため、それより大きい値を指定しても3枚で頭打ちになる。
        let reds: [(u8, u8); 3] = [(4, 34), (13, 35), (22, 36)];
        for (kind_index, red_encoded) in reds.iter().copied().take(rules.red_dora_count as usize) {
            let position = tiles
                .iter()
                .position(|t| t.kind().index() == kind_index && !t.is_red())
                .expect("該当牌がある");
            tiles[position] = Tile::from_encoded(red_encoded).expect("赤ドラは範囲内");
        }

        let mut rng = Rng::from_seed(seed);
        for i in (1..tiles.len()).rev() {
            let j = rng.below(i + 1);
            tiles.swap(i, j);
        }

        let dora = (0..MAX_DORA).map(|i| tiles[dora_position(i)]).collect();
        let ura = (0..MAX_DORA).map(|i| tiles[ura_position(i)]).collect();

        Wall {
            tiles,
            next: 0,
            live_end: DEAD_WALL_START,
            replacements_taken: 0,
            dora_revealed: 1,
            dora,
            ura,
        }
    }

    /// 並びの検証用。**136枚すべて**を返す。牌の総数を数えるのに使わない。
    pub fn all_tiles(&self) -> impl Iterator<Item = Tile> + '_ {
        self.tiles.iter().copied()
    }

    /// いま山に残っている牌。まだ誰の手にも渡っていない牌すべて。
    ///
    /// **`live_end` ではなく `DEAD_WALL_START` まで数える。** 嶺上を引くと
    /// `live_end` が下がるが、そのとき生牌の末尾はツモれなくなるだけで
    /// 山からは消えない（王牌へ組み込まれる）。`live_end` で切ると
    /// その1枚を数え落とし、牌の総数が135枚になる。
    pub fn tiles_in_wall(&self) -> impl Iterator<Item = Tile> + '_ {
        let live = self.tiles[self.next..DEAD_WALL_START].iter().copied();
        let dead = (0..MAX_DORA * 2)
            .map(|i| self.tiles[DEAD_WALL_START + i])
            .chain(
                (self.replacements_taken..MAX_REPLACEMENTS)
                    .map(|i| self.tiles[replacement_position(i)]),
            );
        live.chain(dead)
    }

    pub fn live_remaining(&self) -> u8 {
        (self.live_end - self.next) as u8
    }

    pub fn draw(&mut self) -> Option<Tile> {
        if self.next >= self.live_end {
            return None;
        }
        let tile = self.tiles[self.next];
        self.next += 1;
        Some(tile)
    }

    /// 嶺上牌。引くたびに生牌の末尾が1枚減る。
    pub fn draw_replacement(&mut self) -> Option<Tile> {
        // 生牌を引き切っていたら live_end を下げられない。
        // 下げると live_remaining() が桁溢れする。
        if self.replacements_taken >= MAX_REPLACEMENTS || self.live_end == self.next {
            return None;
        }
        let tile = self.tiles[replacement_position(self.replacements_taken)];
        self.replacements_taken += 1;
        self.live_end -= 1;
        Some(tile)
    }

    pub fn reveal_dora(&mut self) -> Option<Tile> {
        if self.dora_revealed >= MAX_DORA {
            return None;
        }
        self.dora_revealed += 1;
        self.dora.get(self.dora_revealed - 1).copied()
    }

    pub fn dora_indicators(&self) -> &[Tile] {
        &self.dora[..self.dora_revealed]
    }

    pub fn ura_indicators(&self) -> &[Tile] {
        &self.ura[..self.dora_revealed]
    }

    pub fn dora_positions(&self) -> Vec<usize> {
        (0..self.dora_revealed).map(dora_position).collect()
    }

    pub fn ura_positions(&self) -> Vec<usize> {
        (0..self.dora_revealed).map(ura_position).collect()
    }

    pub fn replacement_positions(&self) -> Vec<usize> {
        (0..MAX_REPLACEMENTS).map(replacement_position).collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::ruleset::{MatchLength, Ruleset};
    use protocol::tile::TileKind;
    use std::collections::HashSet;

    fn rules() -> Ruleset {
        Ruleset::kin_no_ma(MatchLength::Hanchan)
    }

    fn seed(byte: u8) -> Seed {
        Seed::from_hex(&format!("{byte:02x}").repeat(32)).expect("hex")
    }

    #[test]
    fn a_wall_holds_every_tile_exactly_four_times() {
        let wall = Wall::new(&seed(1), &rules());
        let mut counts = [0u8; TileKind::COUNT];
        for tile in wall.all_tiles() {
            counts[tile.kind().index() as usize] += 1;
        }
        assert!(counts.iter().all(|c| *c == 4), "34種が4枚ずつでない");
    }

    #[test]
    fn a_wall_holds_exactly_one_hundred_and_thirty_six_tiles() {
        assert_eq!(Wall::new(&seed(1), &rules()).all_tiles().count(), 136);
    }

    #[test]
    fn exactly_three_tiles_are_red() {
        let wall = Wall::new(&seed(2), &rules());
        let mut reds: Vec<u8> = wall
            .all_tiles()
            .filter(|t| t.is_red())
            .map(|t| t.kind().index())
            .collect();
        reds.sort();
        assert_eq!(reds, vec![4, 13, 22], "赤は 5m/5p/5s の各1枚");
    }

    #[test]
    fn the_same_seed_always_produces_the_same_wall() {
        let a: Vec<u8> = Wall::new(&seed(7), &rules())
            .all_tiles()
            .map(|t| t.encoded())
            .collect();
        let b: Vec<u8> = Wall::new(&seed(7), &rules())
            .all_tiles()
            .map(|t| t.encoded())
            .collect();
        assert_eq!(a, b);
    }

    #[test]
    fn different_seeds_produce_different_walls() {
        let a: Vec<u8> = Wall::new(&seed(1), &rules())
            .all_tiles()
            .map(|t| t.encoded())
            .collect();
        let b: Vec<u8> = Wall::new(&seed(2), &rules())
            .all_tiles()
            .map(|t| t.encoded())
            .collect();
        assert_ne!(a, b);
    }

    /// シャッフル方式を変えると過去の牌譜が再現できなくなる。
    /// 固定シードに対する並びのハッシュを凍結し、変更を検出する。
    #[test]
    fn a_fixed_seed_matches_its_golden_vector() {
        use sha2::{Digest, Sha256};
        let encoded: Vec<u8> = Wall::new(&seed(0xAB), &rules())
            .all_tiles()
            .map(|t| t.encoded())
            .collect();
        assert_eq!(encoded.len(), 136);
        let digest: String = Sha256::digest(&encoded)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        // この値は計画作成時に splitmix64 と Fisher-Yates を実際に回して求めた。
        // **変更してはならない。**変わったらシャッフル方式が変わったということであり、
        // 過去の牌譜が再現できなくなる。
        assert_eq!(
            digest,
            "7b0d5f31b3ded153eeb6a5e7e06f041c14cbb4403d1c8f278b6bd37c912b43c4"
        );
    }

    #[test]
    fn one_hundred_and_twenty_two_tiles_can_be_drawn() {
        let mut wall = Wall::new(&seed(3), &rules());
        assert_eq!(wall.live_remaining(), 122);
        let mut drawn = 0;
        while wall.draw().is_some() {
            drawn += 1;
        }
        assert_eq!(drawn, 122);
        assert_eq!(wall.live_remaining(), 0);
    }

    #[test]
    fn a_replacement_draw_shortens_the_live_wall() {
        let mut wall = Wall::new(&seed(4), &rules());
        let before = wall.live_remaining();
        assert!(wall.draw_replacement().is_some());
        assert_eq!(wall.live_remaining(), before - 1);
    }

    #[test]
    fn only_four_replacements_are_available() {
        let mut wall = Wall::new(&seed(5), &rules());
        for _ in 0..4 {
            assert!(wall.draw_replacement().is_some());
        }
        assert!(wall.draw_replacement().is_none());
    }

    #[test]
    fn dora_starts_with_one_and_can_reveal_up_to_five() {
        let mut wall = Wall::new(&seed(6), &rules());
        assert_eq!(wall.dora_indicators().len(), 1);
        assert_eq!(wall.ura_indicators().len(), 1);
        for _ in 0..4 {
            assert!(wall.reveal_dora().is_some());
        }
        assert_eq!(wall.dora_indicators().len(), 5);
        assert_eq!(wall.ura_indicators().len(), 5);
        assert!(wall.reveal_dora().is_none());
    }

    /// 嶺上を引いてもドラ表示牌の位置は動かない。
    /// 動かすと既に開示した裏ドラと重複する。
    #[test]
    fn the_dead_wall_positions_never_overlap() {
        let mut wall = Wall::new(&seed(6), &rules());
        for _ in 0..4 {
            wall.draw_replacement();
            wall.reveal_dora();
        }

        let mut positions = Vec::new();
        positions.extend(wall.dora_positions());
        positions.extend(wall.ura_positions());
        positions.extend(wall.replacement_positions());

        let unique: HashSet<usize> = positions.iter().copied().collect();
        assert_eq!(
            unique.len(),
            positions.len(),
            "位置が重複した: {positions:?}"
        );
        assert_eq!(unique.len(), 14, "王牌はちょうど14枚");
        assert!(
            unique.iter().all(|p| (122..136).contains(p)),
            "王牌の範囲外"
        );
    }

    /// 嶺上を引いて山から出るのは、引いたその1枚だけである。
    /// 生牌の末尾はツモれなくなるが山には残るので、2枚減ってはいけない。
    #[test]
    fn a_replacement_draw_only_moves_one_tile_out_of_the_wall() {
        let mut wall = Wall::new(&seed(11), &rules());
        let before = wall.tiles_in_wall().count();
        assert!(wall.draw_replacement().is_some());
        assert_eq!(
            wall.tiles_in_wall().count(),
            before - 1,
            "嶺上で引いた1枚だけが山から出る"
        );
    }

    /// 配牌前の山は136枚。
    #[test]
    fn a_fresh_wall_holds_every_tile() {
        assert_eq!(Wall::new(&seed(12), &rules()).tiles_in_wall().count(), 136);
    }

    /// 嶺上を引く前後でドラ表示牌そのものが変わらない。
    #[test]
    fn a_replacement_draw_does_not_change_the_revealed_dora() {
        let mut wall = Wall::new(&seed(8), &rules());
        let before: Vec<u8> = wall.dora_indicators().iter().map(|t| t.encoded()).collect();
        wall.draw_replacement();
        let after: Vec<u8> = wall.dora_indicators().iter().map(|t| t.encoded()).collect();
        assert_eq!(before, after);
    }

    #[test]
    fn a_seed_commits_to_itself() {
        let s = seed(9);
        assert_eq!(s.commitment().len(), 64, "SHA-256 の hex は64文字");
        assert_eq!(
            Seed::from_hex(&s.to_hex()).unwrap().commitment(),
            s.commitment()
        );
        assert_ne!(seed(10).commitment(), s.commitment());
    }

    #[test]
    fn malformed_hex_is_rejected() {
        assert!(Seed::from_hex("00").is_none(), "長さが足りない");
        assert!(Seed::from_hex(&"zz".repeat(32)).is_none(), "16進でない");
    }
}
