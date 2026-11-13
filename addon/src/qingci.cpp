/*
 * 清辞拼音 —— Fcitx5 输入法插件
 *
 * 通过 C ABI 调用 Rust 核心引擎（libqingci_core.a）完成候选查询、
 * 联想预测与用户词频学习；本文件负责按键处理、预编辑、候选列表
 * UI 与中文标点（含全/半角切换、成对标点）。
 *
 * SPDX-License-Identifier: MIT
 */
#include <cstdint>
#include <cstdlib>
#include <string>
#include <vector>

#include <fcitx/addonfactory.h>
#include <fcitx/addonmanager.h>
#include <fcitx/candidatelist.h>
#include <fcitx/inputcontext.h>
#include <fcitx/inputcontextproperty.h>
#include <fcitx/inputmethodengine.h>
#include <fcitx/inputpanel.h>
#include <fcitx/instance.h>
#include <fcitx/text.h>
#include <fcitx-utils/key.h>
#include <fcitx-utils/keysym.h>
#include <fcitx-utils/log.h>

// CMake 注入的词库默认安装路径
#ifndef QINGCI_DICT_PATH
#define QINGCI_DICT_PATH "/usr/share/fcitx5/qingci/qingci-full.dic"
#endif

// ---------------------------------------------------------------------------
// Rust 核心引擎 C ABI（对应 core/src/ffi.rs）
// ---------------------------------------------------------------------------
struct QingciCore;

extern "C" {
struct QingciResultsFFI {
    char **texts;
    int count;
};

QingciCore *qingci_engine_new(const char *dict_path, const char *user_path);
int qingci_engine_search(QingciCore *engine, const char *input,
                         QingciResultsFFI *out);
void qingci_results_free(QingciResultsFFI results);
const char *qingci_engine_select(QingciCore *engine, int index);
int qingci_engine_predict(QingciCore *engine, const char *last_word,
                          QingciResultsFFI *out);
const char *qingci_engine_preedit(QingciCore *engine, const char *input);
int qingci_engine_save(QingciCore *engine);
int qingci_engine_entry_count(QingciCore *engine);
void qingci_engine_free(QingciCore *engine);
}

namespace fcitx {

class QingciEngine;

// ---------------------------------------------------------------------------
// 每个输入上下文的状态
// ---------------------------------------------------------------------------
enum class Mode { Normal, Predict };

class QingciState : public InputContextProperty {
public:
    explicit QingciState(QingciEngine *) {}

    void reset() {
        buffer_.clear();
        candidates_.clear();
        mode_ = Mode::Normal;
        dqOpen_ = true;
        sqOpen_ = true;
    }

    std::string buffer_;
    std::vector<std::string> candidates_;
    Mode mode_ = Mode::Normal;
    bool dqOpen_ = true; // 双引号配对：true -> 下一个输出“
    bool sqOpen_ = true; // 单引号配对：true -> 下一个输出‘
};

// ---------------------------------------------------------------------------
// 候选词：被选中时回调引擎提交并学习
// ---------------------------------------------------------------------------
class QingciCandidateWord : public CandidateWord {
public:
    QingciCandidateWord(QingciEngine *engine, size_t index, Text text)
        : CandidateWord(std::move(text)), engine_(engine), index_(index) {}

    void select(InputContext *inputContext) const override;

private:
    QingciEngine *engine_;
    size_t index_;
};

// ---------------------------------------------------------------------------
// 输入法引擎
// ---------------------------------------------------------------------------
class QingciEngine final : public InputMethodEngineV2 {
public:
    explicit QingciEngine(Instance *instance);
    ~QingciEngine() override;

    void keyEvent(const InputMethodEntry &entry,
                  KeyEvent &keyEvent) override;
    void reset(const InputMethodEntry &entry,
               InputContextEvent &event) override;
    void save() override;

    void selectCandidate(InputContext *ic, size_t index);

private:
    QingciState *state(InputContext *ic) {
        return ic->propertyFor(&factory_);
    }

    void updateCandidates(InputContext *ic, QingciState *st);
    void updateUI(InputContext *ic, QingciState *st);
    void commitWord(InputContext *ic, const std::string &word);
    void commitRaw(InputContext *ic, QingciState *st);
    void learnCandidate(QingciState *st, const std::string &word);
    bool handlePunc(InputContext *ic, QingciState *st, KeySym sym);

    Instance *instance_;
    QingciCore *core_ = nullptr;
    KeyList selectionKeys_;
    bool puncEnabled_ = true;
    FactoryFor<QingciState> factory_{
        [this](InputContext &) { return new QingciState(this); }};

    static constexpr size_t kPageSize = 5;
};

// ---------------------------------------------------------------------------
// 工具函数
// ---------------------------------------------------------------------------
namespace {

/// 取字符串最后一个 UTF-8 字符；仅当其为常用 CJK 汉字时返回，否则为空。
std::string lastUtf8Char(const std::string &s) {
    if (s.empty()) {
        return {};
    }
    size_t i = s.size() - 1;
    while (i > 0 && (static_cast<unsigned char>(s[i]) & 0xC0) == 0x80) {
        --i;
    }
    std::string c = s.substr(i);
    if (c.size() == 3 &&
        (static_cast<unsigned char>(c[0]) & 0xF0) == 0xE0) {
        uint32_t cp = (static_cast<uint32_t>(c[0] & 0x0F) << 12) |
                      (static_cast<uint32_t>(c[1] & 0x3F) << 6) |
                      static_cast<uint32_t>(c[2] & 0x3F);
        if (cp >= 0x4E00 && cp <= 0x9FFF) {
            return c;
        }
    }
    return {};
}

/// 周边文本最后一个字节是否为 ASCII 数字（用于 3.14 之类的小数点）。
bool lastSurroundingIsDigit(InputContext *ic) {
    const auto &surrounding = ic->surroundingText();
    if (!surrounding.isValid()) {
        return false;
    }
    const auto &text = surrounding.text();
    if (text.empty()) {
        return false;
    }
    unsigned char prev = static_cast<unsigned char>(text.back());
    return prev >= '0' && prev <= '9';
}

} // namespace

// ---------------------------------------------------------------------------
// 实现
// ---------------------------------------------------------------------------
QingciEngine::QingciEngine(Instance *instance) : instance_(instance) {
    const char *home = ::getenv("HOME");
    std::string userPath = home ? std::string(home)
                                      + "/.local/share/qingci/user.dic"
                                : "/tmp/qingci-user.dic";
    core_ = qingci_engine_new(QINGCI_DICT_PATH, userPath.c_str());
    if (!core_) {
        FCITX_WARN() << "qingci: 词库加载失败，输入将无候选。路径: "
                     << QINGCI_DICT_PATH;
    }

    selectionKeys_ = {Key(FcitxKey_1), Key(FcitxKey_2), Key(FcitxKey_3),
                      Key(FcitxKey_4), Key(FcitxKey_5), Key(FcitxKey_6),
                      Key(FcitxKey_7), Key(FcitxKey_8), Key(FcitxKey_9)};

    instance_->inputContextManager().registerProperty("qingciState",
                                                      &factory_);
}

QingciEngine::~QingciEngine() {
    if (core_) {
        qingci_engine_save(core_);
        qingci_engine_free(core_);
        core_ = nullptr;
    }
}

void QingciEngine::save() {
    if (core_) {
        qingci_engine_save(core_);
    }
}

void QingciEngine::keyEvent(const InputMethodEntry &entry,
                            KeyEvent &keyEvent) {
    (void)entry;
    if (keyEvent.isRelease()) {
        return;
    }

    auto *ic = keyEvent.inputContext();
    auto *st = state(ic);
    const auto &key = keyEvent.key();
    const auto sym = key.sym();

    // 0) Ctrl+. 切换中文/英文标点
    if ((key.states() & KeyState::Ctrl) && sym == FcitxKey_period) {
        puncEnabled_ = !puncEnabled_;
        ic->inputPanel().reset();
        ic->inputPanel().setAuxUp(Text(puncEnabled_ ? "中文标点" : "英文标点"));
        ic->updateUserInterface(UserInterfaceComponent::InputPanel);
        keyEvent.filterAndAccept();
        return;
    }

    // 1) Tab / Shift+Tab：候选高亮移动
    if (sym == FcitxKey_Tab || sym == FcitxKey_ISO_Left_Tab) {
        const bool shift = (key.states() & KeyState::Shift);
        auto list = ic->inputPanel().candidateList();
        if (list && !list->empty()) {
            if (auto *movable = list->toCursorMovable()) {
                if (shift) {
                    movable->prevCandidate();
                } else {
                    movable->nextCandidate();
                }
                ic->updateUserInterface(UserInterfaceComponent::InputPanel);
                keyEvent.filterAndAccept();
                return;
            }
        }
        return; // 无候选时放行
    }

    // 2) 修饰键组合放行给应用
    if (key.hasModifier()) {
        return;
    }

    // 3) 大写字母（CapsLock）：上屏已输入内容后透传大写字母
    if (sym >= FcitxKey_A && sym <= FcitxKey_Z) {
        if (!st->buffer_.empty()) {
            commitRaw(ic, st);
        }
        return; // 大写字母透传（临时英文）
    }

    // 4) 联想模式：字母键退出联想并进入组合；退格/Esc 退出联想
    if (st->mode_ == Mode::Predict) {
        if (sym >= FcitxKey_a && sym <= FcitxKey_z) {
            st->mode_ = Mode::Normal;
            st->candidates_.clear();
            ic->inputPanel().reset();
            // 落入下方字母处理
        } else if (sym == FcitxKey_BackSpace || sym == FcitxKey_Escape) {
            st->reset();
            updateUI(ic, st);
            keyEvent.filterAndAccept();
            return;
        }
    }

    // 5) 字母 -> 追加输入
    if (sym >= FcitxKey_a && sym <= FcitxKey_z) {
        st->buffer_.push_back(static_cast<char>('a' + (sym - FcitxKey_a)));
        updateCandidates(ic, st);
        keyEvent.filterAndAccept();
        return;
    }

    // 6) 退格
    if (sym == FcitxKey_BackSpace) {
        if (st->buffer_.empty()) {
            return; // 交给应用处理
        }
        st->buffer_.pop_back();
        updateCandidates(ic, st);
        keyEvent.filterAndAccept();
        return;
    }

    // 空闲态的其余按键放行
    if (st->buffer_.empty() && st->mode_ != Mode::Predict) {
        return;
    }

    // 7) 空格 -> 提交当前高亮候选（默认第一个）
    if (sym == FcitxKey_space || sym == FcitxKey_KP_Space) {
        auto list = ic->inputPanel().candidateList();
        if (list && !list->empty()) {
            int cursor = list->cursorIndex();
            list->candidate(cursor >= 0 ? cursor : 0).select(ic);
        }
        keyEvent.filterAndAccept();
        return;
    }

    // 8) 数字键 1-9 -> 选词
    if (sym >= FcitxKey_1 && sym <= FcitxKey_9) {
        auto list = ic->inputPanel().candidateList();
        int idx = static_cast<int>(sym - FcitxKey_1);
        if (list && !list->empty() && idx < static_cast<int>(kPageSize)) {
            if (idx < list->size()) {
                list->candidate(idx).select(ic);
            }
        }
        keyEvent.filterAndAccept();
        return;
    }

    // 9) 翻页：= / PageDown / Down；- / PageUp / Up
    if (sym == FcitxKey_equal || sym == FcitxKey_Page_Down ||
        sym == FcitxKey_Down) {
        if (auto *pageable = ic->inputPanel().candidateList()->toPageable()) {
            if (pageable->hasNext()) {
                pageable->next();
                ic->updateUserInterface(
                    UserInterfaceComponent::InputPanel);
            }
        }
        keyEvent.filterAndAccept();
        return;
    }
    if (sym == FcitxKey_minus || sym == FcitxKey_Page_Up ||
        sym == FcitxKey_Up) {
        if (auto *pageable = ic->inputPanel().candidateList()->toPageable()) {
            if (pageable->hasPrev()) {
                pageable->prev();
                ic->updateUserInterface(
                    UserInterfaceComponent::InputPanel);
            }
        }
        keyEvent.filterAndAccept();
        return;
    }

    // 10) 回车 -> 上屏原始输入
    if (sym == FcitxKey_Return || sym == FcitxKey_KP_Enter) {
        if (!st->buffer_.empty()) {
            commitRaw(ic, st);
            keyEvent.filterAndAccept();
            return;
        }
        return; // 联想态放行
    }

    // 11) Esc -> 清空/退出联想
    if (sym == FcitxKey_Escape) {
        st->reset();
        updateUI(ic, st);
        keyEvent.filterAndAccept();
        return;
    }

    // 12) 中文标点
    if (handlePunc(ic, st, sym)) {
        keyEvent.filterAndAccept();
        return;
    }

    // 组合/联想态下其余按键吞掉
    keyEvent.filterAndAccept();
}

void QingciEngine::reset(const InputMethodEntry &entry,
                         InputContextEvent &event) {
    (void)entry;
    auto *ic = event.inputContext();
    auto *st = state(ic);
    st->reset();
    updateUI(ic, st);
}

void QingciEngine::commitRaw(InputContext *ic, QingciState *st) {
    if (!st->buffer_.empty()) {
        ic->commitString(st->buffer_);
    }
    st->reset();
    updateUI(ic, st);
}

void QingciEngine::learnCandidate(QingciState *st, const std::string &word) {
    if (!core_) {
        return;
    }
    for (size_t i = 0; i < st->candidates_.size(); ++i) {
        if (st->candidates_[i] == word) {
            qingci_engine_select(core_, static_cast<int>(i));
            break;
        }
    }
}

void QingciEngine::updateCandidates(InputContext *ic, QingciState *st) {
    st->candidates_.clear();
    if (!st->buffer_.empty() && core_) {
        QingciResultsFFI res{nullptr, 0};
        if (qingci_engine_search(core_, st->buffer_.c_str(), &res) > 0 &&
            res.count > 0 && res.texts) {
            st->candidates_.reserve(static_cast<size_t>(res.count));
            for (int i = 0; i < res.count; ++i) {
                if (res.texts[i]) {
                    st->candidates_.emplace_back(res.texts[i]);
                }
            }
        }
        qingci_results_free(res);
    }
    updateUI(ic, st);
}

void QingciEngine::updateUI(InputContext *ic, QingciState *st) {
    ic->inputPanel().reset();

    // 预编辑：带音节分隔（ni'hao），联想模式无预编辑
    if (!st->buffer_.empty()) {
        std::string pre;
        if (core_) {
            const char *p =
                qingci_engine_preedit(core_, st->buffer_.c_str());
            if (p) {
                pre = p;
            }
        }
        if (pre.empty()) {
            pre = st->buffer_;
        }
        ic->inputPanel().setPreedit(Text(pre));
        ic->updatePreedit();
    }

    // 候选列表
    if (!st->candidates_.empty()) {
        auto list = std::make_unique<CommonCandidateList>();
        list->setPageSize(static_cast<int>(kPageSize));
        list->setSelectionKey(selectionKeys_);
        for (size_t i = 0; i < st->candidates_.size(); ++i) {
            list->append<QingciCandidateWord>(this, i,
                                              Text(st->candidates_[i]));
        }
        list->setCursorKeepInSamePage(true);
        ic->inputPanel().setCandidateList(std::move(list));
    }

    ic->updateUserInterface(UserInterfaceComponent::InputPanel);
}

void QingciEngine::selectCandidate(InputContext *ic, size_t index) {
    auto *st = state(ic);
    if (!core_ || index >= st->candidates_.size()) {
        return;
    }
    commitWord(ic, st->candidates_[index]);
}

void QingciEngine::commitWord(InputContext *ic, const std::string &word) {
    learnCandidate(state(ic), word);
    ic->commitString(word);

    // 联想预测：以上屏词末字开头
    auto *st = state(ic);
    st->reset();
    if (core_) {
        std::string last = lastUtf8Char(word);
        if (!last.empty()) {
            QingciResultsFFI res{nullptr, 0};
            if (qingci_engine_predict(core_, last.c_str(), &res) > 0 &&
                res.count > 0 && res.texts) {
                st->candidates_.reserve(static_cast<size_t>(res.count));
                for (int i = 0; i < res.count; ++i) {
                    if (res.texts[i]) {
                        st->candidates_.emplace_back(res.texts[i]);
                    }
                }
                st->mode_ = Mode::Predict;
            }
            qingci_results_free(res);
        }
    }
    updateUI(ic, st);
}

bool QingciEngine::handlePunc(InputContext *ic, QingciState *st,
                              KeySym sym) {
    if (!puncEnabled_) {
        return false;
    }

    std::string out;
    switch (sym) {
        case FcitxKey_comma:
            out = "，";
            break;
        case FcitxKey_period:
            // 数字之后保持半角点号（3.14）
            out = lastSurroundingIsDigit(ic) ? "." : "。";
            break;
        case FcitxKey_semicolon:
            out = "；";
            break;
        case FcitxKey_colon:
            out = "：";
            break;
        case FcitxKey_question:
            out = "？";
            break;
        case FcitxKey_exclam:
            out = "！";
            break;
        case FcitxKey_parenleft:
            out = "（";
            break;
        case FcitxKey_parenright:
            out = "）";
            break;
        case FcitxKey_bracketleft:
            out = "【";
            break;
        case FcitxKey_bracketright:
            out = "】";
            break;
        case FcitxKey_less:
            out = "《";
            break;
        case FcitxKey_greater:
            out = "》";
            break;
        case FcitxKey_slash:
            out = "、";
            break;
        case FcitxKey_asciitilde:
            out = "～";
            break;
        case FcitxKey_asciicircum:
            out = "……";
            break;
        case FcitxKey_dollar:
            out = "￥";
            break;
        case FcitxKey_percent:
            out = "％";
            break;
        case FcitxKey_ampersand:
            out = "＆";
            break;
        case FcitxKey_at:
            out = "＠";
            break;
        case FcitxKey_asterisk:
            out = "×";
            break;
        case FcitxKey_plus:
            out = "＋";
            break;
        case FcitxKey_quotedbl:
            out = st->dqOpen_ ? "“" : "”";
            st->dqOpen_ = !st->dqOpen_;
            break;
        case FcitxKey_apostrophe:
            out = st->sqOpen_ ? "‘" : "’";
            st->sqOpen_ = !st->sqOpen_;
            break;
        default:
            return false;
    }

    // 组合态：先提交高亮候选（学习）或原始输入；联想态：清空后上屏标点
    if (!st->buffer_.empty()) {
        auto list = ic->inputPanel().candidateList();
        if (!st->candidates_.empty() && list && !list->empty()) {
            int cursor = list->cursorIndex();
            size_t idx = static_cast<size_t>(cursor >= 0 ? cursor : 0);
            if (idx < st->candidates_.size()) {
                const std::string word = st->candidates_[idx];
                learnCandidate(st, word);
                ic->commitString(word);
            }
        } else {
            ic->commitString(st->buffer_);
        }
    }
    st->reset();
    ic->commitString(out);
    updateUI(ic, st);
    return true;
}

// 候选词选中回调
void QingciCandidateWord::select(InputContext *inputContext) const {
    engine_->selectCandidate(inputContext, index_);
}

// ---------------------------------------------------------------------------
// 插件工厂
// ---------------------------------------------------------------------------
class QingciEngineFactory : public AddonFactory {
public:
    AddonInstance *create(AddonManager *manager) override {
        return new QingciEngine(manager->instance());
    }
};

} // namespace fcitx

FCITX_ADDON_FACTORY_V2(qingci, fcitx::QingciEngineFactory)
