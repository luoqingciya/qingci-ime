# 清辞拼音（Qingci IME）

基于 **Ubuntu 26.04.1 LTS + Fcitx5** 的简体中文拼音输入法。

核心引擎使用 **Rust** 实现，Fcitx5 插件胶水层使用 **C++**，通过 C ABI 连接。
本仓库在任意平台均可开发与测试核心引擎；插件部分在 Ubuntu 上构建。

## 架构

```
┌─────────────────────────────────────────────┐
│                Ubuntu 26.04.1 LTS           │
│                                             │
│  应用程序 ── Fcitx5 框架 ── qingci 插件 (C++)│
│                               │ C ABI (FFI) │
│                        qingci-core (Rust)   │
│                        · 音节表与切分        │
│                        · 词典倒排索引        │
│                        · 词频排序/学习       │
└─────────────────────────────────────────────┘
```

| 目录 | 内容 |
|------|------|
| `core/` | Rust 核心引擎（`qingci-core`）+ 命令行演示工具 `qingcli` |
| `core/data/qingci.dic` | 内置基础词库（约 700 条，可替换为完整词库） |
| `addon/` | C++ Fcitx5 插件、addon/inputmethod 描述文件、图标 |
| `scripts/build-ubuntu.sh` | Ubuntu 一键构建安装脚本 |
| `docs/BUILD-UBUNTU.md` | Ubuntu 26.04.1 构建与使用文档 |

## 功能（v0.2）

- **全拼输入**：多路径歧义切分（`xian` → 先/现 与 西安 均可达），撇号 `xi'an` 强制分段
- **简拼输入**：每音节取首字母（`nh` → 你好，`wm` → 我们）
- **不完整音节**：末音节前缀匹配（`nih` → 你好，`zh` → 张/周…）
- **词频排序**：真实语料词频 + 用户词频学习（选词自动记忆、持久化、层内置顶）
- **中文标点**：全角标点自动转换（，。？！「」《》……）、成对标点自动配对、
  数字后小数点保持半角（3.14）、`Ctrl+.` 一键切换全/半角
- **联想预测**：上屏一词后自动推荐以末字开头的候选，连续选词连续联想
- **候选高亮**：Tab / Shift+Tab 移动高亮，空格上屏高亮项
- **预编辑分段**：输入串按音节显示（`ni'hao`）
- **临时英文**：单击 Shift 切换中/英模式；CapsLock 大写字母直接透传；回车上屏原始拼音

## 快速开始（Ubuntu 26.04.1）

### 安装包（推荐）

从 [Releases](https://github.com/luoqingciya/qingci-ime/releases) 下载对应
Ubuntu 版本的 deb 包（CI 自动构建并实测安装）：

```bash
sudo apt install ./qingci-ime_0.2.9_ubuntu26.10_amd64.deb
sudo qingci-setup        # 一键配置：切换 fcitx5 + 写入环境变量
# 注销重新登录 → fcitx5-configtool 添加「清辞拼音」 → Ctrl+Space 切换
```

### 从源码构建

```bash
# 依赖
sudo apt install build-essential cmake extra-cmake-modules \
     libfcitx5core-dev libfcitx5config-dev libfcitx5utils-dev
# Rust（建议 rustup，apt 的 rustc 亦可，需 ≥1.56）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

./scripts/build-ubuntu.sh
```

详细步骤、 deb/tar 说明、Wayland/X11 启用方法与排错见
[docs/BUILD-UBUNTU.md](docs/BUILD-UBUNTU.md)。

## 核心引擎独立使用

```bash
cd core
cargo test          # 34 个单元测试（含全量词库冒烟测试）
cargo run --bin qingcli -- data/qingci-full.dic
# 输入 nihao / nh / wm / nih 回车查看候选
```

## 词库

| 文件 | 规模 | 用途 |
|------|------|------|
| `core/data/qingci-full.dic` | 约 42.3 万条（41.1 万词组 + 1.16 万单字） | **插件默认安装的全量词库** |
| `core/data/qingci.dic` | 约 700 条 | 手写演示词库，供单元测试与快速体验 |

格式（UTF-8，TAB 分隔）：`词条<TAB>词频<TAB>音节1 音节2 ...`（ü 写作 v）。

全量词库由 `cargo run --bin qingdict-gen` 从开源数据自动生成，含品牌词种子；
生成脚本与原始数据路径见 `core/src/bin/qingdict_gen.rs` 头部注释。

## 词库数据来源与致谢

生成 `qingci-full.dic` 所引用的数据均为 MIT 许可开源项目：

| 数据 | 来源 | 许可 | 用途 |
|------|------|------|------|
| jieba 词典 | [fxsjy/jieba](https://github.com/fxsjy/jieba) | MIT | 词频排序 |
| phrase-pinyin-data | [mozillazg/phrase-pinyin-data](https://github.com/mozillazg/phrase-pinyin-data) | MIT | 词组拼音 |
| pinyin-data | [mozillazg/pinyin-data](https://github.com/mozillazg/pinyin-data) | MIT | 单字读音 |

感谢上述项目的作者与贡献者。

## 许可

MIT（LICENSE）。Fcitx5 框架本体为 LGPL-2.1+，本插件以独立动态库形式链接，许可互不传染。
