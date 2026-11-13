# 清辞拼音 —— Ubuntu 26.04.1 LTS 构建与使用文档

## 一、安装依赖

```bash
sudo apt update
sudo apt install build-essential cmake extra-cmake-modules \
     libfcitx5core-dev libfcitx5config-dev libfcitx5utils-dev
```

Rust 工具链（推荐 rustup；apt 安装的 rustc ≥1.56 亦可）：

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

> 说明：Ubuntu 将 Fcitx5 的开发文件拆分为
> `libfcitx5core-dev` / `libfcitx5config-dev` / `libfcitx5utils-dev`
> （Debian 上的 `fcitx5-dev` 在 Ubuntu 中不存在）；
> `extra-cmake-modules` 提供 Fcitx5CompilerSettings 所需的 KDE/ECM 宏。

## 二、构建与安装

```bash
cd qingci-ime
./scripts/build-ubuntu.sh
```

脚本执行三步：
1. `cargo build --release` 编译 Rust 核心引擎，产出 `libqingci_core.a`
2. CMake 编译 Fcitx5 插件（链接静态核心库），产出 `libqingci.so`
3. `sudo cmake --install` 安装到系统

安装位置：

| 文件 | 路径 |
|------|------|
| 插件动态库 | `/usr/lib/<triplet>/fcitx5/libqingci.so` |
| Addon 描述 | `/usr/share/fcitx5/addon/qingci-addon.conf` |
| 输入法描述 | `/usr/share/fcitx5/inputmethod/qingci.conf` |
| 内置词库 | `/usr/share/fcitx5/qingci/qingci.dic` |
| 图标 | `/usr/share/icons/hicolor/scalable/apps/qingci.svg` |
| 用户词频 | `~/.local/share/qingci/user.dic`（运行时生成） |

只构建不安装：`SKIP_INSTALL=1 ./scripts/build-ubuntu.sh`

## 三、启用输入法

1. 将系统输入法框架切换为 fcitx5：

   ```bash
   im-config -n fcitx5
   ```

   注销并重新登录（Wayland 会话下 GNOME 需确认未强制 IBus）。

2. 重启 fcitx5 并添加输入法：

   ```bash
   killall fcitx5; fcitx5 -d
   fcitx5-configtool
   ```

   在「输入法」列表中取消「仅显示当前语言」，搜索 **清辞拼音** 并添加。

3. 使用 `Ctrl+Space` 切换到清辞拼音。

## 四、按键说明（MVP）

| 按键 | 行为 |
|------|------|
| `a`-`z` | 拼音输入（自动候选） |
| `空格` | 提交当前高亮候选 |
| `1`-`9` | 选对应候选 |
| `=` / `PageDown` / `↓` | 下一页 |
| `-` / `PageUp` / `↑` | 上一页 |
| `Backspace` | 删除输入末字符（空缓冲时透传给应用） |
| `Enter` | 上屏原始拼音 |
| `Esc` | 清空输入 |

选词会自动更新用户词频并持久化到 `~/.local/share/qingci/user.dic`。

## 五、验证核心引擎（无需桌面环境）

```bash
cd core && cargo test
cargo run --bin qingcli -- data/qingci.dic
```

输入示例：

```
nihao   → 1.你好 2.你 3.呢 4.妮…
nh      → 1.你好 …
wm      → 1.我们 …
xi'an   → 1.西安 …
```

## 六、排错

- **配置工具里找不到清辞拼音**：确认
  `/usr/share/fcitx5/addon/qingci-addon.conf` 与
  `/usr/share/fcitx5/inputmethod/qingci.conf` 存在；查看
  `fcitx5` 终端日志是否报 `Could not locate library libqingci.so`。
- **插件加载失败**：`ldd /usr/lib/$(gcc -dumpmachine)/fcitx5/libqingci.so`
  检查依赖；确保插件与 fcitx5 为同一架构。
- **候选为空**：`/usr/share/fcitx5/qingci/qingci.dic` 是否存在、
  是否被其他词库路径覆盖。
- **无法切换输入法框架**：GNOME + Wayland 下需安装
  `fcitx5-frontend-gtk4/gtk3/qt5` 并使用 `im-config -n fcitx5`。
- 通用诊断工具：`fcitx5-diagnose`。

## 七、已知限制（MVP）

- 内置词库仅约 700 条，覆盖演示需要；建议导入完整词表
- 无模糊音、双拼、联想、智能整句
- 组合态下未处理的标点按键会被吞掉（不透传）
