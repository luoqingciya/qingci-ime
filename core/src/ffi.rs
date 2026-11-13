//! Fcitx5 插件使用的 C ABI。
//!
//! 候选词列表通过 `QingciResults`（UTF-8 字符串数组）返回，
//! 由调用方在使用完毕后调用 `qingci_results_free` 释放。
//! 选中学过的候选词状态保存在引擎内部（`qingci_engine_select` 按最近
//! 一次查询的候选序号学习）。

use crate::engine::QingciEngine;
use std::ffi::{c_char, c_int, CStr, CString};
use std::ptr;

/// 候选词结果（C 视角：`texts` 指向 `count` 个以 NUL 结尾的 UTF-8 字符串）。
#[repr(C)]
pub struct QingciResults {
    pub texts: *mut *mut c_char,
    pub count: c_int,
}

impl Default for QingciResults {
    fn default() -> Self {
        QingciResults {
            texts: ptr::null_mut(),
            count: 0,
        }
    }
}

/// 创建引擎。`dict_path` / `user_path` 均为 UTF-8 路径。
/// 失败返回空指针。
#[no_mangle]
pub extern "C" fn qingci_engine_new(
    dict_path: *const c_char,
    user_path: *const c_char,
) -> *mut QingciEngine {
    let Some(dict_path) = unsafe_ptr_str(dict_path) else {
        return ptr::null_mut();
    };
    let user_path =
        unsafe_ptr_str(user_path).unwrap_or_else(|| String::from("/dev/null"));
    match QingciEngine::new(std::path::Path::new(&dict_path),
        std::path::Path::new(&user_path))
    {
        Ok(e) => Box::into_raw(Box::new(e)),
        Err(_) => ptr::null_mut(),
    }
}

/// 查询候选词。返回候选数量（0 表示无结果，负数表示错误）。
#[no_mangle]
pub extern "C" fn qingci_engine_search(
    engine: *mut QingciEngine,
    input: *const c_char,
    out: *mut QingciResults,
) -> c_int {
    let Some(engine) = unsafe_ref(engine) else { return -1 };
    let Some(input) = unsafe_ptr_str(input) else { return -2 };
    if out.is_null() {
        return -3;
    }
    let candidates = engine.search(&input);
    let count = candidates.len() as c_int;
    let texts: Vec<*mut c_char> = candidates
        .iter()
        .map(|c| match CString::new(c.word.as_str()) {
            Ok(s) => s.into_raw(),
            Err(_) => ptr::null_mut(), // 词内含 NUL 时置空占位
        })
        .collect();
    let mut texts = texts.into_boxed_slice();
    let out_ref = unsafe { &mut *out };
    out_ref.count = count;
    out_ref.texts = texts.as_mut_ptr();
    std::mem::forget(texts); // 所有权移交调用方，用 qingci_results_free 释放
    count
}

/// 释放 `qingci_engine_search` 返回的结果。
#[no_mangle]
pub extern "C" fn qingci_results_free(results: QingciResults) {
    if results.texts.is_null() || results.count <= 0 {
        return;
    }
    let slice = unsafe {
        Vec::from_raw_parts(
            results.texts,
            results.count as usize,
            results.count as usize,
        )
    };
    for p in slice {
        if !p.is_null() {
            unsafe { drop(CString::from_raw(p)) };
        }
    }
}

/// 选中最近一次查询结果的第 `index` 项（0 基）并学习。
/// 返回选中的词（调用方不拥有该内存，仅在下次引擎调用前有效）；
/// 无效序号返回空指针。
#[no_mangle]
pub extern "C" fn qingci_engine_select(
    engine: *mut QingciEngine,
    index: c_int,
) -> *const c_char {
    let Some(engine) = unsafe_ref(engine) else { return ptr::null() };
    if index < 0 {
        return ptr::null();
    }
    engine.select(index as usize).map_or(ptr::null(), |w| {
        // 返回内部缓存字符串指针是危险的做法；这里改为每次查询
        // 阶段缓存结果词并持有。为简化 ABI，返回泄漏的小缓冲由
        // 调用方立即复制是可接受的 MVP 折衷：改为线程局部缓冲。
        thread_local! {
            static LAST: std::cell::RefCell<CString> =
                std::cell::RefCell::new(CString::default());
        }
        LAST.with(|c| {
            let s = CString::new(w).unwrap_or_default();
            *c.borrow_mut() = s;
            c.borrow().as_ptr()
        })
    })
}

/// 联想预测：返回以 `last_word` 末字开头的候选词列表。
/// 返回候选数量（0 表示无结果，负数表示错误）。
#[no_mangle]
pub extern "C" fn qingci_engine_predict(
    engine: *mut QingciEngine,
    last_word: *const c_char,
    out: *mut QingciResults,
) -> c_int {
    let Some(engine) = unsafe_ref(engine) else { return -1 };
    let Some(last_word) = unsafe_ptr_str(last_word) else { return -2 };
    if out.is_null() {
        return -3;
    }
    let candidates = engine.predict(&last_word);
    let count = candidates.len() as c_int;
    let texts: Vec<*mut c_char> = candidates
        .iter()
        .map(|c| match CString::new(c.word.as_str()) {
            Ok(s) => s.into_raw(),
            Err(_) => ptr::null_mut(),
        })
        .collect();
    let mut texts = texts.into_boxed_slice();
    let out_ref = unsafe { &mut *out };
    out_ref.count = count;
    out_ref.texts = texts.as_mut_ptr();
    std::mem::forget(texts);
    count
}

/// 生成带音节分隔的预编辑串（如 "nihao" -> "ni'hao"）。
/// 返回线程局部缓冲指针，仅在下次引擎调用前有效；失败返回空指针。
#[no_mangle]
pub extern "C" fn qingci_engine_preedit(
    engine: *mut QingciEngine,
    input: *const c_char,
) -> *const c_char {
    let Some(engine) = unsafe_ref(engine) else { return ptr::null() };
    let Some(input) = unsafe_ptr_str(input) else { return ptr::null() };
    thread_local! {
        static PREEDIT: std::cell::RefCell<CString> =
            std::cell::RefCell::new(CString::default());
    }
    let formatted = engine.format_preedit(&input);
    PREEDIT.with(|c| {
        *c.borrow_mut() = CString::new(formatted).unwrap_or_default();
        c.borrow().as_ptr()
    })
}

/// 持久化用户词频。
#[no_mangle]
pub extern "C" fn qingci_engine_save(engine: *mut QingciEngine) -> c_int {
    match unsafe_ref(engine) {
        Some(e) => {
            e.save();
            0
        }
        None => -1,
    }
}

/// 已加载词条数。
#[no_mangle]
pub extern "C" fn qingci_engine_entry_count(engine: *mut QingciEngine) -> c_int {
    match unsafe_ref(engine) {
        Some(e) => e.entry_count() as c_int,
        None => -1,
    }
}

/// 销毁引擎。
#[no_mangle]
pub extern "C" fn qingci_engine_free(engine: *mut QingciEngine) {
    if !engine.is_null() {
        unsafe { drop(Box::from_raw(engine)) };
    }
}

fn unsafe_ptr_str(p: *const c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let s = unsafe { CStr::from_ptr(p) };
    Some(s.to_string_lossy().into_owned())
}

fn unsafe_ref<'a>(p: *mut QingciEngine) -> Option<&'a mut QingciEngine> {
    if p.is_null() {
        None
    } else {
        Some(unsafe { &mut *p })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn user_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "qingci-ffi-{}-{}.dic",
            name,
            std::process::id()
        ))
    }

    #[test]
    fn ffi_full_roundtrip() {
        let dict = CString::new(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("data")
                .join("qingci.dic")
                .to_str()
                .expect("utf8 path"),
        )
        .unwrap();
        let user = CString::new(
            user_file("roundtrip").to_str().expect("utf8 path"),
        )
        .unwrap();

        let e = qingci_engine_new(dict.as_ptr(), user.as_ptr());
        assert!(!e.is_null(), "引擎创建失败");

        // 词条数
        let count = qingci_engine_entry_count(e);
        assert!(count >= 600, "count={count}");

        // 查询
        let input = CString::new("nihao").unwrap();
        let mut res = QingciResults::default();
        let n = qingci_engine_search(e, input.as_ptr(), &mut res);
        assert!(n > 0, "应返回候选数，got {n}");
        assert_eq!(n, res.count);
        assert!(!res.texts.is_null());
        // 首候选应为「你好」
        let first = unsafe { CStr::from_ptr(*res.texts) };
        assert_eq!(first.to_str().unwrap(), "你好");

        // 无效序号
        assert!(qingci_engine_select(e, -1).is_null());
        assert!(qingci_engine_select(e, 99_999).is_null());

        // 有效序号：返回词与候选一致
        let word = qingci_engine_select(e, 0);
        assert!(!word.is_null());
        let word = unsafe { CStr::from_ptr(word) };
        assert_eq!(word.to_str().unwrap(), "你好");

        // 释放与保存
        qingci_results_free(res);
        assert_eq!(qingci_engine_save(e), 0);

        // 联想与预编辑
        let last = CString::new("你好").unwrap();
        let mut pres = QingciResults::default();
        let pn = qingci_engine_predict(e, last.as_ptr(), &mut pres);
        assert!(pn > 0, "联想应返回候选，got {pn}");
        // 首候选不应是刚上屏的词本身；具体排序由词库数据决定
        let first = unsafe { CStr::from_ptr(*pres.texts) };
        assert_ne!(first.to_str().unwrap(), "你好");
        qingci_results_free(pres);

        let pinyin = CString::new("nihao").unwrap();
        let p = qingci_engine_preedit(e, pinyin.as_ptr());
        assert!(!p.is_null());
        let p = unsafe { CStr::from_ptr(p) };
        assert_eq!(p.to_str().unwrap(), "ni'hao");

        qingci_engine_free(e);
    }

    #[test]
    fn ffi_null_and_error_paths() {
        // 空指针创建
        assert!(qingci_engine_new(ptr::null(), ptr::null()).is_null());

        // 不存在的词库
        let bad = CString::new("/nonexistent/qingci.dic").unwrap();
        let user = CString::new(user_file("bad").to_str().unwrap().to_string())
            .unwrap();
        assert!(qingci_engine_new(bad.as_ptr(), user.as_ptr()).is_null());

        // 空引擎句柄调用
        let mut res = QingciResults::default();
        assert_eq!(
            qingci_engine_search(ptr::null_mut(), ptr::null(), &mut res),
            -1
        );
        assert_eq!(qingci_engine_entry_count(ptr::null_mut()), -1);
        assert_eq!(qingci_engine_save(ptr::null_mut()), -1);
        assert!(qingci_engine_select(ptr::null_mut(), 0).is_null());

        // 输出参数为空
        let engine = {
            let dict = CString::new(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("data")
                    .join("qingci.dic")
                    .to_str()
                    .unwrap(),
            )
            .unwrap();
            let user = CString::new(
                user_file("nullout").to_str().unwrap().to_string(),
            )
            .unwrap();
            qingci_engine_new(dict.as_ptr(), user.as_ptr())
        };
        assert!(!engine.is_null());
        let input = CString::new("ni").unwrap();
        assert_eq!(
            qingci_engine_search(engine, input.as_ptr(), ptr::null_mut()),
            -3
        );
        qingci_engine_free(engine);

        // 释放空结果不应崩溃
        qingci_results_free(QingciResults::default());
        qingci_engine_free(ptr::null_mut());
    }

    #[test]
    fn ffi_empty_and_garbage_input() {
        let dict = CString::new(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("data")
                .join("qingci.dic")
                .to_str()
                .unwrap(),
        )
        .unwrap();
        let user = CString::new(user_file("garbage").to_str().unwrap().to_string())
            .unwrap();
        let e = qingci_engine_new(dict.as_ptr(), user.as_ptr());
        assert!(!e.is_null());

        for input in ["", "123!@#", "     "] {
            let input = CString::new(input).unwrap();
            let mut res = QingciResults::default();
            let n = qingci_engine_search(e, input.as_ptr(), &mut res);
            assert_eq!(n, 0, "input={input:?}");
            qingci_results_free(res);
        }
        qingci_engine_free(e);
    }
}
