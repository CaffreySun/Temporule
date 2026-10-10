#!/usr/bin/env bash
# Temporule CLI 安装脚本（规范：docs/CONVENTIONS.md §9）
# 目标目录按序探测：brew --prefix → ~/.local/bin；均不可写则打印 PATH 指南后失败
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN_NAME="temporule"

echo "==> 构建 release CLI…"
cargo build --release -p temporule-cli

SRC="$ROOT/target/release/$BIN_NAME"

pick_dir() {
  if command -v brew >/dev/null 2>&1; then
    local p
    p="$(brew --prefix 2>/dev/null)/bin"
    [ -d "$p" ] && [ -w "$p" ] && { echo "$p"; return 0; }
  fi
  local d="$HOME/.local/bin"
  mkdir -p "$d" 2>/dev/null || true
  [ -d "$d" ] && [ -w "$d" ] && { echo "$d"; return 0; }
  return 1
}

if ! DIR="$(pick_dir)"; then
  cat >&2 <<'EOF'
错误：找不到可写的安装目录（无 Homebrew 且 ~/.local/bin 不可写）。
请手动执行：
  cargo build --release -p temporule-cli
  ln -s "$PWD/target/release/temporule" /你的/PATH/目录/temporule
EOF
  exit 1
fi

DEST="$DIR/$BIN_NAME"
if [ -e "$DEST" ] || [ -L "$DEST" ]; then
  if [ "$(readlink "$DEST" 2>/dev/null)" = "$SRC" ]; then
    echo "==> 已安装（$DEST 指向同一构建产物），跳过"
  else
    echo "==> 已存在 $DEST（非本仓库构建），请先手动移除后重试" >&2
    exit 1
  fi
else
  ln -s "$SRC" "$DEST"
  echo "==> 已安装：$DEST -> $SRC"
fi

case ":$PATH:" in
  *":$DIR:"*) ;;
  *) echo "--> 注意：$DIR 不在 PATH 中，请将其加入你的 shell 配置" ;;
esac

"$DEST" list >/dev/null 2>&1 || true
echo "==> 完成。运行 'temporule list' 验证。"
