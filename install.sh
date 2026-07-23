#!/bin/sh
# aruaru-db インストールスクリプト(systemdを使う主要Linuxディストリ共通)。
#
# 使い方:
#   curl -fsSL https://github.com/aruaru-db/aruaru-db/releases/latest/download/aruaru-db-linux-x86_64.tar.gz | tar xz
#   sudo ./install.sh

set -eu

BIN_SRC="$(dirname "$0")/aruaru-server"
INSTALL_DIR="/usr/local/bin"
DATA_DIR="/var/lib/aruaru-db"
SERVICE_FILE="/etc/systemd/system/aruaru-db.service"

if [ "$(id -u)" -ne 0 ]; then
    echo "root権限で実行してください(例: sudo ./install.sh)" >&2
    exit 1
fi

if [ ! -f "$BIN_SRC" ]; then
    echo "aruaru-server バイナリが見つかりません($BIN_SRC)。同梱のtar.gzを展開したディレクトリで実行してください。" >&2
    exit 1
fi

echo "==> バイナリを ${INSTALL_DIR}/aruaru-server へ配置"
install -m 755 "$BIN_SRC" "${INSTALL_DIR}/aruaru-server"

echo "==> データディレクトリを作成(${DATA_DIR})"
mkdir -p "$DATA_DIR"

if [ ! -f "$SERVICE_FILE" ]; then
    echo "==> systemdサービスを作成(${SERVICE_FILE})"
    cat > "$SERVICE_FILE" << EOF
[Unit]
Description=aruaru-db - Rust製 fjall/redb + DataFusion + openraft の分散DB
After=network.target

[Service]
Type=simple
Environment=ARUARU_DATA_DIR=${DATA_DIR}
# PostgreSQL互換(pgwire)/GraphQL等の待受設定は環境変数で指定すること。
# 例:
#   Environment=ARUARU_PG_BIND=0.0.0.0:5432
ExecStart=${INSTALL_DIR}/aruaru-server
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF
    systemctl daemon-reload
else
    echo "==> 既存のsystemdサービスが見つかったため上書きしません(${SERVICE_FILE})"
fi

echo "==> 完了。次のコマンドで起動してください:"
echo "    sudo systemctl edit aruaru-db  # 環境変数を追記"
echo "    sudo systemctl enable --now aruaru-db"
