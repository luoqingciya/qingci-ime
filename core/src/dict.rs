//! 词典加载与倒排索引。
//!
//! 词库文件格式（每行一条，`#` 开头为注释，UTF-8）：
//! ```text
//! 词条<TAB>基础词频<TAB>音节1 音节2 ...
//! ```
//! 例：`你好	999999	ni hao`
//!
//! 为每个词条建立三类倒排键：
//! - 全拼键：`ni hao`（完整匹配）
//! - 末字全拼键：`n hao`（前 n-1 个音节取简拼，最后一个全拼）
//! - 全简拼键：`n h`（所有音节取简拼）

use crate::syllable::{initial, is_syllable, syllables_with_prefix};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct DictEntry {
    pub word: String,
    pub syllables: Vec<String>,
    pub base_freq: u32,
}

#[derive(Debug, Default)]
pub struct Dictionary {
    pub entries: Vec<DictEntry>,
    /// 全拼键 -> 词条 id 列表
    full_map: HashMap<String, Vec<u32>>,
    /// 末字全拼键 -> 词条 id 列表
    abbrev_last_map: HashMap<String, Vec<u32>>,
    /// 全简拼键 -> 词条 id 列表
    initials_map: HashMap<String, Vec<u32>>,
    /// 首字 -> 词条 id 列表（联想预测用）
    first_char_map: HashMap<char, Vec<u32>>,
}

impl Dictionary {
    /// 从文件加载词库。音节不合法的行会被跳过并计数。
    pub fn load(path: &Path) -> Result<(Dictionary, usize), String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("无法读取词库 {}: {}", path.display(), e))?;
        let (dict, skipped) = Dictionary::parse(&text);
        if dict.entries.is_empty() {
            return Err(format!("词库 {} 中没有任何有效词条", path.display()));
        }
        Ok((dict, skipped))
    }

    /// 解析词库文本并建立索引。
    pub fn parse(text: &str) -> (Dictionary, usize) {
        let mut dict = Dictionary::default();
        let mut skipped = 0usize;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split('\t');
            let (Some(word), Some(freq), Some(syls)) =
                (parts.next(), parts.next(), parts.next())
            else {
                skipped += 1;
                continue;
            };
            let word = word.trim().to_string();
            if word.is_empty() {
                skipped += 1;
                continue;
            }
            let base_freq = match freq.trim().parse::<u32>() {
                Ok(f) => f,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            let syllables: Vec<String> =
                syls.split_whitespace().map(|s| s.to_string()).collect();
            if syllables.is_empty()
                || syllables.len() > 8
                || syllables.iter().any(|s| !is_syllable(s))
            {
                skipped += 1;
                continue;
            }
            let id = dict.entries.len() as u32;
            let entry = DictEntry {
                word,
                syllables,
                base_freq,
            };
            index_entry(&mut dict, id, &entry);
            dict.entries.push(entry);
        }
        (dict, skipped)
    }

    /// 按键查询词条 id 列表。
    pub fn lookup(&self, key: &str) -> &[u32] {
        static EMPTY: Vec<u32> = Vec::new();
        self.full_map
            .get(key)
            .or_else(|| self.abbrev_last_map.get(key))
            .or_else(|| self.initials_map.get(key))
            .map(|v| v.as_slice())
            .unwrap_or(&EMPTY)
    }

    /// 显式按某一类键查询。
    pub fn lookup_in(
        &self,
        which: KeyKind,
        key: &str,
    ) -> &[u32] {
        static EMPTY: Vec<u32> = Vec::new();
        let map = match which {
            KeyKind::Full => &self.full_map,
            KeyKind::AbbrevLast => &self.abbrev_last_map,
            KeyKind::Initials => &self.initials_map,
        };
        map.get(key).map(|v| v.as_slice()).unwrap_or(&EMPTY)
    }

    /// 以某汉字开头的词条 id 列表（联想预测用）。
    pub fn predict_ids(&self, c: char) -> &[u32] {
        static EMPTY: Vec<u32> = Vec::new();
        self.first_char_map
            .get(&c)
            .map(|v| v.as_slice())
            .unwrap_or(&EMPTY)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum KeyKind {
    Full,
    AbbrevLast,
    Initials,
}

fn index_entry(dict: &mut Dictionary, id: u32, entry: &DictEntry) {
    let syls = &entry.syllables;
    let k = syls.len();

    // 首字倒排（联想预测）
    if let Some(first) = entry.word.chars().next() {
        if ('\u{4E00}'..='\u{9FFF}').contains(&first) {
            dict.first_char_map.entry(first).or_default().push(id);
        }
    }

    // 全拼键
    let full_key = syls.join(" ");
    dict.full_map.entry(full_key).or_default().push(id);

    // 全简拼键
    let initials_key: Vec<String> =
        syls.iter().map(|s| initial(s).to_string()).collect();
    dict.initials_map
        .entry(initials_key.join(" "))
        .or_default()
        .push(id);

    // 末字全拼键（前 k-1 个音节取首字母）
    if k >= 2 {
        let mut key_parts: Vec<String> = syls[..k - 1]
            .iter()
            .map(|s| initial(s).to_string())
            .collect();
        key_parts.push(syls[k - 1].clone());
        dict.abbrev_last_map
            .entry(key_parts.join(" "))
            .or_default()
            .push(id);
    }
}

/// 把“不完整音节”展开成所有可能的全拼键补全。
pub fn expand_partial_key(which: KeyKind, elems: &[String]) -> Vec<String> {
    let last = elems.last().cloned().unwrap_or_default();
    let expansions = syllables_with_prefix(&last);
    let mut keys = Vec::new();
    for exp in expansions {
        let mut parts: Vec<String> = elems[..elems.len() - 1].to_vec();
        match which {
            KeyKind::Full => parts.push(exp.to_string()),
            KeyKind::AbbrevLast => {
                for s in &mut parts {
                    *s = initial(s).to_string();
                }
                parts.push(exp.to_string());
            }
            KeyKind::Initials => continue, // 简拼键不依赖末字展开
        }
        keys.push(parts.join(" "));
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "你好\t999999\tni hao\n# 注释\n清辞\t5000\tqing ci\n";

    #[test]
    fn parse_and_index() {
        let (dict, skipped) = Dictionary::parse(SAMPLE);
        assert_eq!(skipped, 0);
        assert_eq!(dict.entries.len(), 2);
        // 全拼
        assert!(dict.lookup_in(KeyKind::Full, "ni hao").len() == 1);
        // 全简拼
        assert!(dict.lookup_in(KeyKind::Initials, "n h").len() == 1);
        // 末字全拼
        assert!(dict.lookup_in(KeyKind::AbbrevLast, "n hao").len() == 1);
        assert!(dict.lookup_in(KeyKind::AbbrevLast, "q ci").len() == 1);
    }

    #[test]
    fn parse_skips_invalid() {
        let (dict, skipped) =
            Dictionary::parse("坏词\t100\tabc xyz\n好词\t10\tni hao\n");
        assert_eq!(skipped, 1);
        assert_eq!(dict.entries.len(), 1);
    }

    #[test]
    fn expand_partial() {
        let keys = expand_partial_key(
            KeyKind::Full,
            &["ni".to_string(), "h".to_string()],
        );
        assert!(keys.contains(&"ni hao".to_string()));
        assert!(keys.contains(&"ni hai".to_string()));
        // 展开结果必须以 h 开头
        assert!(!keys.iter().any(|k| k.ends_with(" xi")));
        assert!(!keys.iter().any(|k| k.ends_with(" ge")));
    }

    #[test]
    fn load_builtin_dictionary() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
        let (dict, skipped) =
            Dictionary::load(&dir.join("qingci.dic")).expect("内置词库应可加载");
        // 内置词库应包含全部有效词条且无跳过行
        assert!(dict.entries.len() >= 600, "{}", dict.entries.len());
        assert_eq!(skipped, 0);
        // 单字与词组均可查到
        assert!(!dict.lookup_in(KeyKind::Full, "wo").is_empty());
        assert!(!dict.lookup_in(KeyKind::Full, "ni hao").is_empty());
    }

    #[test]
    fn load_full_dictionary_smoke() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
        let (dict, skipped) = Dictionary::load(&dir.join("qingci-full.dic"))
            .expect("全量词库应可加载");
        // 全量词库：约 42 万条，全部行合法
        assert!(dict.entries.len() >= 400_000, "{}", dict.entries.len());
        assert_eq!(skipped, 0);
        // 品牌词与常用词均可查到
        assert!(!dict.lookup_in(KeyKind::Full, "qing ci").is_empty());
        assert!(!dict.lookup_in(KeyKind::Full, "ni hao").is_empty());
        assert!(!dict.lookup_in(KeyKind::Initials, "w m").is_empty());
    }

    #[test]
    fn load_missing_file_errors() {
        let err = Dictionary::load(Path::new("/nonexistent/qingci.dic"))
            .expect_err("缺失文件应报错");
        assert!(err.contains("无法读取"));
    }

    #[test]
    fn lookup_falls_back_through_key_kinds() {
        let (dict, _) = Dictionary::parse("我们\t100\two men\n");
        // 全拼键
        assert_eq!(dict.lookup("wo men").len(), 1);
        // 全拼未命中 -> 末字全拼键
        assert_eq!(dict.lookup("w men").len(), 1);
        // 末字全拼未命中 -> 全简拼键
        assert_eq!(dict.lookup("w m").len(), 1);
        // 三种键都未命中
        assert!(dict.lookup("x y z").is_empty());
    }
}
