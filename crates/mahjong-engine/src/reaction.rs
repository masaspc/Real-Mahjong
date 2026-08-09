//! 打牌と槓宣言に対する反応の受付。
//!
//! 早期確定の条件は「現在の最高優先度**以上**を出せる未応答者がいなければ確定」。
//! 「より上」にするとダブロンが原理的に成立しなくなる（仕様 6.4）。

use protocol::command::{ActionOption, CallResponse};
use protocol::seat::Seat;
use protocol::tile::Tile;

/// 応答の優先度。宣言順がそのまま順序になる。
/// **明槓はポンと同順位**なので、専用の値を作らず Pon へ写す。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Priority {
    Pass,
    Chi,
    Pon,
    Ron,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowKind {
    /// 打牌への反応。チー・ポン・明槓・ロンを受け付ける。
    Discard,
    /// 槓宣言への反応。ロンだけを受け付ける。
    Chankan,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rejection {
    /// その席に候補を提示していない。
    NotACandidate,
    AlreadyResponded,
    /// 提示していない種類の応答。
    NotOffered,
    /// 打牌者自身は応答できない。
    IsTheDiscarder,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Outcome {
    Pending,
    /// ロンした席を席順で返す。3人なら三家和として呼び出し側が流局にする。
    Ron(Vec<Seat>),
    Call {
        seat: Seat,
        response: CallResponse,
    },
    PassAll,
}

fn priority_of_response(response: &CallResponse) -> Priority {
    match response {
        CallResponse::Pass => Priority::Pass,
        CallResponse::Chi { .. } => Priority::Chi,
        // 明槓はポンと同順位。
        CallResponse::Pon { .. } | CallResponse::Kan => Priority::Pon,
        CallResponse::Ron => Priority::Ron,
    }
}

fn priority_of_option(option: &ActionOption) -> Priority {
    match option {
        ActionOption::Chi { .. } => Priority::Chi,
        ActionOption::Pon { .. } | ActionOption::Kan { .. } => Priority::Pon,
        ActionOption::Ron => Priority::Ron,
        _ => Priority::Pass,
    }
}

pub struct ReactionWindow {
    id: u32,
    kind: WindowKind,
    from: Seat,
    tile: Tile,
    candidates: [Vec<ActionOption>; 4],
    responses: [Option<CallResponse>; 4],
    opened_at_ms: u64,
    deadline_ms: u64,
}

impl ReactionWindow {
    pub fn open(
        id: u32,
        kind: WindowKind,
        from: Seat,
        tile: Tile,
        candidates: [Vec<ActionOption>; 4],
        opened_at_ms: u64,
        deadline_ms: u64,
    ) -> Self {
        // 槍槓のウィンドウはロンしか受け付けない。
        // debug_assert で呼び出し側の契約にすると、release では素通りし、
        // debug ではテストが検証したい経路より先に落ちる。
        // **ここで落として不変条件を構造で保証する。**
        let candidates = if kind == WindowKind::Chankan {
            candidates.map(|options| {
                options
                    .into_iter()
                    .filter(|o| matches!(o, ActionOption::Ron))
                    .collect()
            })
        } else {
            candidates
        };

        ReactionWindow {
            id,
            kind,
            from,
            tile,
            candidates,
            responses: [None, None, None, None],
            opened_at_ms,
            deadline_ms,
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn kind(&self) -> WindowKind {
        self.kind
    }

    pub fn tile(&self) -> Tile {
        self.tile
    }

    pub fn from(&self) -> Seat {
        self.from
    }

    pub fn respond(&mut self, seat: Seat, response: CallResponse) -> Result<(), Rejection> {
        if seat == self.from {
            return Err(Rejection::IsTheDiscarder);
        }
        let offered = &self.candidates[seat.index()];
        if offered.is_empty() {
            return Err(Rejection::NotACandidate);
        }
        if self.responses[seat.index()].is_some() {
            return Err(Rejection::AlreadyResponded);
        }
        // パスは候補を持つ席なら常に許す。
        // 槍槓でロン以外が弾かれるのは、open が候補から落としているためである。
        if response != CallResponse::Pass {
            let wanted = priority_of_response(&response);
            let offered_here = offered.iter().any(|o| priority_of_option(o) == wanted);
            if !offered_here {
                return Err(Rejection::NotOffered);
            }
        }
        self.responses[seat.index()] = Some(response);
        Ok(())
    }

    /// 状態を変えずに現在の結論を返す。同じ入力からは同じ答えが出る。
    pub fn resolve(&self, now_ms: u64, min_wait_ms: u32) -> Outcome {
        // 全員が答えていても最低待機の前は確定しない。
        // 鳴ける者がいない局面と間の長さを揃え、情報を漏らさないため。
        if now_ms < self.opened_at_ms + min_wait_ms as u64 {
            return Outcome::Pending;
        }
        let expired = now_ms > self.deadline_ms;

        let best = Seat::ALL
            .iter()
            .filter_map(|s| self.responses[s.index()].as_ref())
            .map(priority_of_response)
            .max()
            .unwrap_or(Priority::Pass);

        if !expired {
            // 未応答者が best 以上を出せるなら待つ。「より上」ではない。
            let someone_could_match = Seat::ALL.iter().any(|s| {
                self.responses[s.index()].is_none()
                    && self.candidates[s.index()]
                        .iter()
                        .any(|o| priority_of_option(o) >= best)
            });
            if someone_could_match {
                return Outcome::Pending;
            }
        }

        if best == Priority::Ron {
            let rons: Vec<Seat> = Seat::ALL
                .iter()
                .copied()
                .filter(|s| self.responses[s.index()] == Some(CallResponse::Ron))
                .collect();
            return Outcome::Ron(rons);
        }

        for seat in Seat::ALL {
            if let Some(response) = self.responses[seat.index()] {
                if priority_of_response(&response) == best && best != Priority::Pass {
                    return Outcome::Call { seat, response };
                }
            }
        }
        Outcome::PassAll
    }

    /// ロン以外で同じ優先度の応答が2つ以上ある席。
    ///
    /// 牌は1種4枚しかないため起こりえない。起きたら呼び出し側が落とす。
    /// **優先度ごとに数える。**最初に見たものとだけ比べると、
    /// チー→ポン→ポン の順で来たときに2つ目のポンを見落とす。
    pub fn non_ron_ties(&self) -> Vec<Seat> {
        let mut ties = Vec::new();
        for target in [Priority::Chi, Priority::Pon] {
            let matching: Vec<Seat> = Seat::ALL
                .iter()
                .copied()
                .filter(|s| {
                    self.responses[s.index()].as_ref().map(priority_of_response) == Some(target)
                })
                .collect();
            if matching.len() >= 2 {
                ties.extend(matching);
            }
        }
        ties
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::command::{ActionOption, CallResponse};
    use protocol::notation::parse_tile;
    use protocol::seat::Seat;

    const MIN_WAIT: u32 = 350;

    fn ron_only() -> Vec<ActionOption> {
        vec![ActionOption::Ron]
    }

    fn pon_only() -> Vec<ActionOption> {
        vec![ActionOption::Pon { candidates: vec![] }]
    }

    fn chi_only() -> Vec<ActionOption> {
        vec![ActionOption::Chi { candidates: vec![] }]
    }

    fn window(candidates: [Vec<ActionOption>; 4]) -> ReactionWindow {
        ReactionWindow::open(
            1,
            WindowKind::Discard,
            Seat::new(0),
            parse_tile("3p").unwrap(),
            candidates,
            0,
            5_000,
        )
    }

    fn pon_response() -> CallResponse {
        CallResponse::Pon {
            tiles: [parse_tile("3p").unwrap(); 2],
        }
    }

    fn chi_response() -> CallResponse {
        CallResponse::Chi {
            tiles: [parse_tile("2p").unwrap(), parse_tile("4p").unwrap()],
        }
    }

    /// 全員が答えても最低待機の前は確定しない。
    /// 鳴ける者がいない局面と間の長さを揃え、情報を漏らさないため。
    #[test]
    fn nothing_resolves_before_the_minimum_wait() {
        let mut w = window([vec![], vec![], chi_only(), vec![]]);
        w.respond(Seat::new(2), CallResponse::Pass).unwrap();
        assert_eq!(w.resolve(349, MIN_WAIT), Outcome::Pending);
        assert_eq!(w.resolve(350, MIN_WAIT), Outcome::PassAll);
    }

    #[test]
    fn a_window_with_no_candidates_passes_after_the_wait() {
        let w = window([vec![], vec![], vec![], vec![]]);
        assert_eq!(w.resolve(349, MIN_WAIT), Outcome::Pending);
        assert_eq!(w.resolve(350, MIN_WAIT), Outcome::PassAll);
    }

    /// ポンが確定すれば、チーしか出せない未応答者は待たない。
    #[test]
    fn a_pon_resolves_without_waiting_for_a_chi_candidate() {
        let mut w = window([vec![], pon_only(), vec![], chi_only()]);
        w.respond(Seat::new(1), pon_response()).unwrap();
        match w.resolve(400, MIN_WAIT) {
            Outcome::Call { seat, .. } => assert_eq!(seat, Seat::new(1)),
            other => panic!("ポンで確定するはず: {other:?}"),
        }
    }

    /// チーが答えても、ポンできる未応答者がいれば待つ。
    #[test]
    fn a_chi_waits_for_a_pending_pon_candidate() {
        let mut w = window([vec![], pon_only(), vec![], chi_only()]);
        w.respond(Seat::new(3), chi_response()).unwrap();
        assert_eq!(w.resolve(400, MIN_WAIT), Outcome::Pending);
    }

    /// ポンがパスすれば、チーが確定する。
    #[test]
    fn a_chi_resolves_once_the_pon_candidate_passes() {
        let mut w = window([vec![], pon_only(), vec![], chi_only()]);
        w.respond(Seat::new(3), chi_response()).unwrap();
        w.respond(Seat::new(1), CallResponse::Pass).unwrap();
        match w.resolve(400, MIN_WAIT) {
            Outcome::Call { seat, .. } => assert_eq!(seat, Seat::new(3)),
            other => panic!("チーで確定するはず: {other:?}"),
        }
    }

    /// ロンが1つ確定しても、ロン可能な未応答者がいれば待つ。
    /// ここを「より上」にするとダブロンが原理的に成立しなくなる。
    #[test]
    fn a_ron_waits_for_other_ron_candidates() {
        let mut w = window([vec![], ron_only(), ron_only(), vec![]]);
        w.respond(Seat::new(1), CallResponse::Ron).unwrap();
        assert_eq!(w.resolve(400, MIN_WAIT), Outcome::Pending);

        w.respond(Seat::new(2), CallResponse::Ron).unwrap();
        assert_eq!(
            w.resolve(400, MIN_WAIT),
            Outcome::Ron(vec![Seat::new(1), Seat::new(2)])
        );
    }

    /// もう一方がパスすれば、単独のロンで確定する。
    #[test]
    fn a_single_ron_resolves_once_the_other_candidate_passes() {
        let mut w = window([vec![], ron_only(), ron_only(), vec![]]);
        w.respond(Seat::new(1), CallResponse::Ron).unwrap();
        w.respond(Seat::new(2), CallResponse::Pass).unwrap();
        assert_eq!(w.resolve(400, MIN_WAIT), Outcome::Ron(vec![Seat::new(1)]));
    }

    /// 3人がロンすれば全員を返す。三家和にするかは呼び出し側が決める。
    #[test]
    fn three_rons_are_all_reported() {
        let mut w = window([vec![], ron_only(), ron_only(), ron_only()]);
        for seat in [1u8, 2, 3] {
            w.respond(Seat::new(seat), CallResponse::Ron).unwrap();
        }
        assert_eq!(
            w.resolve(400, MIN_WAIT),
            Outcome::Ron(vec![Seat::new(1), Seat::new(2), Seat::new(3)])
        );
    }

    /// ロンは席順で返す。ダブロンの供託と本場の割り当てが決定的になる。
    #[test]
    fn rons_are_reported_in_seat_order() {
        let mut w = window([vec![], ron_only(), ron_only(), ron_only()]);
        for seat in [3u8, 1, 2] {
            w.respond(Seat::new(seat), CallResponse::Ron).unwrap();
        }
        assert_eq!(
            w.resolve(400, MIN_WAIT),
            Outcome::Ron(vec![Seat::new(1), Seat::new(2), Seat::new(3)])
        );
    }

    #[test]
    fn the_deadline_turns_silence_into_a_pass() {
        let w = window([vec![], pon_only(), vec![], vec![]]);
        assert_eq!(w.resolve(400, MIN_WAIT), Outcome::Pending);
        assert_eq!(w.resolve(5_001, MIN_WAIT), Outcome::PassAll);
    }

    /// 締切を過ぎても、既に答えた鳴きは有効である。
    #[test]
    fn the_deadline_keeps_an_answer_that_already_arrived() {
        let mut w = window([vec![], pon_only(), vec![], chi_only()]);
        w.respond(Seat::new(1), pon_response()).unwrap();
        match w.resolve(5_001, MIN_WAIT) {
            Outcome::Call { seat, .. } => assert_eq!(seat, Seat::new(1)),
            other => panic!("ポンが残るはず: {other:?}"),
        }
    }

    #[test]
    fn a_seat_without_candidates_cannot_respond() {
        let mut w = window([vec![], pon_only(), vec![], vec![]]);
        assert!(matches!(
            w.respond(Seat::new(2), CallResponse::Ron),
            Err(Rejection::NotACandidate)
        ));
    }

    #[test]
    fn the_discarder_cannot_respond_to_their_own_discard() {
        let mut w = window([ron_only(), vec![], vec![], vec![]]);
        assert!(matches!(
            w.respond(Seat::new(0), CallResponse::Ron),
            Err(Rejection::IsTheDiscarder)
        ));
    }

    #[test]
    fn responding_twice_is_rejected() {
        let mut w = window([vec![], pon_only(), vec![], vec![]]);
        w.respond(Seat::new(1), CallResponse::Pass).unwrap();
        assert!(matches!(
            w.respond(Seat::new(1), CallResponse::Pass),
            Err(Rejection::AlreadyResponded)
        ));
    }

    #[test]
    fn a_response_outside_the_offered_options_is_rejected() {
        let mut w = window([vec![], chi_only(), vec![], vec![]]);
        assert!(matches!(
            w.respond(Seat::new(1), CallResponse::Ron),
            Err(Rejection::NotOffered)
        ));
    }

    /// パスは候補を持つ席なら常に許す。
    #[test]
    fn passing_is_always_allowed_for_a_candidate() {
        let mut w = window([vec![], chi_only(), vec![], vec![]]);
        assert!(w.respond(Seat::new(1), CallResponse::Pass).is_ok());
    }

    /// 槍槓のウィンドウはロンだけを受け付ける。
    #[test]
    fn a_chankan_window_only_offers_ron() {
        let w = ReactionWindow::open(
            2,
            WindowKind::Chankan,
            Seat::new(0),
            parse_tile("5s").unwrap(),
            [vec![], ron_only(), vec![], vec![]],
            0,
            5_000,
        );
        assert_eq!(w.kind(), WindowKind::Chankan);
    }

    /// 明槓はポンと同順位。チーしか出せない席を待たずに確定する。
    #[test]
    fn a_minkan_has_the_same_priority_as_a_pon() {
        let kan_only = vec![ActionOption::Kan { candidates: vec![] }];
        let mut w = window([vec![], kan_only, vec![], chi_only()]);
        w.respond(Seat::new(1), CallResponse::Kan).unwrap();
        match w.resolve(400, MIN_WAIT) {
            Outcome::Call { seat, .. } => assert_eq!(seat, Seat::new(1)),
            other => panic!("明槓で確定するはず: {other:?}"),
        }
    }

    /// チーが答えても、明槓できる未応答者がいれば待つ。
    #[test]
    fn a_chi_waits_for_a_pending_minkan_candidate() {
        let kan_only = vec![ActionOption::Kan { candidates: vec![] }];
        let mut w = window([vec![], kan_only, vec![], chi_only()]);
        w.respond(Seat::new(3), chi_response()).unwrap();
        assert_eq!(w.resolve(400, MIN_WAIT), Outcome::Pending);
    }

    /// 槍槓のウィンドウはロン以外を受け付けない。
    ///
    /// **候補にロンしか無いから拒否される、では検査になっていない。**
    /// ポンも載せて `open` へ渡し、それが落とされた結果として
    /// ポンの応答が通らないことを見る。
    #[test]
    fn a_chankan_window_drops_non_ron_candidates() {
        let mut offered = ron_only();
        offered.push(ActionOption::Pon { candidates: vec![] });
        let mut w = ReactionWindow::open(
            2,
            WindowKind::Chankan,
            Seat::new(0),
            parse_tile("5s").unwrap(),
            [vec![], offered, vec![], vec![]],
            0,
            5_000,
        );
        assert!(
            matches!(
                w.respond(Seat::new(1), pon_response()),
                Err(Rejection::NotOffered)
            ),
            "槍槓でポンを受理した"
        );
        assert!(w.respond(Seat::new(1), CallResponse::Ron).is_ok());
    }

    /// 打牌のウィンドウなら同じ候補でポンが通る。
    /// 上のテストが「候補が無いから落ちた」のではないことを示す。
    #[test]
    fn the_same_options_allow_a_pon_on_a_discard_window() {
        let mut offered = ron_only();
        offered.push(ActionOption::Pon { candidates: vec![] });
        let mut w = window([vec![], offered, vec![], vec![]]);
        assert!(w.respond(Seat::new(1), pon_response()).is_ok());
    }

    /// 同順位が3件並んでも検出できる。最初に見たものとだけ比べると
    /// チー→ポン→ポン の順で2つ目のポンを見落とす。
    #[test]
    fn non_ron_ties_are_detected_regardless_of_order() {
        let mut w = window([vec![], pon_only(), pon_only(), chi_only()]);
        w.respond(Seat::new(3), chi_response()).unwrap();
        w.respond(Seat::new(1), pon_response()).unwrap();
        w.respond(Seat::new(2), pon_response()).unwrap();
        let ties = w.non_ron_ties();
        assert_eq!(ties.len(), 2, "ポンの競合2件を検出するはず: {ties:?}");
        assert!(ties.contains(&Seat::new(1)) && ties.contains(&Seat::new(2)));
    }

    /// 非ロンの同順位は牌の枚数上ありえない。
    /// 2人がポンするには各自2枚＋捨て牌1枚で5枚必要だが、牌は1種4枚しかない。
    /// ポンと明槓の競合も6枚必要で成立しない。席順ロジックは書かず検査で守る。
    #[test]
    fn non_ron_ties_never_occur() {
        let mut w = window([vec![], pon_only(), vec![], chi_only()]);
        w.respond(Seat::new(1), pon_response()).unwrap();
        w.respond(Seat::new(3), chi_response()).unwrap();
        assert!(w.non_ron_ties().is_empty(), "同順位の非ロン競合が現れた");
    }
}
