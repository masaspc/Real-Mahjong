//! 部屋と卓の台帳。
//!
//! **卓を立てる前に「誰がどの席か」を決める層である。**`Occupant` は
//! `spawn` の時点で固まるので、人を2人以上座らせるにはその手前に
//! 待合が要る。設計は
//! `docs/superpowers/specs/2026-08-30-rooms-and-seating-design.md`。
//!
//! 席の証明はサーバが配るトークンが持つ。**卓の id を知っているだけでは
//! 座れない。**ここが緩いと、席ごとの視界フィルタが意味を失う——どの席と
//! して繋ぐかを自称できるなら、他人の手牌を要求できてしまう。

use crate::persistence::{hash_token, MatchHead, Scribe, SeatRow};
use crate::session::{spawn_recorded, Clock, Recording, SeedSource, TableHandle};
use crate::table::Occupant;
use protocol::event::PlayerId;
use protocol::ruleset::{MatchLength, Ruleset};
use protocol::seat::Seat;
use rand::seq::SliceRandom;
use rand::RngCore;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 部屋を指す合言葉。**口で伝えられる長さにする。**
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Code(pub String);

/// 席の証明。**これを持っている者だけがその席に座れる。**
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Token(pub String);

/// 合言葉に使う字。
///
/// **`0`/`O` と `1`/`I` を外している。**電話越しや口頭で伝えることを
/// 前提にしているので、聞き分けられない字は入れない。32字あるので
/// 6桁で約10億通りになり、総当たりで他人の部屋を引き当てるには足りない。
const ALPHABET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";

const CODE_LEN: usize = 6;

/// 名前の長さの上限。待合の枠と点棒の行に収まる長さ。
const NAME_MAX: usize = 12;

/// 名無しの人に付ける名。
const ANONYMOUS: &str = "プレイヤー";

pub fn new_code() -> Code {
    let mut bytes = [0u8; CODE_LEN];
    rand::rng().fill_bytes(&mut bytes);
    Code(
        bytes
            .iter()
            .map(|byte| ALPHABET[*byte as usize % ALPHABET.len()] as char)
            .collect(),
    )
}

pub fn new_token() -> Token {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    let mut text = String::with_capacity(32);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    Token(text)
}

/// 人が入れた名前を整える。
///
/// **他人が入力した文字列がそのまま画面と `PlayerId` に載る。**改行や
/// 制御文字が混じると待合の枠が崩れ、長すぎる名前は点棒の行を押し出す。
/// 拒んで入力し直させるほどのことではないので、黙って整える。
pub fn clean_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(NAME_MAX)
        .collect();
    if cleaned.is_empty() {
        ANONYMOUS.to_owned()
    } else {
        cleaned
    }
}

/// 1つの卓に座れる人の数。残りは CPU が埋める。
const SEATS: usize = 4;

/// 「接続中」と見なす猶予。待合の枠に印を出すのに使う。
const PRESENT_MS: u64 = 10_000;

/// 部屋主が消えたと見なすまで。
///
/// **これが無いと部屋が詰む。**開始を押せるのは部屋主だけなので、
/// 部屋主がタブを閉じたまま戻らないと、残りの3人は何もできない。
/// 30秒黙っていたら、待合にいる誰でも押せることにする。
const HOST_GONE_MS: u64 = 30_000;

/// 放置された待合を捨てるまで。
const IDLE_MS: u64 = 30 * 60 * 1_000;

/// 待合にいる人。
#[derive(Clone, Debug)]
struct Member {
    name: String,
    token: Token,
    /// 部屋を作った人。開始を押せる。
    host: bool,
    /// 最後に待合を覗いた時刻。「接続中」の判定と部屋主の生死に使う。
    seen_ms: u64,
    /// その人の browser を指す鍵。牌譜の一覧を引くのに使う。
    ///
    /// **アカウントが入るまでの繋ぎである。**トークンは部屋ごとなので
    /// 1本＝1対局にしかならず、「自分の打った半荘」を並べられない。
    /// 送ってこない画面もありうるので `Option` にしてある。
    player_key: Option<String>,
}

/// 部屋の居場所。
enum RoomState {
    /// 人が集まっている。卓はまだ無い。
    Waiting,
    /// 卓が立っている。`seats[i]` は `members[i]` の席。
    Playing {
        handle: TableHandle,
        seats: Vec<Seat>,
        /// 牌譜の対局 id。残していなければ `None`。
        record_id: Option<String>,
    },
}

struct Room {
    members: Vec<Member>,
    state: RoomState,
    /// 最後に何かが起きた時刻。放置された部屋を掃くのに使う。
    touched_ms: u64,
}

impl Room {
    fn find(&self, token: &Token) -> Option<usize> {
        self.members.iter().position(|m| &m.token == token)
    }

    /// 開始を押せるか。部屋主か、部屋主が黙って久しいか。
    fn can_start(&self, index: usize, now_ms: u64) -> bool {
        if !matches!(self.state, RoomState::Waiting) {
            return false;
        }
        if self.members[index].host {
            return true;
        }
        self.members
            .iter()
            .find(|m| m.host)
            .is_none_or(|host| now_ms.saturating_sub(host.seen_ms) >= HOST_GONE_MS)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum JoinError {
    NoSuchRoom,
    Full,
    AlreadyStarted,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StartError {
    BadToken,
    NotHost,
    AlreadyStarted,
}

impl JoinError {
    /// 画面が分岐に使う名前。**人に見せる文ではない。**
    pub fn slug(&self) -> &'static str {
        match self {
            JoinError::NoSuchRoom => "no_such_room",
            JoinError::Full => "room_full",
            JoinError::AlreadyStarted => "already_started",
        }
    }
}

impl StartError {
    pub fn slug(&self) -> &'static str {
        match self {
            StartError::BadToken => "bad_token",
            StartError::NotHost => "not_host",
            StartError::AlreadyStarted => "already_started",
        }
    }
}

/// 待合の一人ぶんの見え方。
#[derive(Debug, Clone, serde::Serialize)]
pub struct MemberView {
    pub name: String,
    pub host: bool,
    /// 最近この待合を覗いたか。
    pub present: bool,
}

/// 待合の様子。**席は載せない。**開始まで決まらず、決まった席は
/// `MatchStart` が運ぶ。
#[derive(Debug, Clone, serde::Serialize)]
pub struct Lobby {
    pub code: String,
    /// `"waiting"` か `"playing"`。
    pub state: &'static str,
    pub you: MemberView,
    pub members: Vec<MemberView>,
    pub can_start: bool,
}

/// 壁時計のミリ秒。**牌譜の見出しに入れる時刻だけはこれを使う。**
///
/// 卓の中で使う `Clock` は卓が生まれてからの経過であって、いつ打ったかを
/// 表さない。一覧を新しい順に並べるには実時刻が要る。
fn wall_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 席の配り方を決める。
///
/// **開始を押した瞬間に混ぜる。**入室順に配ると部屋主が必ず起家になる。
/// 実際の座決めと同じく、誰がどこに座るかは運で決める。
fn deal_seats(members: usize) -> Vec<Seat> {
    let mut order: Vec<u8> = (0..SEATS as u8).collect();
    order.shuffle(&mut rand::rng());
    order.into_iter().take(members).map(Seat::new).collect()
}

struct Ledger {
    rooms: HashMap<Code, Room>,
    /// トークンから部屋を引く索引。
    ///
    /// **部屋を総なめしない。**トークンは接続のたびに引かれるので、
    /// 部屋数に比例する探索を挟むと卓が増えるほど接続が重くなる。
    by_token: HashMap<Token, Code>,
}

/// 動いている部屋の台帳。
#[derive(Clone)]
pub struct Rooms {
    inner: Arc<Mutex<Ledger>>,
    clock: Arc<Clock>,
    /// 牌譜の書き手。無ければ牌譜を残さない。
    scribe: Option<Scribe>,
}

impl Default for Rooms {
    fn default() -> Self {
        Rooms::new()
    }
}

impl Rooms {
    pub fn new() -> Self {
        Rooms::with_scribe(None)
    }

    /// 牌譜を残す台帳。
    pub fn with_scribe(scribe: Option<Scribe>) -> Self {
        Rooms {
            inner: Arc::new(Mutex::new(Ledger {
                rooms: HashMap::new(),
                by_token: HashMap::new(),
            })),
            clock: Arc::new(Clock::start()),
            scribe,
        }
    }

    /// いまの時刻。HTTP の口はこれを各メソッドへ渡す。
    pub fn now_ms(&self) -> u64 {
        self.clock.now_ms()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("毒されていない").rooms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 部屋を作る。作った人が部屋主になり、そのまま入室する。
    pub fn create(&self, name: &str, player_key: Option<&str>, now_ms: u64) -> (Code, Token) {
        let mut ledger = self.inner.lock().expect("毒されていない");
        // 10億通りに対して衝突はまず起きないが、起きたときに他人の部屋を
        // 上書きするわけにはいかないので引き直す。
        let code = loop {
            let candidate = new_code();
            if !ledger.rooms.contains_key(&candidate) {
                break candidate;
            }
        };
        let token = new_token();
        ledger.rooms.insert(
            code.clone(),
            Room {
                members: vec![Member {
                    name: clean_name(name),
                    token: token.clone(),
                    host: true,
                    seen_ms: now_ms,
                    player_key: player_key.map(str::to_owned),
                }],
                state: RoomState::Waiting,
                touched_ms: now_ms,
            },
        );
        ledger.by_token.insert(token.clone(), code.clone());
        (code, token)
    }

    /// 部屋に入る。
    pub fn join(
        &self,
        code: &Code,
        name: &str,
        player_key: Option<&str>,
        now_ms: u64,
    ) -> Result<Token, JoinError> {
        let mut ledger = self.inner.lock().expect("毒されていない");
        let room = ledger.rooms.get_mut(code).ok_or(JoinError::NoSuchRoom)?;
        // **満室より先に開始済みを見る。**始まった部屋に来た人へ「満室」と
        // 言うと、待てば入れるように聞こえる。
        if !matches!(room.state, RoomState::Waiting) {
            return Err(JoinError::AlreadyStarted);
        }
        if room.members.len() >= SEATS {
            return Err(JoinError::Full);
        }
        let token = new_token();
        room.members.push(Member {
            name: clean_name(name),
            token: token.clone(),
            host: false,
            seen_ms: now_ms,
            player_key: player_key.map(str::to_owned),
        });
        room.touched_ms = now_ms;
        ledger.by_token.insert(token.clone(), code.clone());
        Ok(token)
    }

    /// 待合を覗く。**覗いたこと自体が「まだいる」の合図になる。**
    pub fn look(&self, token: &Token, now_ms: u64) -> Option<Lobby> {
        let mut ledger = self.inner.lock().expect("毒されていない");
        let code = ledger.by_token.get(token)?.clone();
        let room = ledger.rooms.get_mut(&code)?;
        let index = room.find(token)?;
        room.members[index].seen_ms = now_ms;
        room.touched_ms = now_ms;

        let view = |m: &Member| MemberView {
            name: m.name.clone(),
            host: m.host,
            present: now_ms.saturating_sub(m.seen_ms) < PRESENT_MS,
        };
        Some(Lobby {
            code: code.0.clone(),
            state: match room.state {
                RoomState::Waiting => "waiting",
                RoomState::Playing { .. } => "playing",
            },
            you: view(&room.members[index]),
            members: room.members.iter().map(view).collect(),
            can_start: room.can_start(index, now_ms),
        })
    }

    /// 卓を立てる。席を配り、空席を CPU が埋める。
    pub fn start(&self, token: &Token, now_ms: u64) -> Result<(), StartError> {
        let mut ledger = self.inner.lock().expect("毒されていない");
        let code = ledger
            .by_token
            .get(token)
            .ok_or(StartError::BadToken)?
            .clone();
        let room = ledger.rooms.get_mut(&code).ok_or(StartError::BadToken)?;
        let index = room.find(token).ok_or(StartError::BadToken)?;
        if !matches!(room.state, RoomState::Waiting) {
            return Err(StartError::AlreadyStarted);
        }
        if !room.can_start(index, now_ms) {
            return Err(StartError::NotHost);
        }

        let seats = deal_seats(room.members.len());
        // 人を先に置き、残りへ CPU を流し込む。
        let mut occupants: [Option<Occupant>; SEATS] = [None, None, None, None];
        for (member, seat) in room.members.iter().zip(seats.iter()) {
            occupants[seat.index()] = Some(Occupant::Human(PlayerId(member.name.clone())));
        }
        let mut cpu = 0;
        let occupants = occupants.map(|slot| {
            slot.unwrap_or_else(|| {
                cpu += 1;
                Occupant::Cpu(PlayerId(format!("CPU{cpu}")))
            })
        });

        let rules = Ruleset::kin_no_ma(MatchLength::Hanchan);

        // 牌譜の見出しを先に立てる。**席と名前と証明を知っているのは
        // ここだけである。**卓は自分が誰に配っているかを知らない。
        let recording = self.scribe.as_ref().map(|scribe| {
            let record_id = new_token().0;
            let players: Vec<String> = occupants
                .iter()
                .map(|occupant| occupant.player_id().0)
                .collect();
            let rows: Vec<SeatRow> = occupants
                .iter()
                .enumerate()
                .map(|(index, occupant)| {
                    // その席に座っている人を探す。CPU なら見つからない。
                    let who = room
                        .members
                        .iter()
                        .zip(seats.iter())
                        .find(|(_, seat)| seat.index() == index)
                        .map(|(member, _)| member);
                    SeatRow {
                        seat: index as u8,
                        name: occupant.player_id().0,
                        is_cpu: who.is_none(),
                        // **CPU の席には証明も鍵も入れない。**入れると
                        // CPU の席として牌譜を引ける道ができる。
                        token_hash: who.map(|member| hash_token(&member.token.0)),
                        player_key: who.and_then(|member| member.player_key.clone()),
                    }
                })
                .collect();
            scribe.begin(
                MatchHead {
                    id: record_id.clone(),
                    rules_json: serde_json::to_string(&rules).unwrap_or_else(|_| "{}".to_owned()),
                    started_ms: wall_clock_ms(),
                    ended_ms: None,
                    players,
                    result_json: None,
                },
                rows,
            );
            Recording {
                scribe: scribe.clone(),
                record_id,
            }
        });

        let (handle, _actor) =
            spawn_recorded(rules, occupants, SeedSource::from_os(), recording.clone());
        room.state = RoomState::Playing {
            handle,
            seats,
            record_id: recording.map(|r| r.record_id),
        };
        room.touched_ms = now_ms;
        Ok(())
    }

    /// トークンの指す卓と席。**始まっていなければ何も返さない。**
    ///
    /// ここが席の唯一の決め手である。クライアントは席を名乗れない。
    pub fn seat_of(&self, token: &Token) -> Option<(TableHandle, Seat)> {
        let ledger = self.inner.lock().expect("毒されていない");
        let code = ledger.by_token.get(token)?;
        let room = ledger.rooms.get(code)?;
        let index = room.find(token)?;
        match &room.state {
            RoomState::Waiting => None,
            RoomState::Playing { handle, seats, .. } => Some((handle.clone(), *seats.get(index)?)),
        }
    }

    /// トークンの指す牌譜と席。
    ///
    /// **卓が生きているあいだの引き当てにしか使えない。**部屋が掃かれた
    /// 後は、倉に残った `record_seats` の側から引く。だからこそ証明の
    /// ハッシュを倉にも入れてある。
    pub fn record_of(&self, token: &Token) -> Option<(String, Seat)> {
        let ledger = self.inner.lock().expect("毒されていない");
        let code = ledger.by_token.get(token)?;
        let room = ledger.rooms.get(code)?;
        let index = room.find(token)?;
        match &room.state {
            RoomState::Waiting => None,
            RoomState::Playing {
                seats, record_id, ..
            } => Some((record_id.clone()?, *seats.get(index)?)),
        }
    }

    /// 終わった卓と、放置された待合を捨てる。捨てた数を返す。
    pub fn sweep(&self, now_ms: u64) -> usize {
        let mut ledger = self.inner.lock().expect("毒されていない");
        let before = ledger.rooms.len();
        let mut dropped: Vec<Token> = Vec::new();
        ledger.rooms.retain(|_, room| {
            let keep = match &room.state {
                RoomState::Playing { handle, .. } => !handle.is_closed(),
                RoomState::Waiting => now_ms.saturating_sub(room.touched_ms) < IDLE_MS,
            };
            if !keep {
                dropped.extend(room.members.iter().map(|m| m.token.clone()));
            }
            keep
        });
        // **索引も一緒に消す。**残すとトークンが宙に浮き、台帳は空なのに
        // 索引だけが際限なく膨らむ。
        for token in dropped {
            ledger.by_token.remove(&token);
        }
        before - ledger.rooms.len()
    }
}

#[cfg(test)]
mod ledger_tests {
    use super::*;
    use protocol::client_event::ClientEvent;
    use std::collections::HashSet;

    #[tokio::test(start_paused = true)]
    async fn the_one_who_makes_the_room_is_the_host() {
        let rooms = Rooms::new();
        let (_, token) = rooms.create("まさ", None, 0);
        let lobby = rooms.look(&token, 0).expect("覗ける");
        assert!(lobby.you.host, "作った人が部屋主になっていない");
        assert_eq!(lobby.you.name, "まさ");
        assert_eq!(lobby.members.len(), 1);
        assert_eq!(lobby.state, "waiting");
    }

    #[tokio::test(start_paused = true)]
    async fn a_guest_is_not_the_host() {
        let rooms = Rooms::new();
        let (code, _) = rooms.create("まさ", None, 0);
        let guest = rooms.join(&code, "たろう", None, 0).expect("入れる");
        let lobby = rooms.look(&guest, 0).expect("覗ける");
        assert!(!lobby.you.host);
        assert_eq!(lobby.members.iter().filter(|m| m.host).count(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_fifth_person_is_turned_away() {
        let rooms = Rooms::new();
        let (code, _) = rooms.create("1", None, 0);
        for name in ["2", "3", "4"] {
            rooms.join(&code, name, None, 0).expect("4人までは入れる");
        }
        assert_eq!(rooms.join(&code, "5", None, 0), Err(JoinError::Full));
    }

    #[tokio::test(start_paused = true)]
    async fn an_unknown_code_is_not_a_room() {
        let rooms = Rooms::new();
        assert_eq!(
            rooms.join(&Code("ZZZZZZ".to_owned()), "まさ", None, 0),
            Err(JoinError::NoSuchRoom)
        );
    }

    /// **始まった部屋に「満室」と答えない。**待てば入れるように聞こえる。
    #[tokio::test(start_paused = true)]
    async fn joining_a_started_room_says_so() {
        let rooms = Rooms::new();
        let (code, host) = rooms.create("まさ", None, 0);
        rooms.start(&host, 0).expect("部屋主は始められる");
        assert_eq!(
            rooms.join(&code, "たろう", None, 0),
            Err(JoinError::AlreadyStarted)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_unknown_token_sees_nothing() {
        let rooms = Rooms::new();
        rooms.create("まさ", None, 0);
        assert!(rooms.look(&Token("dead".to_owned()), 0).is_none());
        assert!(rooms.seat_of(&Token("dead".to_owned())).is_none());
        assert_eq!(
            rooms.start(&Token("dead".to_owned()), 0),
            Err(StartError::BadToken)
        );
    }

    /// 覗いた本人だけが「接続中」になる。
    #[tokio::test(start_paused = true)]
    async fn presence_follows_who_is_looking() {
        let rooms = Rooms::new();
        let (code, host) = rooms.create("まさ", None, 0);
        let guest = rooms.join(&code, "たろう", None, 0).expect("入れる");

        // 11秒後、部屋主だけが覗く。
        let lobby = rooms.look(&host, 11_000).expect("覗ける");
        assert!(lobby.members[0].present, "覗いた本人が不在になっている");
        assert!(!lobby.members[1].present, "黙っている人が接続中のまま");

        // 客が覗けば戻る。
        let lobby = rooms.look(&guest, 12_000).expect("覗ける");
        assert!(lobby.members[1].present);
    }

    /// **部屋主が消えたまま戻らないと部屋が詰む。**開始を押せるのは
    /// 部屋主だけなので、逃げ道を1本だけ開けてある。
    #[tokio::test(start_paused = true)]
    async fn a_guest_may_start_once_the_host_is_long_gone() {
        let rooms = Rooms::new();
        let (code, host) = rooms.create("まさ", None, 0);
        let guest = rooms.join(&code, "たろう", None, 0).expect("入れる");
        rooms.look(&host, 0).expect("覗ける");

        // 29秒ではまだ押せない。
        let lobby = rooms.look(&guest, 29_000).expect("覗ける");
        assert!(!lobby.can_start, "部屋主が消えたと決めるのが早すぎる");
        assert_eq!(rooms.start(&guest, 29_000), Err(StartError::NotHost));

        // 30秒で押せる。
        let lobby = rooms.look(&guest, 30_000).expect("覗ける");
        assert!(lobby.can_start, "部屋主が黙ったまま部屋が詰んでいる");
        assert!(rooms.start(&guest, 30_000).is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn a_second_start_is_refused() {
        let rooms = Rooms::new();
        let (_, host) = rooms.create("まさ", None, 0);
        rooms.start(&host, 0).expect("1度目は通る");
        assert_eq!(rooms.start(&host, 0), Err(StartError::AlreadyStarted));
    }

    /// **席は開始のたびに混ぜる。**入室順に配ると部屋主が必ず起家になる。
    #[test]
    fn seats_are_dealt_by_chance() {
        let mut host_seats = [0u32; SEATS];
        for _ in 0..2_000 {
            let dealt = deal_seats(2);
            assert_eq!(dealt.len(), 2);
            assert_ne!(dealt[0], dealt[1], "2人が同じ席に着いている");
            host_seats[dealt[0].index()] += 1;
        }
        for (seat, count) in host_seats.iter().enumerate() {
            assert!(
                *count > 350,
                "席{seat} が {count} 回しか出ない。混ざっていない"
            );
        }
    }

    #[test]
    fn everyone_gets_a_different_seat() {
        for members in 1..=SEATS {
            let dealt = deal_seats(members);
            assert_eq!(dealt.len(), members);
            let unique: HashSet<_> = dealt.iter().collect();
            assert_eq!(unique.len(), members, "席が重なっている");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_waiting_room_has_no_seat_yet() {
        let rooms = Rooms::new();
        let (_, host) = rooms.create("まさ", None, 0);
        assert!(rooms.seat_of(&host).is_none(), "卓が立つ前に席が返っている");
    }

    /// **人が2人なら CPU は2人。**空席の埋め方を確かめる。
    #[tokio::test(start_paused = true)]
    async fn the_empty_seats_are_filled_by_cpus() {
        let rooms = Rooms::new();
        let (code, host) = rooms.create("まさ", None, 0);
        let guest = rooms.join(&code, "たろう", None, 0).expect("入れる");
        rooms.start(&host, 0).expect("始められる");

        let (handle, seat) = rooms.seat_of(&host).expect("席がある");
        let (_, mut inbox) = handle.attach(seat, None).await.expect("卓は生きている");
        let mut players = None;
        while let Ok(envelope) = inbox.try_recv() {
            if let ClientEvent::MatchStart { players: p, .. } = &envelope.event {
                players = Some(p.clone());
            }
        }
        let players = players.expect("MatchStart が来ていない");
        let names: Vec<String> = players.iter().map(|p| p.0.clone()).collect();
        assert!(names.contains(&"まさ".to_owned()), "{names:?}");
        assert!(names.contains(&"たろう".to_owned()), "{names:?}");
        assert_eq!(
            names.iter().filter(|n| n.starts_with("CPU")).count(),
            2,
            "CPU の数が合わない: {names:?}"
        );

        let (_, other) = rooms.seat_of(&guest).expect("席がある");
        assert_ne!(seat, other, "2人が同じ席に着いている");
    }

    /// **索引も一緒に消さないと、台帳が空でもトークンだけが積み上がる。**
    #[tokio::test(start_paused = true)]
    async fn an_abandoned_room_is_swept_with_its_tokens() {
        let rooms = Rooms::new();
        let (code, host) = rooms.create("まさ", None, 0);
        rooms.join(&code, "たろう", None, 0).expect("入れる");

        assert_eq!(rooms.sweep(IDLE_MS - 1), 0, "29分で捨てられている");
        assert_eq!(rooms.sweep(IDLE_MS), 1, "30分経っても残っている");
        assert_eq!(rooms.len(), 0);
        assert!(rooms.look(&host, IDLE_MS).is_none());
        assert!(
            rooms
                .inner
                .lock()
                .expect("毒されていない")
                .by_token
                .is_empty(),
            "索引にトークンが残っている"
        );
    }

    /// 覗いているあいだは捨てない。
    #[tokio::test(start_paused = true)]
    async fn looking_keeps_a_room_alive() {
        let rooms = Rooms::new();
        let (_, host) = rooms.create("まさ", None, 0);
        rooms.look(&host, IDLE_MS - 1).expect("覗ける");
        assert_eq!(rooms.sweep(IDLE_MS), 0, "覗いたばかりの部屋が捨てられた");
    }

    #[tokio::test(start_paused = true)]
    async fn a_finished_room_is_swept_away() {
        let rooms = Rooms::new();
        let (_, host) = rooms.create("ひとり", None, 0);
        rooms.start(&host, 0).expect("始められる");
        let (handle, seat) = rooms.seat_of(&host).expect("席がある");
        let (_, mut watcher) = handle.attach(seat, None).await.expect("卓は生きている");
        // 誰も打たなくても持ち時間が尽きて半荘が終わる。
        while watcher.recv().await.is_some() {}
        for _ in 0..100 {
            if handle.is_closed() {
                break;
            }
            tokio::task::yield_now().await;
        }

        let now = rooms.now_ms();
        assert_eq!(rooms.sweep(now), 1, "終わった卓の部屋が残っている");
        assert_eq!(rooms.len(), 0);
    }
}

#[cfg(test)]
mod material_tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn a_code_avoids_letters_that_sound_alike() {
        // **口で伝える合言葉である。**`0` と `O`、`1` と `I` が混ざると
        // 「ゼロ」「オー」の確認から始まる。
        for _ in 0..200 {
            let Code(code) = new_code();
            assert_eq!(code.chars().count(), CODE_LEN, "{code} の長さが違う");
            for c in code.chars() {
                assert!(
                    !"0O1I".contains(c),
                    "{code} に紛らわしい字 {c} が入っている"
                );
                assert!(c.is_ascii_uppercase() || c.is_ascii_digit(), "{code}");
            }
        }
    }

    #[test]
    fn codes_do_not_collide_in_practice() {
        let mut seen = HashSet::new();
        for _ in 0..1_000 {
            assert!(seen.insert(new_code()), "1,000本で重複した");
        }
    }

    #[test]
    fn a_token_is_thirty_two_hex_digits() {
        let Token(token) = new_token();
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(new_token(), new_token());
    }

    #[test]
    fn a_name_loses_control_characters_and_edges() {
        // 改行が残ると待合の枠が縦に伸びる。
        assert_eq!(clean_name("  まさ\n "), "まさ");
        assert_eq!(clean_name("た\u{0}ろう"), "たろう");
    }

    #[test]
    fn a_long_name_is_cut_not_refused() {
        // **文字数で切る。**バイト数で切ると日本語の途中で割れる。
        let cut = clean_name("あいうえおかきくけこさしすせそ");
        assert_eq!(cut.chars().count(), NAME_MAX);
        assert_eq!(cut, "あいうえおかきくけこさし");
    }

    #[test]
    fn an_empty_name_becomes_anonymous() {
        assert_eq!(clean_name(""), ANONYMOUS);
        assert_eq!(clean_name("   "), ANONYMOUS);
    }
}

#[cfg(test)]
mod two_humans_tests {
    use super::*;
    use protocol::client_event::{ClientEvent, ClientEventEnvelope};

    async fn drain(
        inbox: &mut tokio::sync::mpsc::Receiver<ClientEventEnvelope>,
    ) -> Vec<ClientEvent> {
        let mut seen = Vec::new();
        while let Ok(envelope) = inbox.try_recv() {
            seen.push(envelope.event);
        }
        seen
    }

    /// **2人が別の席で打てる。**第2段の目的そのもの。
    #[tokio::test(start_paused = true)]
    async fn two_people_sit_at_different_seats_of_one_table() {
        let rooms = Rooms::new();
        let (code, host) = rooms.create("まさ", None, 0);
        let guest = rooms.join(&code, "たろう", None, 0).expect("入れる");
        rooms.start(&host, 0).expect("始められる");

        let (table, host_seat) = rooms.seat_of(&host).expect("席がある");
        let (_, guest_seat) = rooms.seat_of(&guest).expect("席がある");
        assert_ne!(host_seat, guest_seat, "2人が同じ席に着いている");

        let (_, mut host_inbox) = table.attach(host_seat, None).await.expect("卓は生きている");
        let (_, mut guest_inbox) = table
            .attach(guest_seat, None)
            .await
            .expect("卓は生きている");

        for (seat, events) in [
            (host_seat, drain(&mut host_inbox).await),
            (guest_seat, drain(&mut guest_inbox).await),
        ] {
            let you = events.iter().find_map(|event| match event {
                ClientEvent::MatchStart { you, .. } => Some(*you),
                _ => None,
            });
            assert_eq!(you, Some(seat), "自席の伝わり方が違う");
        }
    }

    /// **他人の手牌が見えない。**視界フィルタの最後の砦。
    ///
    /// 席をトークンで縛る変更が効いていなくても、他の試験は全部通ったまま
    /// この状態になりうる。だから1本を独立させて見張る。
    #[tokio::test(start_paused = true)]
    async fn neither_player_can_see_the_other_hand() {
        let rooms = Rooms::new();
        let (code, host) = rooms.create("まさ", None, 0);
        let guest = rooms.join(&code, "たろう", None, 0).expect("入れる");
        rooms.start(&host, 0).expect("始められる");

        let (table, host_seat) = rooms.seat_of(&host).expect("席がある");
        let (_, guest_seat) = rooms.seat_of(&guest).expect("席がある");
        let (_, mut host_inbox) = table.attach(host_seat, None).await.expect("卓は生きている");
        let (_, mut guest_inbox) = table
            .attach(guest_seat, None)
            .await
            .expect("卓は生きている");

        // 局が終わるまで打たせる。誰も打たないので締切でツモ切りされ、
        // 鳴きも和了も流局も一通り通る。
        tokio::time::sleep(std::time::Duration::from_millis(120_000)).await;
        let host_events = drain(&mut host_inbox).await;
        let guest_events = drain(&mut guest_inbox).await;

        // **数えないと空回りに気づけない。**他席のツモが1件も含まれない
        // 区間を調べて「漏れなし」と言っても、何も見ていないのと同じ。
        let mut foreign_draws = 0;
        for (seat, events) in [(host_seat, &host_events), (guest_seat, &guest_events)] {
            for event in events.iter() {
                match event {
                    // 配牌は自席のぶんだけ。
                    ClientEvent::Deal { your_hand, .. } => {
                        assert_eq!(your_hand.len(), 13, "配牌の枚数が違う");
                    }
                    // **ツモ牌は自席のみ。**他席のツモに牌が乗っていたら漏れ。
                    ClientEvent::Draw {
                        seat: drawer, tile, ..
                    } if *drawer != seat => {
                        foreign_draws += 1;
                        assert!(
                            tile.is_none(),
                            "席{drawer:?} のツモ牌が席{seat:?} に見えている"
                        );
                    }
                    _ => {}
                }
            }
        }

        assert!(
            foreign_draws >= 4,
            "他席のツモを {foreign_draws} 件しか見ていない。試験が空回りしている"
        );

        // 配牌が同じなら、そもそも別の席として配られていない。
        let hand_of = |events: &[ClientEvent]| {
            events.iter().find_map(|event| match event {
                ClientEvent::Deal { your_hand, .. } => Some(your_hand.clone()),
                _ => None,
            })
        };
        let mine = hand_of(&host_events).expect("配牌が届いていない");
        let theirs = hand_of(&guest_events).expect("配牌が届いていない");
        assert_ne!(mine, theirs, "2人に同じ手牌が配られている");
    }
}

#[cfg(test)]
mod connection_tests {
    use super::*;
    use protocol::client_event::ClientEvent;

    /// **置き換えられた接続の受け口は閉じる。**
    ///
    /// WS のループで「受け口を先に見る」修正は、この性質に乗っている。
    /// ここが崩れると、古い画面から遅れて届いた打牌が通ってしまう。
    #[tokio::test(start_paused = true)]
    async fn a_superseded_connection_sees_its_inbox_close() {
        use tokio::sync::mpsc::error::TryRecvError;

        let rooms = Rooms::new();
        let (_, token) = rooms.create("まさ", None, 0);
        rooms.start(&token, 0).expect("始められる");
        let (handle, seat) = rooms.seat_of(&token).expect("席がある");

        let (_, mut old) = handle.attach(seat, None).await.expect("卓は生きている");
        while old.try_recv().is_ok() {}

        // 同じ席へ2本目を繋ぐ。卓は1本目の送り口を捨てる。
        let (_, _new) = handle.attach(seat, None).await.expect("卓は生きている");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let closed = loop {
            match old.try_recv() {
                Ok(_) => {}
                Err(TryRecvError::Empty) => break false,
                Err(TryRecvError::Disconnected) => break true,
            }
        };
        assert!(closed, "置き換えられた受け口が閉じていない");
    }

    /// **同じトークンで繋ぎ直せば同じ席に戻る。**ブラウザを再読み込みしても
    /// 対局が最初からにならないことが、遊びやすさに直結する。
    #[tokio::test(start_paused = true)]
    async fn the_same_token_returns_to_the_same_seat() {
        let rooms = Rooms::new();
        let (_, token) = rooms.create("まさ", None, 0);
        rooms.start(&token, 0).expect("始められる");

        let (handle, seat) = rooms.seat_of(&token).expect("席がある");
        let (connection, mut inbox) = handle.attach(seat, None).await.expect("卓は生きている");
        let mut seen = Vec::new();
        while let Ok(envelope) = inbox.try_recv() {
            seen.push(envelope.seq);
        }
        let last = *seen.last().expect("何も届いていない");
        handle
            .detach(seat, connection)
            .await
            .expect("卓は生きている");
        drop(inbox);

        // 卓は動き続ける。
        tokio::time::sleep(std::time::Duration::from_millis(30_000)).await;

        let (again, same_seat) = rooms.seat_of(&token).expect("席がある");
        assert_eq!(same_seat, seat, "繋ぎ直しで席が変わっている");
        let (_, mut back) = again
            .attach(same_seat, Some(last))
            .await
            .expect("卓は生きている");
        let mut caught_up = Vec::new();
        while let Ok(envelope) = back.try_recv() {
            caught_up.push(envelope.seq);
        }

        assert!(!caught_up.is_empty(), "留守中の分が追いついていない");
        assert!(
            caught_up.iter().all(|seq| *seq > last),
            "見たものがまた来ている"
        );
        assert!(
            !caught_up.contains(&0),
            "繋ぎ直しで対局が最初からになっている"
        );

        let mut restarted = false;
        let (_, mut fresh) = again.attach(same_seat, None).await.expect("卓は生きている");
        while let Ok(envelope) = fresh.try_recv() {
            if matches!(envelope.event, ClientEvent::MatchStart { .. }) {
                restarted = true;
            }
        }
        assert!(restarted, "last_seq なしなら最初から送り直すはず");
    }
}

#[cfg(test)]
mod recording_tests {
    use super::*;
    use crate::persistence::Store;

    /// 倉と、それを抱えた台帳。試験のあいだだけ生きるファイルを使う。
    fn ledger_with_store(tag: &str) -> (Rooms, std::path::PathBuf) {
        let path =
            std::env::temp_dir().join(format!("mj-rooms-{tag}-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = Store::open(&path).expect("開ける");
        (Rooms::with_scribe(Some(Scribe::spawn(store))), path)
    }

    async fn wait_for_head(path: &std::path::Path) -> Option<MatchHead> {
        let reader = Store::open(path).expect("開ける");
        for _ in 0..500 {
            if let Ok(mut all) = reader.list("key-a", 10) {
                if let Some(head) = all.pop() {
                    return Some(head);
                }
            }
            tokio::task::yield_now().await;
        }
        None
    }

    /// 卓が立つと見出しの行ができる。
    #[tokio::test(start_paused = true)]
    async fn starting_writes_the_head() {
        let (rooms, path) = ledger_with_store("head");
        let (_, host) = rooms.create("まさ", Some("key-a"), 0);
        rooms.start(&host, 0).expect("始められる");

        let head = wait_for_head(&path).await.expect("見出しが書かれていない");
        assert_eq!(head.players.len(), 4);
        assert!(
            head.players.contains(&"まさ".to_owned()),
            "{:?}",
            head.players
        );
        assert!(head.rules_json.contains("Hanchan"), "{}", head.rules_json);
        assert!(head.started_ms > 0, "実時刻が入っていない");
        assert_eq!(head.ended_ms, None);

        let _ = std::fs::remove_file(&path);
    }

    /// **人の席にだけ証明と鍵が入り、CPU の席には入らない。**
    #[tokio::test(start_paused = true)]
    async fn only_people_carry_credentials() {
        let (rooms, path) = ledger_with_store("cred");
        let (code, host) = rooms.create("まさ", Some("key-a"), 0);
        let guest = rooms
            .join(&code, "たろう", Some("key-b"), 0)
            .expect("入れる");
        rooms.start(&host, 0).expect("始められる");

        let head = wait_for_head(&path).await.expect("見出しが書かれていない");
        let reader = Store::open(&path).expect("開ける");
        let rows = reader.seats(&head.id).expect("引ける");
        assert_eq!(rows.len(), 4);

        let people: Vec<&SeatRow> = rows.iter().filter(|r| !r.is_cpu).collect();
        assert_eq!(people.len(), 2, "人の数が合わない");
        for row in &people {
            assert!(row.token_hash.is_some(), "席{} に証明が無い", row.seat);
            assert!(row.player_key.is_some(), "席{} に鍵が無い", row.seat);
        }
        for row in rows.iter().filter(|r| r.is_cpu) {
            assert!(
                row.token_hash.is_none(),
                "CPU の席{} に証明がある",
                row.seat
            );
            assert!(row.player_key.is_none(), "CPU の席{} に鍵がある", row.seat);
        }

        // 席の証明が、その席の行に入っていること。
        let (_, host_seat) = rooms.seat_of(&host).expect("席がある");
        let mine = rows
            .iter()
            .find(|r| r.seat == host_seat.index() as u8)
            .expect("行がある");
        assert_eq!(mine.token_hash, Some(hash_token(&host.0)));
        assert_eq!(mine.player_key.as_deref(), Some("key-a"));

        let (_, guest_seat) = rooms.seat_of(&guest).expect("席がある");
        let theirs = rows
            .iter()
            .find(|r| r.seat == guest_seat.index() as u8)
            .expect("行がある");
        assert_eq!(theirs.token_hash, Some(hash_token(&guest.0)));

        let _ = std::fs::remove_file(&path);
    }

    /// **鍵を送らなくても対局はできる。**一覧に出ないだけ。
    #[tokio::test(start_paused = true)]
    async fn a_keyless_player_can_still_play() {
        let (rooms, path) = ledger_with_store("nokey");
        let (_, host) = rooms.create("まさ", None, 0);
        rooms.start(&host, 0).expect("始められる");
        assert!(rooms.seat_of(&host).is_some(), "卓に着けていない");

        let reader = Store::open(&path).expect("開ける");
        for _ in 0..200 {
            tokio::task::yield_now().await;
        }
        assert!(
            reader.list("key-a", 10).expect("引ける").is_empty(),
            "鍵を送っていないのに一覧に出ている"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// 卓が生きているあいだは、トークンから牌譜を引ける。
    #[tokio::test(start_paused = true)]
    async fn a_token_points_at_its_record_while_the_table_lives() {
        let (rooms, path) = ledger_with_store("point");
        let (code, host) = rooms.create("まさ", Some("key-a"), 0);
        let guest = rooms
            .join(&code, "たろう", Some("key-b"), 0)
            .expect("入れる");

        // 卓が立つ前は何も指さない。
        assert!(rooms.record_of(&host).is_none(), "待合で牌譜が引けている");

        rooms.start(&host, 0).expect("始められる");
        let (mine, my_seat) = rooms.record_of(&host).expect("引ける");
        let (theirs, their_seat) = rooms.record_of(&guest).expect("引ける");
        assert_eq!(mine, theirs, "同じ卓なのに別の牌譜を指している");
        assert_ne!(my_seat, their_seat, "2人が同じ席を指している");

        assert!(
            rooms.record_of(&Token("知らない".to_owned())).is_none(),
            "知らない証明で牌譜が引けている"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// 倉を持たない台帳でも卓は立つ。**牌譜は第2段で足したもので、対局の前提ではない。**
    #[tokio::test(start_paused = true)]
    async fn a_ledger_without_a_store_still_seats_people() {
        let rooms = Rooms::new();
        let (_, host) = rooms.create("まさ", Some("key-a"), 0);
        rooms.start(&host, 0).expect("始められる");
        assert!(rooms.seat_of(&host).is_some());
    }
}
