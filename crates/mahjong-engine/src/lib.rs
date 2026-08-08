//! 局と半荘の進行。シードを外から注入され、イベント列を生成する。
//!
//! 時間も I/O も持たないため、同じシードと同じ入力からは必ず同じ結果になる。
//! このファイルはモジュール木の宣言のみを担う。編集しないこと。

pub mod invariant;
pub mod match_flow;
pub mod reaction;
pub mod round;
pub mod state;
pub mod wall;
