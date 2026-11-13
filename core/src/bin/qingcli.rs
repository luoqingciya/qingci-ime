//! 命令行演示与调试工具。
//!
//! 用法：`qingcli [词库路径]`，随后每行输入一个拼音串，
//! 输出前 10 个候选。输入空行或 Ctrl-D 退出。

use qingci_core::QingciEngine;
use std::io::{BufRead, Write};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let dict = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(manifest_dir).join("data").join("qingci.dic"));
    let user = args
        .get(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("qingcli-user.dic"));

    let engine = match QingciEngine::new(&dict, &user) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("错误：{}", err);
            std::process::exit(1);
        }
    };
    eprintln!(
        "青瓷拼音核心引擎演示（词条 {} 条）。输入拼音后回车查询，空行退出。",
        engine.entry_count()
    );

    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim().to_string();
        if line.is_empty() {
            break;
        }
        let candidates = engine.search(&line);
        if candidates.is_empty() {
            writeln!(out, "（无候选）").unwrap();
            continue;
        }
        for (i, c) in candidates.iter().take(10).enumerate() {
            write!(out, "{}.{} ", i + 1, c.word).unwrap();
        }
        writeln!(out).unwrap();
        out.flush().unwrap();
        // 模拟选词学习：选中第 1 项
        let _ = engine.select(0);
    }
    engine.save();
}
