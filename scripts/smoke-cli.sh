#!/usr/bin/env bash
# Temporule CLI 冒烟：全链路命令走一遍（临时数据目录隔离，不碰真实数据）
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export TEMPORULE_DATA_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMPORULE_DATA_DIR"' EXIT

BIN="$ROOT/target/debug/temporule"
[ -x "$BIN" ] || { echo "构建 debug CLI…"; cargo build -p temporule-cli >/dev/null; }

step() { echo "── $*"; }

step "add（活动提醒：每天 9-18 点每 2 小时弹窗）"
"$BIN" add --title 活动提醒 --daily --every 2h --window 9:00-18:00 --popup --auto-dismiss 30

step "add（喝水：通知 + 声音）"
"$BIN" add --title 喝水 --daily --every 1h --window 9:00-18:00 --notification --sound

step "add（周会：每周一 9 点）"
"$BIN" add --title 周会 --weekly mon --at 9:00 --notification

step "list"
"$BIN" list

step "list --json（字段检查）"
"$BIN" list --json | grep -q '"title"' && echo "json ok"

step "edit（喝水改为每 45 分钟）"
ID2="$("$BIN" list --json | python3 -c 'import json,sys; print([r["id"] for r in json.load(sys.stdin) if r["title"]=="喝水"][0])')"
"$BIN" edit "$ID2" --every 45m

step "disable + enable（周会）"
ID3="$("$BIN" list --json | python3 -c 'import json,sys; print([r["id"] for r in json.load(sys.stdin) if r["title"]=="周会"][0])')"
"$BIN" disable "$ID3"
"$BIN" enable "$ID3"

step "snooze（活动提醒 +10 分钟）"
ID1="$("$BIN" list --json | python3 -c 'import json,sys; print([r["id"] for r in json.load(sys.stdin) if r["title"]=="活动提醒"][0])')"
"$BIN" snooze "$ID1" 10m

step "history"
"$BIN" history --limit 5

step "remove（周会，-y）"
"$BIN" remove "$ID3" -y

step "remove 无 -y（应拒绝，退出 2）"
set +e
"$BIN" remove "$ID1" </dev/null 2>/dev/null
code=$?
set -e
if [ "$code" -eq 2 ]; then
  echo "拒绝正确（退出码 2）"
else
  echo "FAIL：remove 无 -y 应退出 2，实际退出 $code" >&2; exit 1
fi

step "互斥 flag（应退出 2）"
if "$BIN" edit "$ID1" --popup --force --auto-dismiss 10 2>/dev/null; then
  echo "FAIL：互斥 flag 未拒绝" >&2; exit 1
else
  echo "拒绝正确"
fi

step "最终 list"
"$BIN" list

echo ""
echo "SMOKE OK"
