#!/usr/bin/env bash
# 青瓷拼音 —— Ubuntu 26.04.1 LTS 一键构建安装脚本
# 用法：./scripts/build-ubuntu.sh [安装|构建二选一]
#   默认：构建并安装（需要 sudo）
#   SKIP_INSTALL=1 ./scripts/build-ubuntu.sh  # 只构建不安装
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "==> [1/3] 构建 Rust 核心引擎 (qingci-core)"
cargo build --release --manifest-path "$ROOT/core/Cargo.toml"
CORE_LIB="$ROOT/core/target/release/libqingci_core.a"
test -f "$CORE_LIB" || { echo "错误：未找到 $CORE_LIB"; exit 1; }

echo "==> [2/3] 构建 Fcitx5 插件 (fcitx5-qingci)"
cmake -B "$ROOT/addon/build" -S "$ROOT/addon" \
    -DQINGCI_CORE_STATIC_LIB="$CORE_LIB" \
    -DQINGCI_CORE_DICT_PATH="$ROOT/core/data/qingci.dic" \
    -DCMAKE_BUILD_TYPE=Release
cmake --build "$ROOT/addon/build" -j"$(nproc)"

if [[ "${SKIP_INSTALL:-0}" == "1" ]]; then
    echo "==> 构建完成（未安装）。插件位于 $ROOT/addon/build/"
    exit 0
fi

echo "==> [3/3] 安装到系统（需要 sudo 权限）"
sudo cmake --install "$ROOT/addon/build"

cat <<'EOF'

安装完成。后续步骤：
  1. 重启 fcitx5：  killall fcitx5 && fcitx5 -d
  2. 打开 fcitx5 配置工具（fcitx5-configtool），在输入法列表中添加「青瓷拼音」
  3. 如系统尚未启用 fcitx5：im-config -n fcitx5 后注销重新登录
EOF
