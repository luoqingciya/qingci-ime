# 青瓷拼音（Qingci IME）

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

## 功能（MVP）

- **全拼输入**：多路径歧义切分（`xian` → 先/现 与 西安 均可达），撇号 `xi'an` 强制分段
- **简拼输入**：每音节取首字母（`nh` → 你好，`wm` → 我们）
- **不完整音节**：末音节前缀匹配（`nih` → 你好，`zh` → 张/周…）
- **词频排序**：基础词频 + 用户词频学习（选词自动记忆、持久化）
- **交互**：字母组合 / 空格或数字选词 / `-` `=` 翻页 / 回车上屏原文 / Esc 清空

## 快速开始（Ubuntu 26.04.1）

```bash
# 依赖
sudo apt install build-essential cmake extra-cmake-modules fcitx5-dev
# Rust（建议 rustup，apt 的 rustc 亦可，需 ≥1.56）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

./scripts/build-ubuntu.sh
```

详细步骤、 Wayland/X11 启用方法与排错见 [docs/BUILD-UBUNTU.md](docs/BUILD-UBUNTU.md)。

## 核心引擎独立使用

```bash
cd core
cargo test          # 19 个单元测试
cargo run --bin qingcli -- data/qingci.dic
# 输入 nihao / nh / wm / nih 回车查看候选
```

## 词库

词库格式（UTF-8，TAB 分隔）：

```
词条<TAB>基础词频<TAB>音节1 音节2 ...
```

当前内置约 700 条演示词库。生产环境建议导入完整拼音词表
（按上述格式转换即可，引擎按需加载，词库越大候选越全）。

## 许可

MIT（LICENSE）。Fcitx5 框架本体为 LGPL-2.1+，本插件以独立动态库形式链接，许可互不传染。
