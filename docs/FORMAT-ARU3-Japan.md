# aruaru-db 三重書き込みフォーマット仕様 (ARU3 v1)

open-aruaru (iLumi) プロジェクト — Layer 3 ストレージのオンディスク形式。
課金アイテム・金融データを「ディスク上でも消失させない」ための設計。

## 設計原則

1. **追記のみ (append-only)** — 上書きしない。破損は「途中で切れる」形でしか起きない。
2. **レコード単体で自己検証可能** — ヘッダ+本体+チェックサムが1レコードに完結。
   ファイル先頭から読めなくても、マジックナンバー走査で任意位置から復元できる。
3. **三重 = 3レプリカ × 各レプリカ内二重チェックサム** — 経路破損とディスク破損を分離検出。
4. **金融レコードは fsync 境界を持つ** — FINANCE フラグ付きレコードは
   書き込み後 fdatasync 完了までACKしない。

## レプリカ配置

```
replica-0/  wal/  segments/   ← 正 (プライマリ書き込み先だが全レプリカ対等)
replica-1/  wal/  segments/
replica-2/  wal/  segments/
```

3レプリカは物理的に別ディスク/別ノード推奨。開発時は
`f:\open-aruaru\aruaru-db\data\replica-{0,1,2}` に配置し、e: へ robocopy でバックアップ。

## セグメントファイル

- ファイル名: `seg-{u64 開始txn連番:020}.aru3` (例 `seg-00000000000000000001.aru3`)
- 最大 256 MiB。超えたらローテーション。
- ファイルヘッダ 64 bytes + レコード列。

### ファイルヘッダ (64 bytes)

| offset | size | 内容 |
|---|---|---|
| 0  | 8  | マジック `ARU3SEG\0` |
| 8  | 2  | フォーマットバージョン (u16 LE) = 1 |
| 10 | 1  | replica_id (0-2) |
| 11 | 5  | 予約 (zero) |
| 16 | 8  | 開始txn連番 (u64 LE) |
| 24 | 8  | 作成UNIX時刻 ms (u64 LE) |
| 32 | 32 | ヘッダ自身のBLAKE3 (offset 0-31 を対象) |

## レコードフォーマット

```
┌──────────────────────────────────────┐
│ RECORD MAGIC  "AR3R"           4 B   │
│ record_len (u32 LE, 本体長)     4 B   │  ← ヘッダ+ペイロード+フッタの総長
│ txn_seq    (u64 LE, 連番)       8 B   │
│ txn_id     (UUIDv7)            16 B   │
│ flags      (u16 LE)             2 B   │  bit0: FINANCE(fsync境界)
│                                       │  bit1: TOMBSTONE(論理削除)
│                                       │  bit2: REPAIR(修復再送で書かれた)
│ key_len    (u16 LE)             2 B   │
│ timestamp_ms (u64 LE)           8 B   │
│ ── ここまで固定ヘッダ 44 B ──          │
│ key        (UTF-8)         key_len B  │
│ payload    (bytes)              可変   │
│ ── フッタ ──                          │
│ payload_checksum BLAKE3        32 B   │  ← Layer1由来。経路破損検出
│ record_checksum  BLAKE3        32 B   │  ← MAGIC〜payload_checksumまで全体。
│                                       │    ディスク破損検出 (二重チェックサム)
└──────────────────────────────────────┘
```

- **二重チェックサムの意味**: `payload_checksum` は open-web-server が計算した値を
  そのまま保存(エンドツーエンド検証)。`record_checksum` は aruaru-db が
  書き込み直前に計算(ローカル完全性)。読み取り時は両方を検証し、
  どちらが壊れたかで「ネット経路破損」か「ディスク破損」かを切り分ける。
- アライメント: レコード開始位置は 8 byte 境界にパディング (zero fill)。

## WAL (Write-Ahead Log)

- セグメント書き込み前に `wal/wal-{date}.aru3w` へ同一レコードを追記。
- WAL は fdatasync 後にセグメントへ反映。セグメント側の fsync 完了で WAL 該当分は回収可能。
- FINANCE レコード: WAL fsync → セグメント書き込み → セグメント fsync → ACK の順を厳守。
- GAME レコード: group commit (最大 2ms または 64レコードでまとめて fsync)。

## クラッシュリカバリ手順

1. 各セグメント末尾から `record_checksum` 不一致のレコードを切り詰め (torn write 除去)。
2. WAL を先頭から再生し、セグメントに無い txn_seq を再適用。
3. 3レプリカ間で txn_seq の最大値を比較し、遅れているレプリカへ差分転送 (anti-entropy)。
4. `payload_checksum` 不一致レコードを検出したら、他レプリカの多数決値で置換
   (置換は新レコード追記 + 旧レコードは REPAIR フラグ付き参照で無効化)。

## 読み取り (多数決)

1. 3レプリカから key の最新レコード (最大 txn_seq) を取得。
2. `record_checksum` 検証 → 失格レプリカは除外し修復キューへ。
3. `payload_checksum` の多数決 (2/3以上一致) を採用値とする。
4. `require_full_quorum=true` (金融残高照会) は 3/3 一致必須。不一致なら
   即時 anti-entropy を起動し、収束後に応答する。

## インデックス

- 各セグメントに対しサイドカー `seg-*.idx` (key BLAKE3 16B prefix → file offset)。
- idx は再生成可能な派生データ。破損時はセグメント走査で再構築。

## バージョニング

- フォーマット変更は version をインクリメントし、読み取りは常に v1..=vN を受理。
- 旧バージョンのセグメントは compaction 時に最新形式へ書き直す。
