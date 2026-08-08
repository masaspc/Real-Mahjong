//! 純粋な麻雀判定。乱数・I/O・時間を一切持たない。
//!
//! このファイルはモジュール木の宣言のみを担う。**Wave 1 の作業では編集しないこと。**
//! 新しいファイルが必要になったらコーディネータへ報告する。
//!
//! ファイル所有権:
//! - `hand` / `shapes` … Wave 0（共有語彙、編集禁止）
//! - `shanten` / `wait` / `furiten` / `callable` … Wave 1a
//! - `decompose` / `yaku_check` / `fu` / `score` … Wave 1b

pub mod callable;
pub mod decompose;
pub mod fu;
pub mod furiten;
pub mod hand;
pub mod score;
pub mod shanten;
pub mod shapes;
pub mod wait;
pub mod yaku_check;
