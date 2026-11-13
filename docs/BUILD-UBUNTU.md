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

## 二、安装

### 方式 A：安装 deb 包（推荐）

从 [Releases](https://github.com/luoqingciya/qingci-ime/releases) 下载
对应版本的 deb（CI 在各发行版容器内构建并实测安装）：

```bash
sudo apt install ./qingci-ime_0.2.6_ubuntu26.10_amd64.deb
# 或：sudo dpkg -i qingci-ime_0.2.6_ubuntu26.10_amd64.deb
```

包内含插件、词库、文档与一键配置脚本，依赖（fcitx5 ≥ 5.1.19）由 apt
自动解析；卸载：`sudo apt remove qingci-ime`（用户词频
`~/.local/share/qingci/` 会保留）。

> deb 针对构建时的 fcitx5 版本（Ubuntu 26.04 = 5.1.19）；
> 其他 Ubuntu 版本请使用方式 B/C。

### 方式 B：通用 tar.gz

从 Releases 下载 `qingci-ime_<版本>-linux-amd64.tar.gz`，解压到根目录：

```bash
sudo tar -xzf qingci-ime_0.2.6-linux-amd64.tar.gz -C /
```

### 方式 C：源码构建

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
| Addon 描述 | `/usr/share/fcitx5/addon/qingci.conf` |
| 输入法描述 | `/usr/share/fcitx5/inputmethod/qingci.conf` |
| 内置词库 | `/usr/share/fcitx5/qingci/qingci-full.dic` |
| 图标 | `/usr/share/icons/hicolor/scalable/apps/qingci.svg` |
| 用户词频 | `~/.local/share/qingci/user.dic`（运行时生成） |

只构建不安装：`SKIP_INSTALL=1 ./scripts/build-ubuntu.sh`

## 三、启用输入法

### 方式 A：一键配置（推荐）

```bash
sudo qingci-setup
```

脚本会自动完成：im-config 切换到 fcitx5、写入 Wayland/X11 会话所需的
输入法环境变量（`GTK_IM_MODULE` / `QT_IM_MODULE` / `XMODIFIERS` /
`SDL_IM_MODULE`，幂等写入 `/etc/environment`）、确认 fcitx5 开机自启。
**完成后注销并重新登录**。

### 方式 B：手动配置

1. 将系统输入法框架切换为 fcitx5：

   ```bash
   im-config -n fcitx5
   ```

2. 写入会话环境变量（**GNOME Wayland 会话必需**，否则应用仍走 ibus）：

   ```bash
   sudo tee -a /etc/environment <<'EOF'
   GTK_IM_MODULE=fcitx
   QT_IM_MODULE=fcitx
   XMODIFIERS=@im=fcitx
   EOF
   ```

3. 注销并重新登录。登录后 fcitx5 会自动启动（右下角托盘图标）。

4. 添加输入法：

   ```bash
   fcitx5-configtool
   ```

   在「输入法」页**取消「仅显示当前语言」**，搜索 **清辞拼音** 并添加。

5. 使用 `Ctrl+Space` 切换到清辞拼音。

> 判断配置是否生效：注销重登后，顶栏不再显示 GNOME 的 ibus「中/A」
> 键盘指示器，右下角出现 fcitx5 托盘图标；若无 fcitx5 托盘，先运行
> `fcitx5 -d` 检查报错，并确认 `/etc/environment` 已含上述变量。

## 四、按键说明（MVP）

| 按键 | 行为 |
|------|------|
| `a`-`z` | 拼音输入（自动候选） |
| `空格` | 提交当前高亮候选 |
| `1`-`9` | 选对应候选 |
| `Tab` / `Shift+Tab` | 高亮下一 / 上一候选 |
| `=` / `PageDown` / `↓` | 下一页 |
| `-` / `PageUp` / `↑` | 上一页 |
| `Backspace` | 删除输入末字符（空缓冲时透传给应用） |
| `Enter` | 上屏原始拼音 |
| `Esc` | 清空输入 / 退出联想 |
| `单击 Shift` | 切换中/英文模式（面板提示「中文/英文」） |
| `CapsLock`+字母 | 大写字母透传（临时英文输入） |

### 中文标点

默认开启全角标点：`, . ; : ? ! ( ) [ ] < > / ~ ^ $ % & @ * +` 分别映射为
`，。；：？！（）【】《》、～……￥％＆＠×＋`；双引号/单引号自动配对（“” ‘’）；
数字后的小数点保持半角（输入 `3.14` 不受影响）。

- `Ctrl+.` 切换中文标点 / 英文标点（面板会显示当前模式）
- 选词后自动进入**联想预测**：直接推荐以末字开头的词，`空格/数字` 连续选词
  连续联想；输入新拼音、`Backspace` 或 `Esc` 退出联想

选词会自动更新用户词频并持久化到 `~/.local/share/qingci/user.dic`。

## 五、验证核心引擎（无需桌面环境）

```bash
cd core && cargo test
cargo run --bin qingcli -- data/qingci-full.dic
```

输入示例（词频排序随数据源略有差异）：

```
nihao   → 1.你好 …
nh      → 1.南海 2.你好 …
wm      → 1.我们 2.文明 …
xi'an   → 1.西安 …
```

## 六、排错

- **配置工具里找不到清辞拼音**：确认
  `/usr/share/fcitx5/addon/qingci.conf` 与
  `/usr/share/fcitx5/inputmethod/qingci.conf` 存在；查看
  `fcitx5` 终端日志是否报 `Could not locate library libqingci.so`。
- **插件加载失败**：`ldd /usr/lib/$(gcc -dumpmachine)/fcitx5/libqingci.so`
  检查依赖；确保插件与 fcitx5 为同一架构。
- **候选为空**：`/usr/share/fcitx5/qingci/qingci-full.dic` 是否存在、
  是否被其他词库路径覆盖。
- **无法切换输入法框架**：GNOME + Wayland 下需安装
  `fcitx5-frontend-gtk4/gtk3/qt5` 并使用 `im-config -n fcitx5`。
- 通用诊断工具：`fcitx5-diagnose`。
- **Firefox（snap 版）打不出中文**：Ubuntu 的 Firefox 是 snap 包，其内置 GTK
  输入模块只有 fcitx4 的 `im-fcitx.so`，不含 fcitx5 所需 `im-fcitx5.so`，
  且 snap 无法访问宿主模块（snap 沙箱限制，与清辞无关）。解决方案二选一：
  1. 安装 deb 版 Firefox（Mozilla Team PPA），走宿主 GTK 模块：
     ```bash
     sudo add-apt-repository ppa:mozillateam/ppa
     sudo apt install firefox
     ```
  2. 或改用 Google Chrome（官方 deb 包）。
  安装后重新登录，浏览器即可正常输入中文。
- **GNOME Wayland 下个别应用不接管输入**：确认该应用为 GTK3/GTK4/Qt 应用；
  XWayland 应用依赖 `XMODIFIERS=@im=fcitx`（qingci-setup 已写入）。

## 七、已知限制

- 无模糊音、双拼、智能整句（组句）、云候选、繁体输出
- 词组内取单字（如从词组中单选一个汉字）暂未支持
- 配置项暂未接入 fcitx5 配置界面（页面大小等目前固定）
