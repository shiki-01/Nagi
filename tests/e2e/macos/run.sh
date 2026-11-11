#!/usr/bin/env bash
# macOS の UI E2E を実行する薄いラッパー（macOS 専用。Xcode 不要、SwiftPM + Command Line Tools のみ）。
#
# 事前に Nagi.app を作っておくこと（LLM なしで十分）:
#   cd core && cargo xtask bundle-macos
#
# 使い方: tests/e2e/macos/run.sh [nagi-e2e の追加引数 ...]（例: --filter romaji-basic）
# 環境変数:
#   NAGI_APP      Nagi.app の場所（既定: core/target/macos-bundle/Nagi.app）
#   NAGI_FIXTURE_DATA  e2e-fixture-data の出力先（既定: $NAGI_E2E_OUT/fixture-data。無ければ自動生成）
#   NAGI_E2E_OUT  成果物（results.json / screenshots/ / host.log）の出力先（既定: tests/e2e/macos/out）
#
# 注意: ~/Library/Input Methods/Nagi.app を上書きし、実行中は launchctl setenv で
#       NAGI_CONFIG_DIR / NAGI_USER_DIR / NAGI_DATA_DIR を一時的に設定する（終了時に解除）。
#       手元の Nagi を日常利用している環境では実行しないこと。
#       キー送出にはアクセシビリティの許可と、ログイン済みの GUI セッションが必要。
set -euo pipefail

if [ "$(uname -s)" != "Darwin" ]; then
  echo "error: macOS 専用です (uname: $(uname -s))" >&2
  exit 2
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
APP="${NAGI_APP:-$ROOT/core/target/macos-bundle/Nagi.app}"
OUT="${NAGI_E2E_OUT:-$ROOT/tests/e2e/macos/out}"

if [ ! -d "$APP" ]; then
  echo "error: Nagi.app が見つかりません: $APP" >&2
  echo "  先に実行: (cd core && cargo xtask bundle-macos)" >&2
  exit 2
fi

# 語彙なしの最小辞書（ユーザー辞書の重ね合わせに辞書本体が要る）。NAGI_FIXTURE_DATA 指定済みならそれを使う
DATA="${NAGI_FIXTURE_DATA:-$OUT/fixture-data}"
if [ ! -f "$DATA/dict/nagi-dict.bin" ]; then
  (cd "$ROOT/core" && cargo xtask e2e-fixture-data --out "$DATA")
fi

cd "$ROOT/frontend-macos"
swift build -c release --product NagiTestHost
swift build -c release --product nagi-e2e
BIN="$(swift build -c release --show-bin-path)"

mkdir -p "$OUT"
"$BIN/nagi-e2e" \
  --scenarios "$ROOT/tests/e2e/scenarios" \
  --fixtures "$ROOT/tests/e2e/fixtures" \
  --app "$APP" \
  --data-dir "$DATA" \
  --test-host "$BIN/NagiTestHost" \
  --out "$OUT" \
  "$@"
