//! 词库生成器：将第三方开源拼音数据转换为青瓷…清辞拼音词库格式。
//!
//! 输入（均为 MIT 许可来源）：
//! 1. jieba 词典（word freq tag）——提供真实词频
//! 2. mozillazg/phrase-pinyin-data（词组: 带调拼音）——词组拼音
//! 3. mozillazg/pinyin-data（U+XXXX: 带调读音列表）——单字拼音
//!
//! 输出：`词条<TAB>词频<TAB>音节1 音节2 ...`（词频降序）
//!
//! 用法：qingdict-gen <jieba.txt> <phrases.txt> <chars.txt> <out.dic>

use qingci_core::syllable::is_syllable;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 仅保留常用 CJK 统一表意文字（BMP 基本区）。
fn is_common_cjk(c: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&c)
}

/// 去除声调符号，ü 系列归一为 v。
fn strip_tone(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let mapped = match c {
            'ā' | 'á' | 'ǎ' | 'à' => 'a',
            'ō' | 'ó' | 'ǒ' | 'ò' => 'o',
            'ē' | 'é' | 'ě' | 'è' | 'ê' | 'ế' | 'ề' | 'ể' | 'ệ' => 'e',
            'ī' | 'í' | 'ǐ' | 'ì' => 'i',
            'ū' | 'ú' | 'ǔ' | 'ù' => 'u',
            'ǖ' | 'ǘ' | 'ǚ' | 'ǜ' | 'ü' => 'v',
            'ń' | 'ň' | 'ǹ' => 'n',
            '\u{1E3F}' => 'm', // ḿ（组合符号已由后续分支剥离）
            c if ('\u{0300}'..='\u{036F}').contains(&c) => continue, // 组合附加符号
            c => c,
        };
        out.push(mapped);
    }
    out
}

/// 解析 jieba 词典：word freq [tag]。
fn parse_jieba(path: &Path) -> Result<HashMap<String, u32>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("读取 {} 失败: {}", path.display(), e))?;
    let mut map = HashMap::with_capacity(400_000);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        // word 可能含空格（极少）：末两段为 freq 与词性
        let (word, freq) = if parts.len() >= 3 {
            (
                parts[..parts.len() - 2].join(" "),
                parts[parts.len() - 2],
            )
        } else {
            (parts[0].to_string(), parts[1])
        };
        if let Ok(f) = freq.parse::<u32>() {
            map.insert(word, f);
        }
    }
    Ok(map)
}

/// 解析单字读音：U+XXXX: 读1,读2 # 字。
fn parse_chars(path: &Path) -> Result<HashMap<char, Vec<String>>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("读取 {} 失败: {}", path.display(), e))?;
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((cp_part, rest)) = line.split_once(": ") else {
            continue;
        };
        if !cp_part.starts_with("U+") {
            continue;
        }
        let Ok(cp) = u32::from_str_radix(&cp_part[2..], 16) else {
            continue;
        };
        let Some(c) = char::from_u32(cp) else { continue };
        if !is_common_cjk(c) {
            continue;
        }
        // 去掉注释，取第一个合法读音
        let readings = rest.split('#').next().unwrap_or("");
        let mut valid = Vec::new();
        for r in readings.split(',') {
            let s = strip_tone(r.trim());
            if is_syllable(&s) {
                valid.push(s);
            }
        }
        if !valid.is_empty() {
            map.insert(c, valid);
        }
    }
    Ok(map)
}

/// 品牌词种子：输入法自身名称须在对应拼音下优先（外部数据不含）。
const SEEDS: &[(&str, u32, &str)] = &[("清辞", 900_000, "qing ci")];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        eprintln!(
            "用法: qingdict-gen <jieba.txt> <phrases.txt> <chars.txt> <out.dic>"
        );
        std::process::exit(2);
    }
    let jieba_path = PathBuf::from(&args[1]);
    let phrases_path = PathBuf::from(&args[2]);
    let chars_path = PathBuf::from(&args[3]);
    let out_path = PathBuf::from(&args[4]);

    let jieba = parse_jieba(&jieba_path).unwrap_or_else(|e| {
        eprintln!("错误: {e}");
        std::process::exit(1);
    });
    let chars = parse_chars(&chars_path).unwrap_or_else(|e| {
        eprintln!("错误: {e}");
        std::process::exit(1);
    });
    eprintln!(
        "jieba 词频 {} 条；单字读音 {} 字",
        jieba.len(),
        chars.len()
    );

    let phrase_text = std::fs::read_to_string(&phrases_path).unwrap_or_else(|e| {
        eprintln!("错误: 读取 {} 失败: {}", phrases_path.display(), e);
        std::process::exit(1);
    });

    // (词, 频, 音节)
    let mut entries: Vec<(String, u32, Vec<String>)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut skipped_syl = 0usize;
    let mut skipped_non_cjk = 0usize;

    for line in phrase_text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((word, py)) = line.split_once(": ") else {
            continue;
        };
        let word = word.trim();
        let n_chars = word.chars().count();
        if !(2..=8).contains(&n_chars) {
            continue;
        }
        if !word.chars().all(is_common_cjk) {
            skipped_non_cjk += 1;
            continue;
        }
        let syls: Vec<String> =
            py.split_whitespace().map(strip_tone).collect();
        if syls.len() != n_chars || syls.iter().any(|s| !is_syllable(s)) {
            skipped_syl += 1;
            continue;
        }
        let key = format!("{}\t{}", word, syls.join(" "));
        if !seen.insert(key) {
            continue;
        }
        // 无 jieba 词频的多为多音词次要读音变体，置于真实低频词之下
        let freq = jieba.get(word).copied().unwrap_or(50);
        entries.push((word.to_string(), freq, syls));
    }
    let phrase_count = entries.len();
    let phrase_no_freq = entries.iter().filter(|(_, f, _)| *f == 50).count();

    // 单字：jieba 字频 + pinyin-data 首选合法读音
    for (word, &freq) in &jieba {
        let mut it = word.chars();
        let (Some(c), None) = (it.next(), it.next()) else {
            continue;
        };
        if !is_common_cjk(c) {
            continue;
        }
        let Some(readings) = chars.get(&c) else {
            continue;
        };
        let Some(p) = readings.first() else { continue };
        let key = format!("{}\t{}", word, p);
        if !seen.insert(key) {
            continue;
        }
        entries.push((word.clone(), freq, vec![p.clone()]));
    }
    let char_count = entries.len() - phrase_count;

    // 注入品牌词种子：已存在同音节词条则覆盖词频，否则新增
    for (word, freq, py) in SEEDS {
        let syls: Vec<String> = py.split_whitespace().map(str::to_string).collect();
        let key = format!("{}\t{}", word, syls.join(" "));
        if let Some(e) = entries
            .iter_mut()
            .find(|(w, _, s)| w == word && *s == syls)
        {
            e.1 = *freq;
        } else if seen.insert(key) {
            entries.push((word.to_string(), *freq, syls));
        }
    }

    // 频次降序，同频按词序稳定排序
    entries.sort_by(|a, b| {
        b.1.cmp(&a.1).then(a.0.cmp(&b.0))
    });

    let mut out = String::with_capacity(entries.len() * 24);
    out.push_str("# 清辞拼音全量词库（自动生成，勿手改）\n");
    out.push_str("# 格式：词条<TAB>词频<TAB>音节1 音节2 ...\n");
    out.push_str("# 生成：cargo run --bin qingdict-gen（见 README「词库数据来源」）\n");
    for (word, freq, syls) in &entries {
        out.push_str(&format!("{}\t{}\t{}\n", word, freq, syls.join(" ")));
    }
    std::fs::write(&out_path, out).unwrap_or_else(|e| {
        eprintln!("错误: 写入 {} 失败: {}", out_path.display(), e);
        std::process::exit(1);
    });

    eprintln!(
        "完成: 共 {} 条（词组 {} 条，其中无词频数据 {} 条；单字 {} 条）\
         ；跳过非法音节行 {}、非常用 CJK 行 {}",
        entries.len(),
        phrase_count,
        phrase_no_freq,
        char_count,
        skipped_syl,
        skipped_non_cjk
    );
}
