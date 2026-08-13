//! 卓の台帳。id から卓を引き、無ければ作る。
//!
//! **このウェーブに認証は無い。**id を知っていれば誰でもその席に座れる。
//! ローカルで遊んで評価するための段階であり、公開するものではない。
//! 署名付きの再開トークンは後のウェーブで入れる（仕様 8.1）。

use crate::session::{spawn, SeedSource, TableHandle};
use crate::table::Occupant;
use protocol::event::PlayerId;
use protocol::ruleset::{MatchLength, Ruleset};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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
