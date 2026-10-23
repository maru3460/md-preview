#!/bin/bash
# md で site/ のファイルを 1 つ開き、数秒待って閉じ、その間に記録用のサーバへ来た接続を出す。
#
# 使い方: run.sh <off|on> [site/ の中のファイル（既定 probe.html）]
#   off … 既定（「文書を信頼する」が OFF）
#   on  … 「文書を信頼する」が ON
# 環境変数 MDBIN で叩く md を選ぶ（既定は PATH の md）。WAIT で待つ秒数を変えられる（既定 7）。
#
# 設定は使い捨ての MD_CONFIG_DIR に書くので、日常の md の設定には触れない。MD_NO_IPC で
# 既存の窓へ転送せず、MD_NO_DETACH で前景に留めて、ここから終了させる。
set -eu
E=$(cd "$(dirname "$0")" && pwd)
MDBIN=${MDBIN:-md}
WAIT=${WAIT:-7}
mode=${1:-}
file=${2:-probe.html}
case "$mode" in
  off|on) ;;
  *) echo "使い方: $0 <off|on> [file]" >&2; exit 2 ;;
esac
[ -f "$E/server.log" ] || { echo "先に別の端末で python3 -u logger.py > server.log を動かす" >&2; exit 1; }

cfg=$(mktemp -d)
trap 'rm -rf "$cfg"' EXIT
printf 'trust-documents=%s\n' "$mode" > "$cfg/settings"

n0=$(wc -l < "$E/server.log")
env MD_NO_IPC=1 MD_NO_DETACH=1 MD_NO_BUNDLE=1 MD_CONFIG_DIR="$cfg" MD_SOCKET="$cfg/md.sock" \
  "$MDBIN" "$E/site/$file" > "$E/app-$mode.log" 2>&1 &
pid=$!
sleep "$WAIT"
kill "$pid" 2>/dev/null || true
sleep 1
kill -9 "$pid" 2>/dev/null || true

echo "=== $mode（trust-documents=$mode）$file"
echo "--- 記録用のサーバに来た接続:"
tail -n +$((n0 + 1)) "$E/server.log"
echo "--- （アプリの出力は app-$mode.log）"
