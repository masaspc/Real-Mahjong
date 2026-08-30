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

use crate::session::{spawn, SeedSource, TableHandle};
use crate::table::Occupant;
use protocol::event::PlayerId;
use protocol::ruleset::{MatchLength, Ruleset};
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
