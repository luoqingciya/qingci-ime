/*
 * 清辞拼音 —— Fcitx5 输入法插件（MVP）
 *
 * 通过 C ABI 调用 Rust 核心引擎（libqingci_core.a）完成候选词查询
 * 与用户词频学习；本文件负责按键处理、预编辑与候选列表 UI。
 *
 * SPDX-License-Identifier: MIT
 */
#include <cctype>
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

// CMake 注入的词库默认安装路径
#ifndef QINGCI_DICT_PATH
#define QINGCI_DICT_PATH "/usr/share/fcitx5/qingci/qingci.dic"
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
int qingci_engine_save(QingciCore *engine);
int qingci_engine_entry_count(QingciCore *engine);
void qingci_engine_free(QingciCore *engine);
}

namespace fcitx {

class QingciEngine;

// ---------------------------------------------------------------------------
// 每个输入上下文的状态：原始输入缓冲与候选列表
// ---------------------------------------------------------------------------
class QingciState : public InputContextProperty {
public:
    explicit QingciState(QingciEngine *) {}

    void reset() {
        buffer_.clear();
        candidates_.clear();
    }

    std::string buffer_;
    std::vector<std::string> candidates_;
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

    Instance *instance_;
    QingciCore *core_ = nullptr;
    KeyList selectionKeys_;
    FactoryFor<QingciState> factory_{
        [this](InputContext &) { return new QingciState(this); }};

    static constexpr size_t kPageSize = 5;
};

// ---------------------------------------------------------------------------
// 实现
// ---------------------------------------------------------------------------
QingciEngine::QingciEngine(Instance *instance) : instance_(instance) {
    // 词库：安装路径；用户词频：XDG 数据目录
    const char *home = ::getenv("HOME");
    std::string userPath = home ? std::string(home)
                                      + "/.local/share/qingci/user.dic"
                                : "/tmp/qingci-user.dic";
    core_ = qingci_engine_new(QINGCI_DICT_PATH, userPath.c_str());

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

    // 仅响应无修饰键（Ctrl/Alt/Shift/Super 组合放行给应用）
    if (key.hasModifier()) {
        return;
    }

    // 1) 字母 -> 追加输入
    if (sym >= FcitxKey_a && sym <= FcitxKey_z) {
        st->buffer_.push_back(static_cast<char>('a' + (sym - FcitxKey_a)));
        updateCandidates(ic, st);
        keyEvent.filterAndAccept();
        return;
    }

    // 2) 退格
    if (sym == FcitxKey_BackSpace) {
        if (st->buffer_.empty()) {
            return; // 交给应用处理
        }
        st->buffer_.pop_back();
        updateCandidates(ic, st);
        keyEvent.filterAndAccept();
        return;
    }

    // 组合态之外的按键全部放行
    if (st->buffer_.empty()) {
        return;
    }

    // 3) 空格 -> 提交当前高亮候选（默认第一个）
    if (sym == FcitxKey_space || sym == FcitxKey_KP_Space) {
        auto list = ic->inputPanel().candidateList();
        if (list && !list->empty()) {
            int cursor = list->cursorIndex();
            const CandidateWord &word =
                list->candidate(cursor >= 0 ? cursor : 0);
            word.select(ic);
        }
        keyEvent.filterAndAccept();
        return;
    }

    // 4) 数字键 1-9 -> 选词
    if (sym >= FcitxKey_1 && sym <= FcitxKey_9) {
        auto list = ic->inputPanel().candidateList();
        int idx = static_cast<int>(sym - FcitxKey_1);
        if (list && !list->empty() && idx < static_cast<int>(kPageSize)) {
            if (idx < static_cast<int>(list->size())) {
                list->candidate(idx).select(ic);
            }
        }
        keyEvent.filterAndAccept();
        return;
    }

    // 5) 翻页：- / = / PageUp / PageDown
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

    // 6) 回车 -> 上屏原始输入；Esc -> 清空
    if (sym == FcitxKey_Return || sym == FcitxKey_KP_Enter) {
        ic->commitString(st->buffer_);
        st->reset();
        updateUI(ic, st);
        keyEvent.filterAndAccept();
        return;
    }
    if (sym == FcitxKey_Escape) {
        st->reset();
        updateUI(ic, st);
        keyEvent.filterAndAccept();
        return;
    }

    // 其余按键在组合态下吞掉，避免破坏拼音缓冲
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

    // 预编辑显示原始拼音输入
    if (!st->buffer_.empty()) {
        ic->inputPanel().setPreedit(Text(st->buffer_));
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
    // 选词 + 用户词频学习（核心引擎内按最近一次查询的序号记录）
    if (core_) {
        const auto *st = state(ic);
        for (size_t i = 0; i < st->candidates_.size(); ++i) {
            if (st->candidates_[i] == word) {
                qingci_engine_select(core_, static_cast<int>(i));
                break;
            }
        }
    }
    ic->commitString(word);
    auto *st = state(ic);
    st->reset();
    updateUI(ic, st);
}

// 候选词选中回调（CandidateWord::select 的实现放在类外，
// 因为需要 QingciEngine 完整定义）
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
