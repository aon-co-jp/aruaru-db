// aruaru-db Android版: リモートのaruaru-dbクラスタ(admin API)を監視・管理する
// クライアントアプリ。open-web-server/android/(参照実装)と同じGradle構成
// パターンに従う。
//
// 位置づけ(重要): このアプリはaruaru-db本体(Rust製分散DBサーバー)を
// Android上で動かすものではない。管理API(`/admin/cluster`等、
// crates/aruaru-server/src/admin.rs)を持つ稼働中のaruaru-dbクラスタへ
// リモート接続し、疎通確認・クラスタ状態表示を行うモニタリング/管理
// クライアントとして設計する(詳細は`../CLAUDE.md`のHANDOFF節参照)。
plugins {
    id("com.android.application") version "8.7.2" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
}
