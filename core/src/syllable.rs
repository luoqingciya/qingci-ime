//! 拼音音节表与切分。
//!
//! 提供无调标准普通话音节全集（约 380 个），以及把连续小写字母串
//! 切分成音节序列的多路径 DP 枚举（用于处理 xian / xi'an 这类歧义）。
//! 切分失败时，剩余部分作为“不完整音节”（partial）保留。

/// 无调拼音音节表（按声母分组排列，便于人工校对）。
pub const SYLLABLES: &[&str] = &[
    // 零声母
    "a", "o", "e", "ai", "ei", "ao", "ou", "an", "en", "ang", "eng", "er",
    // b
    "ba", "bo", "bai", "bei", "bao", "ban", "ben", "bang", "beng", "bi",
    "bie", "biao", "bian", "bin", "bing", "bu",
    // p
    "pa", "po", "pai", "pei", "pao", "pou", "pan", "pen", "pang", "peng",
    "pi", "pie", "piao", "pian", "pin", "ping", "pu",
    // m
    "ma", "mo", "me", "mai", "mei", "mao", "mou", "man", "men", "mang",
    "meng", "mi", "mie", "miao", "miu", "mian", "min", "ming", "mu",
    // f
    "fa", "fo", "fei", "fou", "fan", "fen", "fang", "feng", "fu",
    // d
    "da", "de", "dai", "dei", "den", "dia", "dao", "dou", "dan", "dang",
    "deng", "dong", "di", "die", "diao", "diu", "dian", "ding", "du", "duo",
    "dui", "duan", "dun",
    // t
    "ta", "te", "tai", "tao", "tou", "tan", "tang", "teng", "tong", "ti",
    "tie", "tiao", "tian", "ting", "tu", "tuo", "tui", "tuan", "tun", "tei",
    // n
    "na", "nai", "nei", "ne", "nao", "nan", "nen", "nang", "neng", "nong",
    "ni", "nie", "niao", "niu", "nian", "nin", "niang", "ning", "nu", "nuo",
    "nuan", "nv", "nve", "lo",
    // l
    "la", "le", "lai", "lei", "lao", "lou", "lan", "lang", "leng", "long",
    "li", "lia", "liao", "liu", "lian", "lin", "liang", "ling", "lu", "luo",
    "luan", "lun", "lv", "lve",
    // g
    "ga", "ge", "gai", "gei", "gao", "gou", "gan", "gen", "gang", "geng",
    "gong", "gu", "gua", "guo", "guai", "gui", "guan", "gun", "guang",
    // k
    "ka", "ke", "kai", "kei", "kao", "kou", "kan", "ken", "kang", "keng",
    "kong", "ku", "kua", "kuo", "kuai", "kui", "kuan", "kun", "kuang",
    // h
    "ha", "he", "hai", "hei", "hao", "hou", "han", "hen", "hang", "heng",
    "hong", "hu", "hua", "huo", "huai", "hui", "huan", "hun", "huang",
    // j
    "ji", "jia", "jie", "jiao", "jiu", "jian", "jin", "jiang", "jing", "ju",
    "jue", "juan", "jun", "jiong",
    // q
    "qi", "qia", "qie", "qiao", "qiu", "qian", "qin", "qiang", "qing", "qu",
    "que", "quan", "qun", "qiong",
    // x
    "xi", "xia", "xie", "xiao", "xiu", "xian", "xin", "xiang", "xing", "xu",
    "xue", "xuan", "xun", "xiong",
    // zh
    "zha", "zhe", "zhi", "zhai", "zhei", "zhao", "zhou", "zhan", "zhen",
    "zhang", "zheng", "zhong", "zhu", "zhua", "zhuo", "zhuai", "zhui",
    "zhuan", "zhun", "zhuang",
    // ch
    "cha", "che", "chi", "chai", "chao", "chou", "chan", "chen", "chang",
    "cheng", "chong", "chu", "chua", "chuo", "chuai", "chui", "chuan",
    "chun", "chuang",
    // sh
    "sha", "she", "shi", "shai", "shei", "shao", "shou", "shan", "shen",
    "shang", "sheng", "shu", "shua", "shuo", "shuai", "shui", "shuan",
    "shun", "shuang",
    // r
    "re", "ri", "rao", "rou", "ran", "ren", "rang", "reng", "rong", "ru",
    "rua", "ruo", "rui", "ruan", "run",
    // z
    "za", "ze", "zi", "zai", "zei", "zao", "zou", "zan", "zen", "zang",
    "zeng", "zong", "zu", "zuo", "zui", "zuan", "zun",
    // c
    "ca", "ce", "ci", "cai", "cao", "cou", "can", "cen", "cang", "ceng",
    "cong", "cu", "cuo", "cui", "cuan", "cun",
    // s
    "sa", "se", "si", "sai", "sao", "sou", "san", "sen", "sang", "seng",
    "song", "su", "suo", "sui", "suan", "sun",
    // y
    "ya", "yo", "ye", "yao", "you", "yan", "yin", "yang", "ying", "yu",
    "yue", "yuan", "yun", "yong", "yi",
    // w
    "wa", "wo", "wai", "wei", "wan", "wen", "wang", "weng", "wu",
];

/// 音节最长长度（zhuang = 6）。
const MAX_SYLLABLE_LEN: usize = 6;
/// 单次切分枚举的路径上限，防止指数爆炸。
const MAX_PATHS: usize = 8;

use std::collections::HashSet;
use std::sync::OnceLock;

fn syllable_set() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| SYLLABLES.iter().copied().collect())
}

/// 判断字符串是否为合法音节。
pub fn is_syllable(s: &str) -> bool {
    syllable_set().contains(s)
}

/// 返回所有以 `prefix` 开头的音节（用于不完整输入展开）。
pub fn syllables_with_prefix(prefix: &str) -> Vec<&'static str> {
    SYLLABLES
        .iter()
        .filter(|s| s.starts_with(prefix))
        .copied()
        .collect()
}

/// 取音节的简拼首字母（zh/ch/sh 也按首字符处理，与主流输入法一致）。
pub fn initial(s: &str) -> char {
    s.chars().next().unwrap_or('\0')
}

/// 把一段无分隔符的字母串切分为音节序列，返回最多 `cap` 条路径。
///
/// 若某个位置没有任何合法音节前缀，则从该位置到结尾整体作为一个
/// “不完整音节”元素返回（例如 "niha" -> [["ni", "ha"]]）。
pub fn segment(input: &str) -> Vec<Vec<String>> {
    let bytes = input.as_bytes();
    let mut memo: std::collections::HashMap<usize, Vec<Vec<String>>> =
        std::collections::HashMap::new();
    let paths = rec(bytes, 0, &mut memo);
    if paths.is_empty() {
        return vec![vec![input.to_string()]];
    }
    paths
}

fn rec(
    bytes: &[u8],
    pos: usize,
    memo: &mut std::collections::HashMap<usize, Vec<Vec<String>>>,
) -> Vec<Vec<String>> {
    if pos >= bytes.len() {
        return vec![Vec::new()];
    }
    if let Some(hit) = memo.get(&pos) {
        return hit.clone();
    }
    let mut out: Vec<Vec<String>> = Vec::new();
    let max_len = MAX_SYLLABLE_LEN.min(bytes.len() - pos);
    for len in (1..=max_len).rev() {
        let piece = std::str::from_utf8(&bytes[pos..pos + len]).expect("ascii only");
        if !is_syllable(piece) {
            continue;
        }
        for rest in rec(bytes, pos + len, memo) {
            if out.len() >= MAX_PATHS {
                break;
            }
            let mut path = Vec::with_capacity(rest.len() + 1);
            path.push(piece.to_string());
            path.extend(rest);
            out.push(path);
        }
    }
    // 该位置起没有任何合法音节：剩余部分整体作为不完整音节。
    if out.is_empty() {
        let tail = std::str::from_utf8(&bytes[pos..]).expect("ascii only").to_string();
        out.push(vec![tail]);
    }
    memo.insert(pos, out.clone());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syllable_table_sanity() {
        assert!(is_syllable("ni"));
        assert!(is_syllable("hao"));
        assert!(is_syllable("zhuang"));
        assert!(is_syllable("a"));
        assert!(!is_syllable("i"));
        assert!(!is_syllable("nihao"));
        assert!(!is_syllable("zh"));
        // 表内不允许重复
        let set: HashSet<_> = SYLLABLES.iter().collect();
        assert_eq!(set.len(), SYLLABLES.len());
    }

    #[test]
    fn segment_simple() {
        let paths = segment("nihao");
        assert!(paths.contains(&vec!["ni".to_string(), "hao".to_string()]));
    }

    #[test]
    fn segment_ambiguous() {
        let paths = segment("xian");
        assert!(paths.contains(&vec!["xian".to_string()]));
        assert!(paths.contains(&vec!["xi".to_string(), "an".to_string()]));
    }

    #[test]
    fn segment_partial_tail() {
        let paths = segment("niha");
        // "ha" 不是合法音节结尾的前缀组合: ni + ha(不完整)
        assert!(paths.iter().any(|p| p == &vec!["ni".to_string(), "ha".to_string()]));
    }

    #[test]
    fn segment_jianpin_letters() {
        // 全部为单字母且非音节时，整体作为不完整音节
        let paths = segment("nh");
        assert_eq!(paths, vec![vec!["nh".to_string()]]);
    }

    #[test]
    fn segment_single_zero_initial_syllable() {
        // 首条路径为最长切分；随后可能带不完整音节路径（多路径枚举）
        assert_eq!(segment("a"), vec![vec!["a".to_string()]]);
        assert_eq!(segment("er").first().unwrap(), &vec!["er".to_string()]);
    }

    #[test]
    fn initial_takes_first_char() {
        // zh/ch/sh 也按首字符处理，与主流输入法简拼一致
        assert_eq!(initial("shi"), 's');
        assert_eq!(initial("zhuang"), 'z');
        assert_eq!(initial("ni"), 'n');
        assert_eq!(initial("a"), 'a');
    }

    #[test]
    fn prefix_expansions_all_start_with_prefix() {
        let exps = syllables_with_prefix("zh");
        assert!(!exps.is_empty());
        assert!(exps.contains(&"zhuang"));
        assert!(exps.iter().all(|s| s.starts_with("zh")));
        // 不存在的前缀返回空
        assert!(syllables_with_prefix("xx").is_empty());
    }

    #[test]
    fn segment_long_input_terminates() {
        // 多个音节连打：应能完整切分且路径数有限（输入须为小写，
        // 大小写归一化由引擎层负责）
        let paths = segment("woaiBeijingtiananmen".to_lowercase().as_str());
        assert!(paths.iter().any(|p| p.len() >= 7));
    }
}
