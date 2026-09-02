# HLC タイムスタンプ・エンコーディング再設計(A.6-1 の設計ミス修正)

**正本**。この文書は `crates/aruaru-dist/src/hlc.rs`(2026-08-31 続き14 で新設)の
`HlcTimestamp::as_nanos()` に含まれる **u64 オーバーフローの設計ミス** を、
一次資料調査に基づいて再設計するためのもの。実装に触れる前に必ず読むこと。

関連: `docs/CONTROL_PLANE_REDESIGN.md` 付録 A.6-1(HLC を `closed_ts` /
`wal_service` / `multi_raft` のタイムスタンプ源として使う構想)。

---

## 1. 何が壊れているか(根本原因)

```rust
// hlc.rs 現状
const LOGICAL_BITS: u32 = 16;
pub fn as_nanos(&self) -> u64 {
    (self.pt << LOGICAL_BITS) | (self.l as u64 & LOGICAL_MASK)   // ← self.pt << 16
}
```

`hlc.rs` のドキュメントコメントは「`pt` はラップアラウンドしないフル 64bit の
Unix ナノ秒をそのまま保持する」と明記している。しかし Unix エポックからの
ナノ秒は 2026 年時点で **約 1.76 × 10¹⁸ ≈ 2⁶⁰·⁶**。これを `<< 16` すると
**約 2⁷⁶·⁶** となり u64(2⁶⁴)を大きく超える。

- debug ビルド: `attempt to shift left with overflow` で **panic**。
- release ビルド: 静かにラップして **順序が壊れた無意味な値** を返す。

`Hlc::now()` に `SystemTime::now().duration_since(UNIX_EPOCH).as_nanos() as u64`
を渡し、その戻り値の `.as_nanos()` を `closed_ts` 等へ渡そうとした瞬間に
顕在化する。**フル nanos の `pt` と 16bit の論理カウンタを 1 個の u64 へ
「pt を上へシフトして」詰める、という発想自体が成立しない。**

---

## 2. 一次資料調査(2026-09、英語)

| システム | 物理成分の単位 | 保持形式 | 単一整数エンコード |
|---|---|---|---|
| **CockroachDB** (`pkg/util/hlc/timestamp.go`) | **ナノ秒** (`WallTime int64`) | `WallTime` と `Logical int32` は**別フィールド**。パックしない | 無し。文字列化は `"%d.%010d"`(10進、`WallTime`.`Logical`) |
| YugabyteDB (HybridTime) | マイクロ秒 | 上位 52bit=物理(µs)、下位 12bit=論理 を 1 個の `uint64` へ | あり(µs 粒度に落として桁を空ける) |
| 「compact 64-bit HLC」系ライブラリ(muratbuffalo 解説ほか) | **ミリ秒** | 上位 48bit=物理(ms)、下位 16bit=論理 | あり(ms 粒度に落として桁を空ける) |

出典:
- CockroachDB `timestamp.go`(WallTime=ナノ秒・別フィールド・`AsOfSystemTime`
  は 10 進文字列):https://github.com/cockroachdb/cockroach/blob/master/pkg/util/hlc/timestamp.go
- Hybrid Logical Clocks 解説(48+16 の compact 表現、論理は最大 65536):
  http://muratbuffalo.blogspot.com/2014/07/hybrid-logical-clocks.html
- HLC 論文「Logical Physical Clocks and Consistent Snapshots in Globally
  Distributed Databases」(Kulkarni et al.)

**要点**: 物理成分と論理成分を 1 個の整数へ詰めたい場合、実在の設計は
すべて **物理成分の粒度を落として(µs / ms へ)下位ビットを空ける**。
「ナノ秒精度を保ったまま論理カウンタも詰める」を実現している実装は無い
(CockroachDB はそもそもパックせず別フィールドにしている)。

---

## 3. 設計方針の選択肢

### 案A: CockroachDB 方式 — パックをやめ、別フィールドのまま運ぶ
`closed_ts` / `wal_service` / `multi_raft` の「u64 ナノ秒」API を `HlcTimestamp`
(2 フィールド)を受ける形へ変更する。**最も正確**だが、これら 3 サブシステムと
その GraphQL/REST リゾルバ・テスト・side transport の型を全面的に変更する
大スライスになる(P3 で GraphQL 化したばかりの `closedTsAdvance` 等の
`String` 引数も 2 値化が必要)。

### 案B(採用): 物理成分を 65µs 粒度へ切り捨てて下位 16bit を空ける
`Hlc` の内部 `pt` を **常に `wall & !0xFFFF`**(下位 16bit をゼロにした
Unix ナノ秒、分解能 ≒ 65.536µs)として保持する。論理カウンタ `l` は
その空いた下位 16bit にちょうど収まる。

```
ordinal(u64) = pt | (l & 0xFFFF)
             = (wall_nanos & 0xFFFF_FFFF_FFFF_0000) | logical16
```

- **オーバーフローしない**: `pt` は下位 16bit が必ず 0。`l` はその 16bit を
  埋めるだけ。左シフトも桁上げも無い。値域は素の Unix ナノ秒とほぼ同じ。
- **厳密に単調増加**(逆転しない):
  - 同一 65µs バケット内: `pt` は不変、`l` が増える → ordinal 増加。
  - バケットが進む時: `pt` は `≥ 0x10000`(65536)ジャンプし、`l` は 0 へ
    リセット。旧バケットの ordinal は `pt_old | l`(`l < 65536`)なので、
    どれだけ `l` が大きくても新バケットの `pt_new`(`≥ pt_old + 65536`)を
    超えられない → **逆転不可能**。
- **論理カウンタ枯渇時の桁上げ**: 万一 1 バケット(65µs)内で `l` が
  65535 を超えたら(= 単一ノードで 65µs に 65536 回超の HLC イベント、
  現実にはほぼ起こらない)、`pt` を次バケット(`pt + 0x10000`)へ強制的に
  進めて `l = 0` にする。「未来を 1 バケットぶん先食いする」ことで
  単調性を絶対に壊さない。

### 案B の正直な代償(誇張しない)
1. **物理分解能は 65.536µs**。旧コメントが暗に主張していた「ナノ秒精度」は
   失われる。ただし HLC は「厳密な物理時刻」ではなく「因果順序を保った
   単調な版番号 + 実時刻の近似」であり、`closed_ts` の `target_lag` は
   **秒**単位(既定 3 秒)なので実用上の影響は無い。
2. **クロックスキュー上限(CockroachDB の `max_offset`)は引き続き未実装**
   ——`update()` はリモート値を常に受理する(続き14 からの制約、変更なし)。
3. **`plan_follower_read` のゲート精度**: HLC ordinal は真の壁時計に対して
   `[wall − 65535, wall + 65535]` ナノ秒(± 約 65µs)の範囲に収まる。
   したがって follower-read 可否判定が「約 65µs 先の commit を通す」あるいは
   「約 65µs 前の commit を弾く」ことがあり得る。`target_lag`(秒)に対して
   無視できる誤差。

---

## 4. 実装(案B)

### 4.1 `hlc.rs` の変更

```rust
const LOGICAL_BITS: u32 = 16;
const LOGICAL_MASK: u64 = (1 << LOGICAL_BITS) - 1;   // 0xFFFF
const PHYSICAL_MASK: u64 = !LOGICAL_MASK;            // 0xFFFF_FFFF_FFFF_0000

/// 壁時計ナノ秒を「バケット」へ切り捨てる(下位 16bit をゼロに)。
fn bucket(wall_nanos: u64) -> u64 { wall_nanos & PHYSICAL_MASK }
```

- `HlcTimestamp::as_nanos()` を **`as_ordinal()` へ改名**し、
  `(self.pt & PHYSICAL_MASK) | (self.l as u64 & LOGICAL_MASK)` を返す。
  `pt` は本来バケット整列済みだが、外部から任意値で構築された
  `HlcTimestamp` に対しても安全なよう `& PHYSICAL_MASK` を残す。
  互換のため `#[deprecated] pub fn as_nanos()` を `as_ordinal()` へ委譲する
  薄いエイリアスとして残す(既存呼び出し元を壊さない)。
- `Hlc::now(wall_now_nanos)`:
  ```
  b = bucket(wall_now_nanos)
  if b > pt            -> (b,  0)
  else if l < 0xFFFF   -> (pt, l + 1)
  else                 -> (pt + 0x10000, 0)   // 論理枯渇 → 次バケットを先食い
  ```
- `Hlc::update(remote, wall_now_nanos)`:
  ```
  b = bucket(wall_now_nanos)
  max_pt = max(pt, bucket(remote.pt), b)      // remote.pt もバケット整列して扱う
  new_l  = 論理成分の決定(下記)
  桁上げ: new_l > 0xFFFF なら max_pt += 0x10000; new_l = 0
  ```
  論理成分:
  - `max_pt == b` かつ `b` が `pt` と `remote_b` の両方より大 → `0`
  - `pt == max_pt` かつ `remote_b == max_pt` → `max(l, remote.l) + 1`
  - `pt == max_pt` → `l + 1`
  - `remote_b == max_pt` → `remote.l + 1`
  - それ以外 → `0`
- `Hlc::observe_ordinal(&self, remote_ordinal: u64, wall_now_nanos: u64)`
  を新設: side transport から届く「u64 ordinal」を
  `HlcTimestamp { pt: remote_ordinal & PHYSICAL_MASK,
  l: (remote_ordinal & LOGICAL_MASK) as u32 }` へ復号して `update()` する。
- `Hlc::now_ordinal(&self) -> u64`: `SystemTime::now()` を内部で読み
  `now(wall).as_ordinal()` を返す実運用向けの便利メソッド
  (テストは引き続き `now(wall_nanos)` を使う)。

### 4.2 サブシステムへの配線

- **`AdminState`(aruaru-server)/ `AdminCtx`(aruaru-graphql)** に
  `hlc: Arc<aruaru_dist::Hlc>` を追加。`main.rs` で同一インスタンスを共有
  (`topology` / `closed_ts` / `keyring` と同じパターン)。
- **`aruaru-graphql/src/admin_resolvers.rs`**: `now_unix_nanos()` を
  `closed_ts_advance` / `plan_follower_read` の「省略時 now」に使っている
  箇所を `ctx.data::<Arc<Hlc>>()?.now_ordinal()` へ差し替える
  (明示的に `now_nanos` 引数が渡された場合はそれを尊重、既存挙動維持)。
- **`aruaru-server/src/admin.rs`**: `closed_ts_receive`(side transport 受信)
  で、取り込んだ更新の最大 closed-ts を `state.hlc.observe_ordinal(max, wall)`
  へ渡してローカル HLC をリモート観測で前進させる。`closed_ts_publish` は
  変更なし(送るのは `closed_ts` が既に持っている値)。
- `wal_service` / `multi_raft` は現状 GraphQL リゾルバ側で明示的な
  タイムスタンプを要求しており(`walAppend` は LSN、`multiRaft*` は
  タイムスタンプを取らない)、**今回の配線対象は `closed_ts` 系のみ**。
  `wal_service` の `commitLSN` は LSN であって時刻ではないため HLC の
  対象外(この点は付録 A.6-1 の記述を precise 化する)。

### 4.3 テスト

- `hlc.rs`:
  - `as_ordinal_never_overflows_with_real_unix_nanos`:
    `pt = 1_760_000_000_000_000_000`(実 Unix ナノ秒相当)+ `l = 5` で
    `as_ordinal()` が panic せず、`< u64::MAX` かつ `pt` 近傍の値を返す。
  - `ordinal_is_monotonic_across_bucket_boundary_even_with_large_logical`:
    バケット境界を跨いでも(旧バケットで `l` を 65000 まで回した後
    `wall` を 1 バケット進めても)ordinal が厳密増加。
  - `now_carries_into_next_bucket_when_logical_saturates`:
    同一 `wall` で 70000 回 `now()` を呼び、ordinal が単調増加し続け、
    `pt` が途中で次バケットへ桁上げされる。
  - `observe_ordinal_pulls_local_clock_forward`:
    ローカルより先の ordinal を `observe_ordinal` した後、`now()` の
    ordinal がそれを上回る。
  - 既存 `as_nanos_preserves_ordering_between_timestamps` はバケット整列した
    `pt` 値(`0x10000` / `0x20000`)へ更新。
- `aruaru-graphql`: `closed_ts_advance` を `now_nanos` 省略で呼ぶと HLC 由来の
  ordinal で前進し、2 連続呼び出しで closed-ts が厳密に増えること。
- 実プロセス HTTP E2E: `closedTsAdvance`(引数省略)→ `closedTimestamp` が
  HLC ordinal で前進、`planFollowerRead` が commit を正しくゲートする。

---

## 5. フェーズ(この文書のスコープ内)

1. **P-HLC-1**: `hlc.rs` を案B へ書き換え + テスト(この文書の 4.1・4.3 前半)。
   → 単体で完結、`closed_ts` 等には未接続。
2. **P-HLC-2**: `AdminState` / `AdminCtx` へ `Arc<Hlc>` 共有 + `admin_resolvers`
   の `now_unix_nanos()` 差し替え + `closed_ts_receive` の `observe_ordinal`
   + GraphQL/実HTTP E2E(4.2・4.3 後半)。
3. **P-HLC-3(将来)**: 案A(パックをやめ 2 フィールドで運ぶ)への移行を
   検討するか、`max_offset` 相当のスキュー上限を入れるか。今回はやらない。

案A への全面移行は「投げやり・その場しのぎ禁止」の原則に照らしても
**過大**(P3 で GraphQL 化したばかりの API を再び全面変更する)。案B は
オーバーフローという実害を完全に除去し、65µs 粒度という明示的な代償だけを
負う——実在の compact HLC 実装(48+16 の ms 版)と同種の割り切りであり、
「一から設計し直した」結果として妥当。
