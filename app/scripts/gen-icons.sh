#!/usr/bin/env bash
# 由 assets/icons 源图生成 Tauri 打包图标集与托盘图标。
# 源图：../assets/icons/Temporule_AppIcon.png（1024）
# 产物：src-tauri/icons/{icon.icns, 32x32.png, 128x128.png, ..., tray.png}
# 依赖：macOS 自带 sips / iconutil。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$(dirname "$SCRIPT_DIR")"
SRC_ICON="$APP_DIR/../assets/icons/Temporule_AppIcon.png"
TRAY_SRC="$APP_DIR/../assets/icons/Temporule_64.png"
OUT_DIR="$APP_DIR/src-tauri/icons"
ICONSET_DIR="$OUT_DIR/Temporule.iconset"

if [[ ! -f "$SRC_ICON" ]]; then
  echo "缺少源图标：$SRC_ICON" >&2
  exit 1
fi
if [[ ! -f "$TRAY_SRC" ]]; then
  echo "缺少托盘源图标：$TRAY_SRC" >&2
  exit 1
fi

mkdir -p "$OUT_DIR"
rm -rf "$ICONSET_DIR"
mkdir -p "$ICONSET_DIR"

# iconutil 需要的 iconset 尺寸清单
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$SRC_ICON" --out "$ICONSET_DIR/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$SRC_ICON" --out "$ICONSET_DIR/icon_${size}x${size}@2x.png" >/dev/null
done

iconutil -c icns "$ICONSET_DIR" -o "$OUT_DIR/icon.icns"
rm -rf "$ICONSET_DIR"

# Tauri bundle 需要的 png 集
for size in 32 128 256 512; do
  sips -z "$size" "$size" "$SRC_ICON" --out "$OUT_DIR/${size}x${size}.png" >/dev/null
done
sips -z 64 64 "$SRC_ICON" --out "$OUT_DIR/128x128@2x.png" >/dev/null
sips -z 512 512 "$SRC_ICON" --out "$OUT_DIR/256x256@2x.png" >/dev/null
sips -z 1024 1024 "$SRC_ICON" --out "$OUT_DIR/512x512@2x.png" >/dev/null

# 托盘图标（64px）
cp "$TRAY_SRC" "$OUT_DIR/tray.png"

echo "图标生成完成：$OUT_DIR"
