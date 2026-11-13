//! 输入法引擎：查询、排序与用户学习。

use crate::dict::{expand_partial_key, Dictionary, KeyKind};
use crate::syllable::{initial, is_syllable, segment, syllables_with_prefix};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 各匹配方式的权重（分层主键：全拼 > 部分音节 > 末字简拼 > 全简拼）。
/// 真实词频跨度极大（jieba 从 3 到 300 万+），乘性权重压不住跨层倒挂，
/// 故权重作为主排序键、词频仅在层内比较。
const W_FULL: u32 = 4;
const W_ABBREV_LAST: u32 = 2;
const W_PARTIAL: u32 = 3;
const W_INITIALS: u32 = 1;

/// 层内键：用户选词置顶（次主键），基础词频兜底（最低位）。
/// 排列：weight × 1e12 + min(user,9999) × 1e9 + min(freq, 999_999_999)。
fn layer_score(weight: u32, base_freq: u32, user_freq: u32) -> u64 {
    let w = weight as u64;
    let u = (user_freq.min(9_999) as u64) * 1_000_000_000;
    let f = (base_freq.min(999_999_999)) as u64;
    w * 1_000_000_000_000 + u + f
}

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
    pub score: u64,
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

        let mut scores: HashMap<u32, u64> = HashMap::new();
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

    /// 联想预测：返回以 `last_word` 末字开头的候选（不含该词本身），
    /// 用户选过的词置顶，其后按词频降序，最多 50 条。
    pub fn predict(&self, last_word: &str) -> Vec<Candidate> {
        let mut inner = self.inner.lock().expect("poisoned");
        let Some(dict) = inner.dict.as_ref() else {
            return Vec::new();
        };
        let Some(last) = last_word.chars().last() else {
            return Vec::new();
        };
        if !('\u{4E00}'..='\u{9FFF}').contains(&last) {
            return Vec::new();
        }

        let mut scored: Vec<(u32, u64)> = dict
            .predict_ids(last)
            .iter()
            .filter(|&&id| dict.entries[id as usize].word != last_word)
            .map(|&id| {
                let e = &dict.entries[id as usize];
                let user = inner.user_freq.get(&e.word).copied().unwrap_or(0);
                (id, layer_score(W_PARTIAL, e.base_freq, user))
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored.truncate(50);

        // 先克隆词表（结束 dict 借用），再更新学习状态
        let words: Vec<String> = scored
            .iter()
            .map(|&(id, _)| dict.entries[id as usize].word.clone())
            .collect();
        inner.last_results = scored.iter().map(|&(id, _)| id).collect();
        words
            .into_iter()
            .zip(scored.into_iter().map(|(_, score)| score))
            .map(|(word, score)| Candidate { word, score })
            .collect()
    }

    /// 生成带音节分隔的预编辑串（如 "nihao" -> "ni'hao"）。
    /// 撇号强制分段原样保留；无法切分的字母串原样返回。
    pub fn format_preedit(&self, raw: &str) -> String {
        let input = normalize(raw);
        if input.is_empty() {
            return String::new();
        }
        let mut segments: Vec<String> = Vec::new();
        for seg in input.split('\'').filter(|s| !s.is_empty()) {
            let path = segment(seg);
            let sylls = path.first().cloned().unwrap_or_default();
            if sylls.is_empty() {
                continue;
            }
            segments.push(sylls.join("'"));
        }
        segments.join("'")
    }
}

/// 对一条音节元素序列生成各类查询键并把命中结果并入 `scores`。
fn query_elements(
    dict: &Dictionary,
    user_freq: &HashMap<String, u32>,
    elems: &[String],
    scores: &mut HashMap<u32, u64>,
) {
    let k = elems.len();
    let last = &elems[k - 1];
    let last_valid = is_syllable(last);

    let add = |ids: &[u32], w: u32, scores: &mut HashMap<u32, u64>| {
        for &id in ids {
            let entry = &dict.entries[id as usize];
            let user = user_freq.get(&entry.word).copied().unwrap_or(0);
            let s = layer_score(w, entry.base_freq, user);
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
    fn select_invalid_index_returns_none() {
        let e = engine();
        assert!(e.select(0).is_none(), "未查询时 select 应返回 None");
        let cands = e.search("wo");
        let len = cands.len();
        assert!(e.select(len).is_none(), "越界序号应返回 None");
        assert!(e.select(1).is_some());

        let empty = QingciEngine::empty();
        assert!(empty.select(0).is_none());
    }

    #[test]
    fn user_freq_persists_across_engine_instances() {
        let dict = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join("qingci.dic");
        let user_path = user_file("persist");
        let _ = std::fs::remove_file(&user_path);

        // 实例 A：查询并选中，词频落盘
        let a = QingciEngine::new(&dict, &user_path).expect("load dict");
        let _ = a.search("nh");
        assert_eq!(a.select(0).as_deref(), Some("你好"));
        a.save();

        // 实例 B：加载同一用户词频文件，选中词应获得加权
        let b = QingciEngine::new(&dict, &user_path).expect("reload dict");
        let cands = b.search("nh");
        assert_eq!(cands[0].word, "你好");

        // 文件内容可被重新解析
        let text = std::fs::read_to_string(&user_path).expect("persisted");
        assert!(text.lines().any(|l| l.starts_with("你好\t")));
    }

    #[test]
    fn overlong_input_returns_empty() {
        let e = engine();
        let long_input = "a".repeat(51);
        assert!(e.search(&long_input).is_empty());
        // 长度恰在上限内则正常
        let ok_input = "a".repeat(20);
        let _ = e.search(&ok_input);
    }

    #[test]
    fn candidates_sorted_by_score_desc() {
        let e = engine();
        for input in ["wo", "nih", "wm", "shi"] {
            let cands = e.search(input);
            for w in cands.windows(2) {
                assert!(
                    w[0].score >= w[1].score,
                    "input={input}: {}:{} < {}:{}",
                    w[0].word,
                    w[0].score,
                    w[1].word,
                    w[1].score
                );
            }
        }
    }

    #[test]
    fn predict_by_last_char() {
        // 构造小型确定性词库验证联想
        let dir = std::env::temp_dir().join(format!(
            "qingci-predict-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let dict_path = dir.join("mini.dic");
        std::fs::write(
            &dict_path,
            "你好\t100\tni hao\n好感\t80\thao gan\n好的\t90\thao de\n\
             好人\t70\thao ren\n我们\t99\two men\n",
        )
        .unwrap();
        let user = user_file("predict");
        let _ = std::fs::remove_file(&user);
        let e = QingciEngine::new(&dict_path, &user).expect("load mini dict");

        // 以「好」开头的词按频次降序，不含「你好」本身
        let preds = e.predict("你好");
        let words: Vec<&str> = preds.iter().map(|c| c.word.as_str()).collect();
        assert_eq!(words, vec!["好的", "好感", "好人"]);
        // 联想结果可被学习
        assert_eq!(e.select(0).as_deref(), Some("好的"));
        // 非中文末字返回空
        assert!(e.predict("").is_empty());
        assert!(e.predict("abc").is_empty());
        assert!(e.predict("我们").is_empty(), "们开头无词");
    }

    #[test]
    fn format_preedit_segments() {
        let e = engine();
        assert_eq!(e.format_preedit("nihao"), "ni'hao");
        assert_eq!(e.format_preedit("xi'an"), "xi'an");
        assert_eq!(e.format_preedit("nh"), "nh");
        assert_eq!(e.format_preedit("niha"), "ni'ha");
        assert_eq!(e.format_preedit("NIHAO"), "ni'hao");
        assert_eq!(e.format_preedit(""), "");
        assert_eq!(e.format_preedit("wo'ai"), "wo'ai");
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
