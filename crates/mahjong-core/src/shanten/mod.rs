//! 向聴数の計算。標準形・七対子・国士無双の3形を別ファイルに分ける。
//!
//! 規約: **-1 が和了、0 がテンパイ。** 標準形の最大は 8。
//! `overall` が3形の最小を返す。呼び出し側は原則これを使う。
//!
//! このファイルは編集しないこと（Wave 0 で確定済み）。

pub mod chiitoitsu;
pub mod kokushi;
pub mod overall;
pub mod standard;
