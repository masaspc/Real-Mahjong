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
}
