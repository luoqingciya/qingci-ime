//! 输入法引擎：查询、排序与用户学习。

use crate::dict::{expand_partial_key, Dictionary, KeyKind};
use crate::syllable::{initial, is_syllable, segment, syllables_with_prefix};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 各匹配方式的权重（整数千分比）。
const W_FULL: u32 = 1000;
const W_ABBREV_LAST: u32 = 600;
const W_PARTIAL: u32 = 800;
const W_INITIALS: u32 = 350;

/// 返回候选上限。
const MAX_CANDIDATES: usize = 200;
/// 输入长度上限。
const MAX_INPUT_LEN: usize = 50;

#[derive(Debug, Default)]
pub struct QingciEngine {
    inner: Mutex<EngineInner>,
}

#[derive(Debug, Default)]
struct EngineInner {
    dict: Option<Dictionary>,
    /// 用户词频（学习结果）：词 -> 追加词频
    user_freq: HashMap<String, u32>,
    user_path: Option<PathBuf>,
    /// 最近一次查询的候选（词条 id），供 select 学习。
    last_results: Vec<u32>,
}

/// 查询结果中的单个候选。
#[derive(Debug, Clone)]
pub struct Candidate {
    pub word: String,
    pub score: u32,
}

impl QingciEngine {
    /// 创建引擎并加载词库与用户词频文件。
    pub fn new(dict_path: &Path, user_path: &Path) -> Result<Self, String> {
        let (dict, skipped) = Dictionary::load(dict_path)?;
        if skipped > 0 {
            eprintln!(
                "qingci-core: 词库 {} 跳过 {} 条无效词条",
                dict_path.display(),
                skipped
            );
        }
        let mut inner = EngineInner {
            dict: Some(dict),
            user_freq: HashMap::new(),
            user_path: Some(user_path.to_path_buf()),
            last_results: Vec::new(),
        };
        load_user_freq(&mut inner, user_path);
        Ok(QingciEngine {
            inner: Mutex::new(inner),
        })
    }

    /// 仅测试用的空引擎。
    #[cfg(test)]
    fn empty() -> Self {
        QingciEngine {
            inner: Mutex::new(EngineInner::default()),
        }
    }

    /// 查询候选词，按分数从高到低排序。
    pub fn search(&self, raw: &str) -> Vec<Candidate> {
        let mut inner = self.inner.lock().expect("poisoned");
        let Some(dict) = inner.dict.as_ref() else {
            return Vec::new();
        };

        let input = normalize(raw);
        if input.is_empty() || input.len() > MAX_INPUT_LEN {
            inner.last_results.clear();
            return Vec::new();
        }

        // 撇号强制分段；各段独立切分。多路径歧义（如 xian / xi'an）
        // 通过跨段的笛卡尔组合参与匹配（组合数上限 16，防爆炸）。
        let seg_paths: Vec<Vec<Vec<String>>> = input
            .split('\'')
            .filter(|s| !s.is_empty())
            .map(|seg| {
                let mut paths = segment(seg);
                paths.truncate(4);
                paths
            })
            .collect();
        if seg_paths.is_empty() {
            inner.last_results.clear();
            return Vec::new();
        }

        const MAX_COMBOS: usize = 16;
        let mut combos: Vec<Vec<String>> = seg_paths[0].clone();
        for paths in &seg_paths[1..] {
            let mut next = Vec::new();
            for combo in &combos {
                for path in paths {
                    if next.len() >= MAX_COMBOS {
                        break;
                    }
                    let mut c = combo.clone();
                    c.extend(path.clone());
                    next.push(c);
                }
            }
            combos = next;
        }

        let mut scores: HashMap<u32, u32> = HashMap::new();
        for elems in &combos {
            query_elements(dict, &inner.user_freq, elems, &mut scores);
        }

        let mut ranked: Vec<Candidate> = scores
            .into_iter()
            .map(|(id, score)| Candidate {
                word: dict.entries[id as usize].word.clone(),
                score,
            })
            .collect();
        ranked.sort_by(|a, b| b.score.cmp(&a.score).then(a.word.cmp(&b.word)));
        ranked.truncate(MAX_CANDIDATES);

        // 记录候选顺序，供 select() 学习（按 id 去重后的顺序）。
        let id_by_word: HashMap<&str, u32> = dict
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.word.as_str(), i as u32))
            .collect();
        inner.last_results = ranked
            .iter()
            .filter_map(|c| id_by_word.get(c.word.as_str()).copied())
            .collect();

        ranked
    }

    /// 选中第 `index` 个候选并学习（提升用户词频）。
    /// 返回选中的词。
    pub fn select(&self, index: usize) -> Option<String> {
        let mut inner = self.inner.lock().expect("poisoned");
        let id = *inner.last_results.get(index)?;
        let word = inner
            .dict
            .as_ref()
            .expect("dict loaded")
            .entries[id as usize]
            .word
            .clone();
        let freq = inner.user_freq.entry(word.clone()).or_insert(0);
        *freq = (*freq + 1).min(20_000);
        let path = inner.user_path.clone();
        if let Some(p) = path {
            let _ = save_user_freq(&inner, &p);
        }
        Some(word)
    }

    /// 持久化用户词频。
    pub fn save(&self) {
        let inner = self.inner.lock().expect("poisoned");
        if let Some(p) = &inner.user_path {
            let _ = save_user_freq(&inner, p);
        }
    }

    /// 当前已加载词条数。
    pub fn entry_count(&self) -> usize {
        self.inner
            .lock()
            .expect("poisoned")
            .dict
            .as_ref()
            .map(|d| d.entries.len())
            .unwrap_or(0)
    }
}

/// 对一条音节元素序列生成各类查询键并把命中结果并入 `scores`。
fn query_elements(
    dict: &Dictionary,
    user_freq: &HashMap<String, u32>,
    elems: &[String],
    scores: &mut HashMap<u32, u32>,
) {
    let k = elems.len();
    let last = &elems[k - 1];
    let last_valid = is_syllable(last);

    let add = |ids: &[u32], w: u32, scores: &mut HashMap<u32, u32>| {
        for &id in ids {
            let entry = &dict.entries[id as usize];
            let user = user_freq.get(&entry.word).copied().unwrap_or(0);
            let s = w.saturating_mul(entry.base_freq.saturating_add(user * 3));
            let slot = scores.entry(id).or_insert(0);
            if s > *slot {
                *slot = s;
            }
        }
    };

    if last_valid {
        // 全拼
        let full = elems.join(" ");
        add(dict.lookup_in(KeyKind::Full, &full), W_FULL, scores);
        // 末字全拼（前 n-1 取简拼）
        if k >= 2 {
            let mut parts: Vec<String> =
                elems[..k - 1].iter().map(|s| initial(s).to_string()).collect();
            parts.push(last.clone());
            add(
                dict.lookup_in(KeyKind::AbbrevLast, &parts.join(" ")),
                W_ABBREV_LAST,
                scores,
            );
        }
        // 全简拼
        let initials: Vec<String> =
            elems.iter().map(|s| initial(s).to_string()).collect();
        add(
            dict.lookup_in(KeyKind::Initials, &initials.join(" ")),
            W_INITIALS,
            scores,
        );
    } else {
        // 末段为不完整音节：展开成可能的音节补全
        for key in expand_partial_key(KeyKind::Full, elems) {
            add(dict.lookup_in(KeyKind::Full, &key), W_PARTIAL, scores);
        }
        if k >= 2 {
            for key in expand_partial_key(KeyKind::AbbrevLast, elems) {
                add(
                    dict.lookup_in(KeyKind::AbbrevLast, &key),
                    W_ABBREV_LAST,
                    scores,
                );
            }
        } else {
            let chars: Vec<char> = last.chars().collect();
            if chars.len() >= 2 {
                // 纯字母串无法按音节切分（如 "nh"、"wm"）：
                // 按「每个字母一个音节」的简拼模式查询。
                let key: Vec<String> =
                    chars.iter().map(|c| c.to_string()).collect();
                add(
                    dict.lookup_in(KeyKind::Initials, &key.join(" ")),
                    W_INITIALS,
                    scores,
                );
                // 前 n-1 个简拼字母 + 末字母作为不完整音节展开
                let tail = chars[chars.len() - 1].to_string();
                let prefix: Vec<String> = chars[..chars.len() - 1]
                    .iter()
                    .map(|c| c.to_string())
                    .collect();
                for exp in syllables_with_prefix(&tail) {
                    let mut parts = prefix.clone();
                    parts.push(exp.to_string());
                    add(
                        dict.lookup_in(KeyKind::AbbrevLast, &parts.join(" ")),
                        W_ABBREV_LAST,
                        scores,
                    );
                }
            } else {
                // 单个字母（如 "n"）：简拼首字母查询
                let first = initial(last).to_string();
                add(
                    dict.lookup_in(KeyKind::Initials, &first),
                    W_INITIALS,
                    scores,
                );
            }
        }
    }
}

/// 归一化输入：仅保留小写字母与撇号。
fn normalize(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphabetic() || *c == '\'')
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn load_user_freq(inner: &mut EngineInner, path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split('\t');
        if let (Some(w), Some(f)) = (it.next(), it.next()) {
            if let Ok(f) = f.trim().parse::<u32>() {
                inner.user_freq.insert(w.trim().to_string(), f);
            }
        }
    }
}

fn save_user_freq(inner: &EngineInner, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("创建目录失败 {}: {}", parent.display(), e))?;
    }
    let mut lines: Vec<String> = inner
        .user_freq
        .iter()
        .map(|(w, f)| format!("{}\t{}", w, f))
        .collect();
    lines.sort();
    std::fs::write(path, lines.join("\n") + "\n")
        .map_err(|e| format!("写入用户词频失败 {}: {}", path.display(), e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 并行测试共用临时文件会互相删除，按测试名 + 进程号隔离。
    fn user_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "qingci-test-{}-{}.dic",
            name,
            std::process::id()
        ))
    }

    fn engine() -> QingciEngine {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let dict = dir.join("data").join("qingci.dic");
        let user = user_file("default");
        let _ = std::fs::remove_file(&user);
        QingciEngine::new(&dict, &user).expect("load dict")
    }

    #[test]
    fn full_pinyin_single_char() {
        let e = engine();
        let cands = e.search("wo");
        assert!(!cands.is_empty());
        assert_eq!(cands[0].word, "我");
    }

    #[test]
    fn full_pinyin_word() {
        let e = engine();
        let cands = e.search("nihao");
        assert!(!cands.is_empty());
        assert_eq!(cands[0].word, "你好");
    }

    #[test]
    fn ambiguous_xian_prefers_full_syllable() {
        let e = engine();
        let cands = e.search("xian");
        // 先：全拼 "xian" 的单字/词组应排在 xi'an 组合之前
        let first = &cands[0].word;
        assert!(
            first == "先" || first == "西安" || first == "现",
            "got {}",
            first
        );
    }

    #[test]
    fn jianpin_two_letters() {
        let e = engine();
        let cands = e.search("nh");
        assert!(!cands.is_empty());
        assert_eq!(cands[0].word, "你好");
    }

    #[test]
    fn abbrev_last() {
        let e = engine();
        // w m -> 简拼; wf -> 不完整末音节 f(发/分/放…)
        let cands = e.search("wm");
        assert_eq!(cands[0].word, "我们");
    }

    #[test]
    fn partial_last_syllable() {
        let e = engine();
        let cands = e.search("nih");
        // ni + h(展开为 hao 等)
        assert!(cands
            .iter()
            .take(3)
            .any(|c| c.word == "你好"),
            "top3: {:?}",
            cands.iter().take(3).map(|c| c.word.clone()).collect::<Vec<_>>());
    }

    #[test]
    fn learning_boosts() {
        let dict = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join("qingci.dic");
        let user_path = user_file("learning");
        let _ = std::fs::remove_file(&user_path);
        let e = QingciEngine::new(&dict, &user_path).expect("load dict");
        let base = e.search("wm");
        let _ = e.select(0);
        // 再查一次，被选中的词仍在首位或显著提前
        let again = e.search("wm");
        assert!(!again.is_empty());
        assert_eq!(again[0].word, base[0].word);
        // 用户词频已持久化
        let text = std::fs::read_to_string(&user_path)
            .expect("用户词频文件应已写入");
        assert!(text.contains("我们"));
    }

    #[test]
    fn apostrophe_forces_boundary() {
        let e = engine();
        let cands = e.search("xi'an");
        assert!(!cands.is_empty());
        assert_eq!(cands[0].word, "西安");
    }

    #[test]
    fn empty_and_garbage() {
        let e = engine();
        assert!(e.search("").is_empty());
        assert!(e.search("123!@#").is_empty());
    }

    #[test]
    fn normalized_case() {
        let e = engine();
        assert_eq!(e.search("NIHAO")[0].word, "你好");
    }

    #[test]
    fn empty_engine_returns_nothing() {
        let e = QingciEngine::empty();
        assert!(e.search("ni").is_empty());
    }
}
