#!/bin/sh
# aruaru-db アンインストールスクリプト(systemdを使う主要Linux
# ディストリ共通、install.shと対になる新規スクリプト、2026-07-30追記)。
#
# **安全性の設計方針(最重要)**: `install.sh`が作成する
# `/var/lib/aruaru-db`(ARUARU_DATA_DIR)には実際のDBデータ(fjall/redb
# のストレージファイル)が入っている。このスクリプトは**データ
# ディレクトリを絶対に削除しない**——バイナリとsystemdサービス定義
# のみを削除し、データディレクトリの存在と場所を明示して終了する。
# 「別バージョンをインストールし直す/アンインストールする際に既存データや
# HDDのデータへ悪影響を与えないように」というユーザー指示への対応。

set -eu

INSTALL_DIR="/usr/local/bin"
BIN_PATH="${INSTALL_DIR}/aruaru-server"
DATA_DIR="/var/lib/aruaru-db"
SERVICE_NAME="aruaru-db"
SERVICE_FILE="/etc/systemd/system/${SERVICE_NAME}.service"

if [ "$(id -u)" -ne 0 ]; then
    echo "root権限で実行してください(例: sudo ./uninstall.sh)" >&2
    exit 1
fi

if systemctl is-active --quiet "$SERVICE_NAME" 2>/dev/null; then
    echo "==> ${SERVICE_NAME}.service を停止"
    systemctl stop "$SERVICE_NAME"
fi
if systemctl is-enabled --quiet "$SERVICE_NAME" 2>/dev/null; then
    echo "==> ${SERVICE_NAME}.service を無効化"
    systemctl disable "$SERVICE_NAME"
fi
if [ -f "$SERVICE_FILE" ]; then
    echo "==> ${SERVICE_FILE} を削除"
    rm -f "$SERVICE_FILE"
    systemctl daemon-reload
fi

if [ -f "$BIN_PATH" ]; then
    echo "==> ${BIN_PATH} を削除"
    rm -f "$BIN_PATH"
else
    echo "==> ${BIN_PATH} は見つかりませんでした(既に削除済み)"
fi

echo "==> 完了。"
if [ -d "$DATA_DIR" ]; then
    echo "    データディレクトリ ${DATA_DIR} は意図的に削除していません。"
    echo "    別バージョンの再インストール時にもこのデータはそのまま利用されます。"
    echo "    データも含めて完全に削除したい場合のみ、内容を確認の上で手動で"
    echo "    'rm -rf ${DATA_DIR}' を実行してください(このスクリプトは自動実行しません)。"
else
    echo "    データディレクトリ ${DATA_DIR} は見つかりませんでした。"
fi
