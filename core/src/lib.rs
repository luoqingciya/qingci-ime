//! 清辞拼音核心引擎。
//!
//! 与框架无关的中文（简体）拼音引擎，供 Fcitx5 插件通过 C ABI 调用，
//! 也可直接作为 Rust 库或 `qingcli` 命令行工具使用。
//!
//! 功能（MVP）：
//! - 全拼（多路径歧义切分，如 xian / xi'an）
//! - 简拼（每音节取首字母，如 nh -> 你好）
//! - 不完整末音节前缀匹配（如 nih -> 你好）
//! - 词频排序 + 用户词频学习（持久化）

pub mod dict;
pub mod engine;
pub mod ffi;
pub mod syllable;

pub use engine::{Candidate, QingciEngine};
