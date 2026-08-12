#!/bin/sh
# aruaru-db インストールスクリプト(systemdを使う主要Linuxディストリ共通)。
#
# 使い方(tar.gzを手動展開せずそのまま実行できる):
#   curl -fsSL https://raw.githubusercontent.com/aon-co-jp/aruaru-db/main/install.sh -o install.sh
#   sudo sh install.sh
#
# 実バグ修正・簡易化(2026-08-12、ユーザー指示「aruaru-dbやaruaru-llm
# などのインストールもより簡単にして」への対応、`open-english`の
# 「Setup aruaru-db」導線からの実地調査で発覚):
# 1. **URLの誤り**: これまで本コメント・README.md・install.ps1が参照
#    していたGitHubリポジトリURLは`aruaru-db/aruaru-db`という誤った
#    組織名で、実際のリポジトリ(`aon-co-jp/aruaru-db`、`git remote -v`
#    で確認)とは異なっていた——手順通りに実行すると404になる状態が
#    ここまで放置されていた。
# 2. **手動手順が多すぎた**: 従来はtar.gzを手動でダウンロード・展開して
#    このスクリプトを実行し、さらに末尾で`systemctl enable --now`を
#    ユーザー自身が別途実行する必要があった。今回、(a)`aruaru-server`
#    バイナリが見つからない場合はGitHub Releases APIから最新のLinux
#    向けtar.gzを自動取得・自動展開し、(b)`--no-enable`を渡さない限り
#    `systemctl enable --now`まで自動実行するよう変更した。

set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BIN_SRC="${SCRIPT_DIR}/aruaru-server"
INSTALL_DIR="/usr/local/bin"
DATA_DIR="/var/lib/aruaru-db"
SERVICE_FILE="/etc/systemd/system/aruaru-db.service"
NO_ENABLE=0

for arg in "$@"; do
    case "$arg" in
        --no-enable) NO_ENABLE=1 ;;
    esac
done

if [ "$(id -u)" -ne 0 ]; then
    echo "root権限で実行してください(例: sudo sh install.sh)" >&2
    exit 1
fi

if [ ! -f "$BIN_SRC" ]; then
    echo "==> aruaru-server バイナリが見つからないため、GitHub Releasesから最新版を自動取得します"
    API_URL="https://api.github.com/repos/aon-co-jp/aruaru-db/releases/latest"
    ASSET_URL="$(curl -fsSL "$API_URL" | grep -o '"browser_download_url": *"[^"]*linux-x86_64[^"]*tar\.gz"' | head -n1 | sed -E 's/.*"(https:[^"]+)"/\1/')"
    if [ -z "$ASSET_URL" ]; then
        echo "Linux向けリリースアセットが見つかりませんでした。https://github.com/aon-co-jp/aruaru-db/releases から手動で取得してください。" >&2
        exit 1
    fi
    ARCHIVE="${SCRIPT_DIR}/aruaru-db-download.tar.gz"
    curl -fsSL "$ASSET_URL" -o "$ARCHIVE"
    tar -xzf "$ARCHIVE" -C "$SCRIPT_DIR"
    rm -f "$ARCHIVE"
    if [ ! -f "$BIN_SRC" ]; then
        echo "ダウンロード・展開後もaruaru-server バイナリが見つかりません($BIN_SRC)。アーカイブの内部構成が変わった可能性があります。" >&2
        exit 1
    fi
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

if [ "$NO_ENABLE" -eq 1 ]; then
    echo "==> --no-enable指定のため、サービスの有効化・起動はスキップしました。手動で行う場合:"
    echo "    sudo systemctl edit aruaru-db  # 環境変数を追記"
    echo "    sudo systemctl enable --now aruaru-db"
else
    echo "==> サービスを有効化・起動します"
    systemctl enable --now aruaru-db
    echo "==> 完了。systemctl status aruaru-db で状態を確認できます。"
fi
