#!/bin/sh
# Nagi（macOS 用の日本語入力）のアンインストール。
#   curl -fsSL https://nagi.shiki-01.com/uninstall.sh | sh
#   設定・学習データ・辞書・モデルも削除する場合（確認あり）:
#   curl -fsSL https://nagi.shiki-01.com/uninstall.sh | sh -s -- --purge
#   非対話で削除する場合（確認を省く）:
#   curl -fsSL https://nagi.shiki-01.com/uninstall.sh | sh -s -- --purge --yes
#
# 動作中の Nagi（NagiIMEApp と、その子の imserver）を止め、~/Library/Input Methods/Nagi.app を削除する。
# 設定・学習データ・辞書・モデル（~/Library/Application Support/com.shiki.nagi/）は既定では残し、
# --purge を付けたときだけ削除する（削除前に確認する。--yes で確認を省く）。
# 管理者権限は使わない。ネットワークには接続しない。何度実行しても失敗しない（無いものはスキップ）。
#
# 入力ソースの一覧から Nagi を外すには、ログアウトして再ログインする必要がある。
set -eu

APP="$HOME/Library/Input Methods/Nagi.app"
DATA_DIR="$HOME/Library/Application Support/com.shiki.nagi"

say() { printf '%s\n' "nagi: $*"; }
fail() { printf '%s\n' "nagi: エラー: $*" >&2; exit 1; }

usage() {
  cat <<'EOF'
使い方: uninstall.sh [--purge [--yes]]
  既定        Nagi.app と動作中のプロセスを削除し、設定・学習データ・辞書・モデルは残す
  --purge     設定・学習データ・辞書・モデルも削除する（確認プロンプトあり）
  --yes       --purge の確認を省く（非対話で使うとき）
例: curl -fsSL https://nagi.shiki-01.com/uninstall.sh | sh -s -- --purge --yes
EOF
}

purge=0
yes=0
for arg in "$@"; do
  case "$arg" in
    --purge) purge=1 ;;
    --yes) yes=1 ;;
    -h|--help) usage; exit 0 ;;
    *) fail "不明な引数です: $arg（使い方は --help）" ;;
  esac
done
if [ "$yes" = 1 ] && [ "$purge" != 1 ]; then
  say "--yes は --purge と一緒に指定したときだけ意味があります（無視します）"
fi

[ "$(uname -s)" = "Darwin" ] || fail "macOS 専用です"

# 削除を伴う確認は、何か変更する前に済ませる（断ったときや非対話で止まったときに途中まで消さないため）
confirm_purge() {
  if [ ! -e "$DATA_DIR" ]; then
    say "設定・学習データ・辞書・モデルは見つかりませんでした（削除は不要）"
    return 0
  fi
  [ "$yes" = 1 ] && return 0
  # 標準入力は curl からのパイプなので、確認は端末（/dev/tty）から読む
  if ! (exec </dev/tty) 2>/dev/null; then
    fail "端末で確認できません。削除するなら --purge に加えて --yes を指定してください（何も削除していません）"
  fi
  printf '%s\n' "次のデータを削除します（元に戻せません）: $DATA_DIR" \
    "  設定・学習データ（変換の学習内容を含む）・辞書・変換モデル" >/dev/tty
  printf '%s' "続けますか？ [y/N] " >/dev/tty
  answer=""
  read -r answer </dev/tty || answer=""
  case "$answer" in
    y|Y|yes|YES) ;;
    *) say "データは削除せず、アプリだけ削除します"; purge=0 ;;
  esac
}

stop_nagi() {
  if pgrep -x NagiIMEApp >/dev/null 2>&1 || pgrep -f "$APP/Contents/MacOS/imserver" >/dev/null 2>&1; then
    say "動作中の Nagi を停止します"
    # imserver はアプリ同梱のものだけを止める（開発中に別の場所で起動している imserver には触れない）
    pkill -x NagiIMEApp >/dev/null 2>&1 || true
    pkill -f "$APP/Contents/MacOS/imserver" >/dev/null 2>&1 || true
    sleep 1
    if pgrep -x NagiIMEApp >/dev/null 2>&1; then
      say "警告: Nagi を停止できませんでした。ログアウトすると終了します"
    fi
  fi
}

if [ "$purge" = 1 ]; then
  confirm_purge
fi

stop_nagi

if [ -e "$APP" ]; then
  rm -rf "$APP" || fail "Nagi.app を削除できません: $APP"
  say "Nagi.app を削除しました: $APP"
else
  say "Nagi.app は見つかりませんでした（スキップ）: $APP"
fi
# 入力メニューの一覧を更新させる（残ることがあるが、その場合は次のログアウト後に消える）
killall TextInputMenuAgent >/dev/null 2>&1 || true

if [ "$purge" = 1 ]; then
  if [ -e "$DATA_DIR" ]; then
    rm -rf "$DATA_DIR" || fail "データを削除できません: $DATA_DIR"
    say "設定・学習データ・辞書・モデルを削除しました: $DATA_DIR"
  else
    say "データは見つかりませんでした（スキップ）: $DATA_DIR"
  fi
elif [ -e "$DATA_DIR" ]; then
  say "設定・学習データ・辞書・モデルは残しています: $DATA_DIR"
  say "  削除するには --purge を付けて再実行してください"
fi

cat <<'EOF'

次の手順:
  入力ソースの一覧から Nagi を外すには、ログアウトして再ログインしてください。
EOF
