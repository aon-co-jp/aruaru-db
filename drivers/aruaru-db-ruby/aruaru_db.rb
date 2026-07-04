# aruaru-DB Ruby ドライバー
#
# パッケージ名: aruaru-db-ruby (RubyGems)
# gem install aruaru-db-ruby
# 内部依存: gem 'pg', '~> 1.5'
#
# aruaru-DB は PostgreSQL ワイヤ互換のため pg gem がそのまま使えます。
# このドライバーは Git-on-SQL 操作を型付き API で包むラッパーです。
#
# 使用例:
#   require 'aruaru_db'
#   db = AruaruDB::Client.connect
#   db.branch('feature/ruby-test')
#   db.execute("CREATE TABLE tasks (id INT, title TEXT)")
#   puts db.commit('Add tasks via aruaru-db-ruby')
#   db.close

require 'pg'

module AruaruDB
  # コミットログのエントリ
  CommitEntry = Struct.new(:id, :short_id, :author, :message, :timestamp, :root_hash,
                            keyword_init: true)

  # 差分統計
  DiffStat = Struct.new(:added, :removed, :modified, keyword_init: true)

  # aruaru-DB クライアント
  #
  # Ruby の慣例に従い、モジュール AruaruDB 内に Client クラスを定義する。
  # フルパスは AruaruDB::Client。
  class Client
    # @param host     [String]  ホスト名 (既定: 'localhost')
    # @param port     [Integer] ポート番号 (既定: 5432)
    # @param dbname   [String]  データベース名 (既定: 'aruaru')
    # @param user     [String]  ユーザー名 (既定: 'root')
    # @param password [String]  パスワード
    def initialize(host: 'localhost', port: 5432, dbname: 'aruaru',
                   user: 'root', password: '')
      @conn = PG.connect(host:, port:, dbname:, user:, password:)
    end

    # URL から接続する ("postgres://root@localhost:5432/aruaru")
    def self.connect(url: 'postgres://root@localhost:5432/aruaru', **opts)
      if url && opts.empty?
        instance = allocate
        instance.instance_variable_set(:@conn, PG.connect(url))
        instance
      else
        new(**opts)
      end
    end

    # ── Git-on-SQL ──────────────────────────────────────────

    # ブランチを作成する
    def branch(name)
      @conn.exec_params("SELECT aruaru_branch($1)", [name])
      nil
    end

    # ブランチを切り替える
    def checkout(name)
      @conn.exec_params("SELECT aruaru_checkout($1)", [name])
      nil
    end

    # 現在のブランチ名を返す
    def current_branch
      @conn.exec("SELECT aruaru_current_branch()").getvalue(0, 0)
    end

    # コミットしてコミット ID を返す
    def commit(message)
      result = @conn.exec_params("SELECT aruaru_commit($1) AS commit_id", [message])
      result.getvalue(0, 0)
    end

    # fast-forward マージしてコミット ID を返す
    def merge(from_branch)
      result = @conn.exec_params("SELECT aruaru_merge($1) AS commit_id", [from_branch])
      result.getvalue(0, 0)
    end

    # コミットログを取得する
    def log(limit: 20)
      result = @conn.exec_params(
        "SELECT * FROM aruaru_log ORDER BY timestamp DESC LIMIT $1", [limit]
      )
      result.map do |row|
        CommitEntry.new(
          id: row['id'], short_id: row['short_id'], author: row['author'],
          message: row['message'], timestamp: row['timestamp'],
          root_hash: row['root_hash']
        )
      end
    end

    # 2ブランチ間の差分統計を返す
    def diff(from, to)
      row = @conn.exec_params(
        "SELECT * FROM aruaru_diff($1, $2)", [from, to]
      ).first
      DiffStat.new(added: row['added'].to_i, removed: row['removed'].to_i,
                   modified: row['modified'].to_i)
    end

    # ── 汎用 SQL ─────────────────────────────────────────────

    # SQL を実行して変更行数を返す
    def execute(sql, *params)
      result = @conn.exec_params(sql, params)
      result.cmd_tuples
    end

    # SELECT を実行してハッシュの配列を返す
    def query(sql, *params)
      @conn.exec_params(sql, params).map(&:to_h)
    end

    # 生の PG::Connection を返す (高度な用途向け)
    def raw = @conn

    def close = @conn.close
  end
end
