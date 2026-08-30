//! 牌譜の倉。
//!
//! **保存するのはサーバの真実（`Event`）であって、射影した後のものではない。**
//! 射影後を保存すると席の数だけ列が要り、しかも「後から別の席の視点で見る」が
//! 永久にできなくなる。真実を1本だけ残し、読み出すたびに `project()` を通す。
//! 生の配信と同じ関数を通るので、視界フィルタの抜け道が生まれない。
//!
//! 設計は `docs/superpowers/specs/2026-08-30-records-design.md`。

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use protocol::event::EventEnvelope;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;

/// 対局の見出し。
#[derive(Clone, Debug, PartialEq)]
pub struct MatchHead {
    pub id: String,
    /// `Ruleset` の JSON。**倉は麻雀の中身を知らない。**
    pub rules_json: String,
    pub started_ms: u64,
    pub ended_ms: Option<u64>,
    /// 席順の名前。
    pub players: Vec<String>,
    /// 終局の点数と順位の JSON。未了なら `None`。
    pub result_json: Option<String>,
}

/// 席と、その席を見てよい人。
#[derive(Clone, Debug, PartialEq)]
pub struct SeatRow {
    pub seat: u8,
    pub name: String,
    pub is_cpu: bool,
    /// 席の証明の SHA-256。CPU は `None`。
    pub token_hash: Option<String>,
    /// その人の browser を指す鍵。CPU と、鍵を送らなかった人は `None`。
    pub player_key: Option<String>,
}

/// 席の証明を、そのまま持たずに照合できる形にする。
///
/// **漏れた倉がそのまま席の証明にならないようにする。**牌譜は消えないので、
/// 卓が畳まれた後もこの行だけは残り続ける。
pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn gzip(text: &str) -> std::io::Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(text.as_bytes())?;
    encoder.finish()
}

fn gunzip(bytes: &[u8]) -> std::io::Result<String> {
    let mut text = String::new();
    GzDecoder::new(bytes).read_to_string(&mut text)?;
    Ok(text)
}

/// 牌譜を置くところ。
pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        Store::from(Connection::open(path)?)
    }

    /// 試験のため、どこにも残さない倉。
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Store::from(Connection::open_in_memory()?)
    }

    fn from(conn: Connection) -> rusqlite::Result<Self> {
        let store = Store { conn };
        store.migrate()?;
        Ok(store)
    }

    /// 表を用意する。**何度呼んでも同じ形になる。**
    fn migrate(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            CREATE TABLE IF NOT EXISTS records (
              id          TEXT PRIMARY KEY,
              rules       TEXT NOT NULL,
              started_ms  INTEGER NOT NULL,
              ended_ms    INTEGER,
              players     TEXT NOT NULL,
              result      TEXT
            );
            CREATE TABLE IF NOT EXISTS record_seats (
              record_id   TEXT NOT NULL REFERENCES records(id),
              seat        INTEGER NOT NULL,
              name        TEXT NOT NULL,
              is_cpu      INTEGER NOT NULL,
              token_hash  TEXT,
              player_key  TEXT,
              PRIMARY KEY (record_id, seat)
            );
            CREATE INDEX IF NOT EXISTS record_seats_by_player
              ON record_seats(player_key);
            CREATE TABLE IF NOT EXISTS record_events (
              record_id   TEXT NOT NULL REFERENCES records(id),
              chunk       INTEGER NOT NULL,
              first_seq   INTEGER NOT NULL,
              last_seq    INTEGER NOT NULL,
              events      BLOB NOT NULL,
              PRIMARY KEY (record_id, chunk)
            );
            ",
        )
    }

    /// 対局が始まったことを書く。
    pub fn begin_match(&self, head: &MatchHead, seats: &[SeatRow]) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO records (id, rules, started_ms, players)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                head.id,
                head.rules_json,
                head.started_ms as i64,
                serde_json::to_string(&head.players).unwrap_or_else(|_| "[]".to_owned()),
            ],
        )?;
        for row in seats {
            self.conn.execute(
                "INSERT OR REPLACE INTO record_seats
                   (record_id, seat, name, is_cpu, token_hash, player_key)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    head.id,
                    row.seat,
                    row.name,
                    row.is_cpu as i32,
                    row.token_hash,
                    row.player_key,
                ],
            )?;
        }
        Ok(())
    }

    /// 局の切れ目で、その局のぶんを1塊として足す。
    ///
    /// **1件1行にしない。**半荘で1,300行になるうえ、読み出しは必ず対局
    /// まるごとなので、行に割る利点が無い。
    pub fn append(&self, record_id: &str, events: &[EventEnvelope]) -> rusqlite::Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let jsonl: String = events
            .iter()
            .filter_map(|envelope| serde_json::to_string(envelope).ok())
            .collect::<Vec<_>>()
            .join("\n");
        let blob = gzip(&jsonl)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let chunk: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(chunk) + 1, 0) FROM record_events WHERE record_id = ?1",
            params![record_id],
            |row| row.get(0),
        )?;
        self.conn.execute(
            "INSERT INTO record_events (record_id, chunk, first_seq, last_seq, events)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                record_id,
                chunk,
                events.first().map(|e| e.seq).unwrap_or(0),
                events.last().map(|e| e.seq).unwrap_or(0),
                blob,
            ],
        )?;
        Ok(())
    }

    /// 終局を書く。
    pub fn finish(
        &self,
        record_id: &str,
        ended_ms: u64,
        result_json: Option<&str>,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE records SET ended_ms = ?2, result = ?3 WHERE id = ?1",
            params![record_id, ended_ms as i64, result_json],
        )?;
        Ok(())
    }

    /// 席の証明から席を引く。**知らない証明では何も返さない。**
    pub fn seat_of(&self, record_id: &str, token_hash: &str) -> rusqlite::Result<Option<u8>> {
        self.conn
            .query_row(
                "SELECT seat FROM record_seats WHERE record_id = ?1 AND token_hash = ?2",
                params![record_id, token_hash],
                |row| row.get::<_, u8>(0),
            )
            .optional()
    }

    /// 保存した真実を、書いた順に繋げて返す。
    pub fn events(&self, record_id: &str) -> rusqlite::Result<Vec<EventEnvelope>> {
        let mut statement = self
            .conn
            .prepare("SELECT events FROM record_events WHERE record_id = ?1 ORDER BY chunk")?;
        let blobs = statement.query_map(params![record_id], |row| row.get::<_, Vec<u8>>(0))?;
        let mut all = Vec::new();
        for blob in blobs {
            let text = gunzip(&blob?).unwrap_or_default();
            for line in text.lines() {
                if let Ok(envelope) = serde_json::from_str::<EventEnvelope>(line) {
                    all.push(envelope);
                }
            }
        }
        Ok(all)
    }

    pub fn head(&self, record_id: &str) -> rusqlite::Result<Option<MatchHead>> {
        self.conn
            .query_row(
                "SELECT id, rules, started_ms, ended_ms, players, result
                 FROM records WHERE id = ?1",
                params![record_id],
                read_head,
            )
            .optional()
    }

    pub fn seats(&self, record_id: &str) -> rusqlite::Result<Vec<SeatRow>> {
        let mut statement = self.conn.prepare(
            "SELECT seat, name, is_cpu, token_hash, player_key
             FROM record_seats WHERE record_id = ?1 ORDER BY seat",
        )?;
        let rows = statement.query_map(params![record_id], |row| {
            Ok(SeatRow {
                seat: row.get(0)?,
                name: row.get(1)?,
                is_cpu: row.get::<_, i32>(2)? != 0,
                token_hash: row.get(3)?,
                player_key: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// その browser が打った対局。**新しい順。**
    pub fn list(&self, player_key: &str, limit: u32) -> rusqlite::Result<Vec<MatchHead>> {
        let mut statement = self.conn.prepare(
            "SELECT r.id, r.rules, r.started_ms, r.ended_ms, r.players, r.result
             FROM records r
             JOIN record_seats s ON s.record_id = r.id
             WHERE s.player_key = ?1
             ORDER BY r.started_ms DESC
             LIMIT ?2",
        )?;
        let rows = statement.query_map(params![player_key, limit], read_head)?;
        rows.collect()
    }
}

fn read_head(row: &rusqlite::Row<'_>) -> rusqlite::Result<MatchHead> {
    let players: String = row.get(4)?;
    Ok(MatchHead {
        id: row.get(0)?,
        rules_json: row.get(1)?,
        started_ms: row.get::<_, i64>(2)? as u64,
        ended_ms: row.get::<_, Option<i64>>(3)?.map(|ms| ms as u64),
        players: serde_json::from_str(&players).unwrap_or_default(),
        result_json: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::event::{Event, PlayerId};
    use protocol::seat::Seat;

    fn head(id: &str, started_ms: u64) -> MatchHead {
        MatchHead {
            id: id.to_owned(),
            rules_json: r#"{"length":"Hanchan"}"#.to_owned(),
            started_ms,
            ended_ms: None,
            players: vec!["まさ".into(), "たろう".into(), "CPU1".into(), "CPU2".into()],
            result_json: None,
        }
    }

    fn seats() -> Vec<SeatRow> {
        vec![
            SeatRow {
                seat: 0,
                name: "まさ".into(),
                is_cpu: false,
                token_hash: Some(hash_token("token-a")),
                player_key: Some("key-a".into()),
            },
            SeatRow {
                seat: 1,
                name: "たろう".into(),
                is_cpu: false,
                token_hash: Some(hash_token("token-b")),
                player_key: Some("key-b".into()),
            },
            SeatRow {
                seat: 2,
                name: "CPU1".into(),
                is_cpu: true,
                token_hash: None,
                player_key: None,
            },
            SeatRow {
                seat: 3,
                name: "CPU2".into(),
                is_cpu: true,
                token_hash: None,
                player_key: None,
            },
        ]
    }

    fn chunk(from: u32, count: u32) -> Vec<EventEnvelope> {
        (from..from + count)
            .map(|seq| EventEnvelope {
                seq,
                event: Event::DoraReveal {
                    indicator: protocol::tile::Tile::from_encoded((seq % 34) as u8)
                        .expect("範囲内"),
                },
            })
            .collect()
    }

    #[test]
    fn a_fresh_file_gets_its_tables() {
        let dir = std::env::temp_dir().join(format!("mj-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("作れる");
        let path = dir.join("records.sqlite");
        let _ = std::fs::remove_file(&path);

        // **2度開いても壊れない。**起動のたびに migrate が走る。
        {
            let store = Store::open(&path).expect("開ける");
            store
                .begin_match(&head("a", 100), &seats())
                .expect("書ける");
        }
        {
            let store = Store::open(&path).expect("2度目も開ける");
            assert!(store.head("a").expect("引ける").is_some());
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn chunks_join_back_into_one_stream() {
        let store = Store::open_in_memory().expect("開ける");
        store
            .begin_match(&head("a", 100), &seats())
            .expect("書ける");
        store.append("a", &chunk(0, 5)).expect("書ける");
        store.append("a", &chunk(5, 5)).expect("書ける");
        store.append("a", &chunk(10, 3)).expect("書ける");

        let all = store.events("a").expect("読める");
        assert_eq!(all.len(), 13);
        let seqs: Vec<u32> = all.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, (0..13).collect::<Vec<_>>(), "順番が崩れている");
    }

    /// **空を投げても塊を作らない。**局が0件で終わることは無いが、
    /// 作ると読み出しに空行が混ざる。
    #[test]
    fn an_empty_chunk_is_not_written() {
        let store = Store::open_in_memory().expect("開ける");
        store
            .begin_match(&head("a", 100), &seats())
            .expect("書ける");
        store.append("a", &[]).expect("書ける");
        assert!(store.events("a").expect("読める").is_empty());
    }

    /// **CPU の席には証明も鍵も入れない。**入れると、CPU の席として
    /// 牌譜を引ける道ができる。
    #[test]
    fn a_cpu_seat_carries_no_credentials() {
        let store = Store::open_in_memory().expect("開ける");
        store
            .begin_match(&head("a", 100), &seats())
            .expect("書ける");
        for row in store.seats("a").expect("引ける") {
            if row.is_cpu {
                assert!(row.token_hash.is_none(), "席{} に証明がある", row.seat);
                assert!(row.player_key.is_none(), "席{} に鍵がある", row.seat);
            }
        }
    }

    #[test]
    fn a_token_finds_only_its_own_seat() {
        let store = Store::open_in_memory().expect("開ける");
        store
            .begin_match(&head("a", 100), &seats())
            .expect("書ける");
        assert_eq!(
            store.seat_of("a", &hash_token("token-a")).expect("引ける"),
            Some(0)
        );
        assert_eq!(
            store.seat_of("a", &hash_token("token-b")).expect("引ける"),
            Some(1)
        );
        assert_eq!(
            store.seat_of("a", &hash_token("知らない")).expect("引ける"),
            None
        );
    }

    /// **証明は生で持たない。**漏れた倉がそのまま席の証明にならないため。
    #[test]
    fn the_raw_token_is_never_stored() {
        let store = Store::open_in_memory().expect("開ける");
        store
            .begin_match(&head("a", 100), &seats())
            .expect("書ける");
        for row in store.seats("a").expect("引ける") {
            assert_ne!(row.token_hash.as_deref(), Some("token-a"));
            assert_ne!(row.token_hash.as_deref(), Some("token-b"));
        }
        assert_eq!(hash_token("token-a").len(), 64);
    }

    #[test]
    fn finishing_fills_in_the_end() {
        let store = Store::open_in_memory().expect("開ける");
        store
            .begin_match(&head("a", 100), &seats())
            .expect("書ける");
        assert_eq!(
            store.head("a").expect("引ける").expect("ある").ended_ms,
            None
        );

        store
            .finish("a", 900, Some(r#"{"placements":[1,2,3,4]}"#))
            .expect("書ける");
        let done = store.head("a").expect("引ける").expect("ある");
        assert_eq!(done.ended_ms, Some(900));
        assert!(done.result_json.expect("ある").contains("placements"));
    }

    /// 途中で落ちても、それまでの局は読める。
    #[test]
    fn an_unfinished_match_still_reads_back() {
        let store = Store::open_in_memory().expect("開ける");
        store
            .begin_match(&head("a", 100), &seats())
            .expect("書ける");
        store.append("a", &chunk(0, 40)).expect("書ける");

        let head = store.head("a").expect("引ける").expect("ある");
        assert_eq!(head.ended_ms, None, "終わっていないのに終局が入っている");
        assert_eq!(store.events("a").expect("読める").len(), 40);
    }

    #[test]
    fn the_list_is_newest_first_and_per_person() {
        let store = Store::open_in_memory().expect("開ける");
        store
            .begin_match(&head("old", 100), &seats())
            .expect("書ける");
        store
            .begin_match(&head("new", 500), &seats())
            .expect("書ける");

        let mine = store.list("key-a", 10).expect("引ける");
        assert_eq!(
            mine.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(),
            vec!["new", "old"],
            "新しい順になっていない"
        );
        assert!(
            store.list("知らない鍵", 10).expect("引ける").is_empty(),
            "他人の鍵で牌譜が出ている"
        );
    }

    #[test]
    fn the_list_respects_its_limit() {
        let store = Store::open_in_memory().expect("開ける");
        for i in 0..5 {
            store
                .begin_match(&head(&format!("m{i}"), 100 + i), &seats())
                .expect("書ける");
        }
        assert_eq!(store.list("key-a", 3).expect("引ける").len(), 3);
    }

    /// 名前と席は書いたとおりに戻る。
    #[test]
    fn seats_come_back_in_order() {
        let store = Store::open_in_memory().expect("開ける");
        store
            .begin_match(&head("a", 100), &seats())
            .expect("書ける");
        let rows = store.seats("a").expect("引ける");
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].seat, 0);
        assert_eq!(rows[0].name, "まさ");
        assert_eq!(rows[3].name, "CPU2");
    }

    /// **真実がそのまま戻る。**JSON にして gzip して戻す往復で、
    /// 中身が変わっていないこと。
    #[test]
    fn the_truth_survives_the_round_trip() {
        let store = Store::open_in_memory().expect("開ける");
        store
            .begin_match(&head("a", 100), &seats())
            .expect("書ける");
        let written = vec![
            EventEnvelope {
                seq: 0,
                event: Event::MatchStart {
                    players: std::array::from_fn(|i| PlayerId(format!("p{i}"))),
                    rules: protocol::ruleset::Ruleset::kin_no_ma(
                        protocol::ruleset::MatchLength::Hanchan,
                    ),
                },
            },
            EventEnvelope {
                seq: 1,
                event: Event::Discard {
                    seat: Seat::new(2),
                    tile: protocol::tile::Tile::from_encoded(11).expect("範囲内"),
                    manner: protocol::event::DiscardManner::Tsumogiri,
                },
            },
        ];
        store.append("a", &written).expect("書ける");
        assert_eq!(store.events("a").expect("読める"), written);
    }

    #[test]
    fn gzip_actually_shrinks_the_record() {
        // **圧縮が効かない形になっていたら気付く。**中身は JSON という
        // よく縮む文字列なので、素の JSONL より小さくなるはず。
        let text = "{\"seq\":0,\"event\":{\"type\":\"dora_reveal\",\"indicator\":3}}\n".repeat(200);
        let packed = gzip(&text).expect("縮む");
        assert!(packed.len() * 10 < text.len(), "圧縮が効いていない");
        assert_eq!(gunzip(&packed).expect("戻る"), text);
    }
}
