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

use crate::session::{spawn, Clock, SeedSource, TableHandle};
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

/// 卓を指す文字列。クライアントが作って `localStorage` に置く。
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct TableId(pub String);

/// 動いている卓の台帳。
#[derive(Clone)]
pub struct Tables {
    inner: Arc<Mutex<HashMap<TableId, TableHandle>>>,
}

impl Default for Tables {
    fn default() -> Self {
        Tables::new()
    }
}

impl Tables {
    pub fn new() -> Self {
        Tables {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// その id の卓を返す。無ければ作る。
    ///
    /// **席0が人、1〜3が CPU。**席替えとマッチングは後のウェーブ。
    pub fn get_or_create(&self, id: &TableId) -> TableHandle {
        let mut map = self.inner.lock().expect("毒されていない");
        if let Some(handle) = map.get(id) {
            if !handle.is_closed() {
                return handle.clone();
            }
        }
        let (handle, _actor) = spawn(
            Ruleset::kin_no_ma(MatchLength::Hanchan),
            [
                Occupant::Human(PlayerId("you".to_owned())),
                Occupant::Cpu(PlayerId("cpu1".to_owned())),
                Occupant::Cpu(PlayerId("cpu2".to_owned())),
                Occupant::Cpu(PlayerId("cpu3".to_owned())),
            ],
            SeedSource::from_os(),
        );
        map.insert(id.clone(), handle.clone());
        handle
    }

    /// 終わった卓を捨てる。捨てた数を返す。
    pub fn sweep(&self) -> usize {
        let mut map = self.inner.lock().expect("毒されていない");
        let before = map.len();
        map.retain(|_, handle| !handle.is_closed());
        before - map.len()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("毒されていない").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
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
}

/// 部屋の居場所。
enum RoomState {
    /// 人が集まっている。卓はまだ無い。
    Waiting,
    /// 卓が立っている。`seats[i]` は `members[i]` の席。
    Playing {
        handle: TableHandle,
        seats: Vec<Seat>,
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
}

impl Default for Rooms {
    fn default() -> Self {
        Rooms::new()
    }
}

impl Rooms {
    pub fn new() -> Self {
        Rooms {
            inner: Arc::new(Mutex::new(Ledger {
                rooms: HashMap::new(),
                by_token: HashMap::new(),
            })),
            clock: Arc::new(Clock::start()),
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
    pub fn create(&self, name: &str, now_ms: u64) -> (Code, Token) {
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
                }],
                state: RoomState::Waiting,
                touched_ms: now_ms,
            },
        );
        ledger.by_token.insert(token.clone(), code.clone());
        (code, token)
    }

    /// 部屋に入る。
    pub fn join(&self, code: &Code, name: &str, now_ms: u64) -> Result<Token, JoinError> {
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

        let (handle, _actor) = spawn(
            Ruleset::kin_no_ma(MatchLength::Hanchan),
            occupants,
            SeedSource::from_os(),
        );
        room.state = RoomState::Playing { handle, seats };
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
            RoomState::Playing { handle, seats } => Some((handle.clone(), *seats.get(index)?)),
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
        let (_, token) = rooms.create("まさ", 0);
        let lobby = rooms.look(&token, 0).expect("覗ける");
        assert!(lobby.you.host, "作った人が部屋主になっていない");
        assert_eq!(lobby.you.name, "まさ");
        assert_eq!(lobby.members.len(), 1);
        assert_eq!(lobby.state, "waiting");
    }

    #[tokio::test(start_paused = true)]
    async fn a_guest_is_not_the_host() {
        let rooms = Rooms::new();
        let (code, _) = rooms.create("まさ", 0);
        let guest = rooms.join(&code, "たろう", 0).expect("入れる");
        let lobby = rooms.look(&guest, 0).expect("覗ける");
        assert!(!lobby.you.host);
        assert_eq!(lobby.members.iter().filter(|m| m.host).count(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_fifth_person_is_turned_away() {
        let rooms = Rooms::new();
        let (code, _) = rooms.create("1", 0);
        for name in ["2", "3", "4"] {
            rooms.join(&code, name, 0).expect("4人までは入れる");
        }
        assert_eq!(rooms.join(&code, "5", 0), Err(JoinError::Full));
    }

    #[tokio::test(start_paused = true)]
    async fn an_unknown_code_is_not_a_room() {
        let rooms = Rooms::new();
        assert_eq!(
            rooms.join(&Code("ZZZZZZ".to_owned()), "まさ", 0),
            Err(JoinError::NoSuchRoom)
        );
    }

    /// **始まった部屋に「満室」と答えない。**待てば入れるように聞こえる。
    #[tokio::test(start_paused = true)]
    async fn joining_a_started_room_says_so() {
        let rooms = Rooms::new();
        let (code, host) = rooms.create("まさ", 0);
        rooms.start(&host, 0).expect("部屋主は始められる");
        assert_eq!(
            rooms.join(&code, "たろう", 0),
            Err(JoinError::AlreadyStarted)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_unknown_token_sees_nothing() {
        let rooms = Rooms::new();
        rooms.create("まさ", 0);
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
        let (code, host) = rooms.create("まさ", 0);
        let guest = rooms.join(&code, "たろう", 0).expect("入れる");

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
        let (code, host) = rooms.create("まさ", 0);
        let guest = rooms.join(&code, "たろう", 0).expect("入れる");
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
        let (_, host) = rooms.create("まさ", 0);
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
        let (_, host) = rooms.create("まさ", 0);
        assert!(rooms.seat_of(&host).is_none(), "卓が立つ前に席が返っている");
    }

    /// **人が2人なら CPU は2人。**空席の埋め方を確かめる。
    #[tokio::test(start_paused = true)]
    async fn the_empty_seats_are_filled_by_cpus() {
        let rooms = Rooms::new();
        let (code, host) = rooms.create("まさ", 0);
        let guest = rooms.join(&code, "たろう", 0).expect("入れる");
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
        let (code, host) = rooms.create("まさ", 0);
        rooms.join(&code, "たろう", 0).expect("入れる");

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
        let (_, host) = rooms.create("まさ", 0);
        rooms.look(&host, IDLE_MS - 1).expect("覗ける");
        assert_eq!(rooms.sweep(IDLE_MS), 0, "覗いたばかりの部屋が捨てられた");
    }

    #[tokio::test(start_paused = true)]
    async fn a_finished_room_is_swept_away() {
        let rooms = Rooms::new();
        let (_, host) = rooms.create("ひとり", 0);
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
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn the_same_id_gives_the_same_table() {
        let tables = Tables::new();
        let id = TableId("abc".to_owned());
        let first = tables.get_or_create(&id);
        let second = tables.get_or_create(&id);
        assert!(!first.is_closed());
        assert!(!second.is_closed());
        assert_eq!(tables.len(), 1, "同じ id で2卓できている");
    }

    #[tokio::test(start_paused = true)]
    async fn different_ids_give_different_tables() {
        let tables = Tables::new();
        let _ = tables.get_or_create(&TableId("a".to_owned()));
        let _ = tables.get_or_create(&TableId("b".to_owned()));
        assert_eq!(tables.len(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_table_seats_one_human_and_three_cpus() {
        use protocol::client_event::ClientEvent;
        use protocol::seat::Seat;

        let tables = Tables::new();
        let handle = tables.get_or_create(&TableId("x".to_owned()));
        let (_, mut inbox) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");

        let mut hand = None;
        while let Ok(envelope) = inbox.try_recv() {
            if let ClientEvent::Deal { your_hand, .. } = &envelope.event {
                hand = Some(your_hand.len());
            }
        }
        assert_eq!(hand, Some(13), "席0へ配牌が届いていない");
    }

    /// **終わった卓を捨てないと、遊ぶたびに積み上がる。**
    #[tokio::test(start_paused = true)]
    async fn a_finished_table_is_swept_away() {
        use protocol::seat::Seat;

        let tables = Tables::new();
        let handle = tables.get_or_create(&TableId("done".to_owned()));
        let (_, mut watcher) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        // 席0も CPU ではないが、誰も打たなくても持ち時間が尽きて進む。
        while watcher.recv().await.is_some() {}
        for _ in 0..100 {
            if handle.is_closed() {
                break;
            }
            tokio::task::yield_now().await;
        }

        assert_eq!(tables.sweep(), 1, "終わった卓が捨てられていない");
        assert_eq!(tables.len(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn sweeping_keeps_the_living() {
        let tables = Tables::new();
        let _ = tables.get_or_create(&TableId("alive".to_owned()));
        assert_eq!(tables.sweep(), 0);
        assert_eq!(tables.len(), 1);
    }

    /// **置き換えられた接続の受け口は閉じる。**
    ///
    /// WS のループで「受け口を先に見る」修正は、この性質に乗っている。
    /// ここが崩れると、古い画面から遅れて届いた打牌が通ってしまう。
    #[tokio::test(start_paused = true)]
    async fn a_superseded_connection_sees_its_inbox_close() {
        use protocol::seat::Seat;
        use tokio::sync::mpsc::error::TryRecvError;

        let tables = Tables::new();
        let handle = tables.get_or_create(&TableId("supersede".to_owned()));
        let (_, mut old) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        while old.try_recv().is_ok() {}

        // 同じ席へ2本目を繋ぐ。卓は1本目の送り口を捨てる。
        let (_, _new) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
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

    /// **繋ぎ直しても同じ卓に戻る。**ブラウザを再読み込みしても
    /// 対局が最初からにならないことが、評価のしやすさに直結する。
    #[tokio::test(start_paused = true)]
    async fn the_same_id_resumes_the_same_match() {
        use protocol::client_event::ClientEvent;
        use protocol::seat::Seat;

        let tables = Tables::new();
        let id = TableId("resume".to_owned());

        let first = tables.get_or_create(&id);
        let (connection, mut inbox) = first
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let mut seen = Vec::new();
        while let Ok(envelope) = inbox.try_recv() {
            seen.push(envelope.seq);
        }
        let commitment = seen.len();
        assert!(commitment > 0, "何も届いていない");
        let last = *seen.last().expect("何か届いている");
        first
            .detach(Seat::new(0), connection)
            .await
            .expect("卓は生きている");
        drop(inbox);

        // 卓は動き続ける。
        tokio::time::sleep(std::time::Duration::from_millis(30_000)).await;

        let again = tables.get_or_create(&id);
        let (_, mut back) = again
            .attach(Seat::new(0), Some(last))
            .await
            .expect("卓は生きている");
        let mut caught_up = Vec::new();
        while let Ok(envelope) = back.try_recv() {
            caught_up.push(envelope.seq);
        }

        assert_eq!(tables.len(), 1, "繋ぎ直しで別の卓ができている");
        assert!(!caught_up.is_empty(), "留守中の分が追いついていない");
        assert!(
            caught_up.iter().all(|seq| *seq > last),
            "見たものがまた来ている"
        );
        // 対局の頭からやり直していないこと。
        assert!(
            !caught_up.contains(&0),
            "繋ぎ直しで対局が最初からになっている"
        );

        let mut restarted = false;
        let (_, mut fresh) = again
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        while let Ok(envelope) = fresh.try_recv() {
            if matches!(envelope.event, ClientEvent::MatchStart { .. }) {
                restarted = true;
            }
        }
        assert!(restarted, "last_seq なしなら最初から送り直すはず");
    }
}
