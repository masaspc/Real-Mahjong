//! 唯一 I/O と時間を持つ層。1卓 = 1 tokio task の Actor とする。
//! このファイルはモジュール木の宣言のみを担う。編集しないこと。

pub mod http;
pub mod persistence;
pub mod rooms;
pub mod session;
pub mod table;
