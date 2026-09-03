# frozen_string_literal: true

require_relative "db/version"

# aruaru-db 公式 Ruby コネクタ(薄いラッパー) / official Ruby connector.
#
# これは独自の PostgreSQL ドライバではない。業界標準の `pg` gem
# (libpq のバインディング)をそのまま使い、その上に aruaru-db の
# Git-on-SQL 機能(`SELECT aruaru_commit('msg')` と
# `... AS OF COMMIT '<id>'`)を Ruby の慣用 API で足しただけの薄い層。
# Rails(ActiveRecord の `postgresql` アダプタ)からは、既存の
# `ActiveRecord::Base.connection.raw_connection`(`PG::Connection`)を
# {Aruaru::Db::Client.from_pg} へ渡して使う。
#
# This is NOT a custom PostgreSQL driver. It wraps the standard `pg` gem
# and only adds idiomatic helpers for aruaru-db's Git-on-SQL surface
# (`aruaru_commit` and `AS OF COMMIT`).
#
# 正本 / source of truth: ../../docs/CLIENTS.md
module Aruaru
  module Db
    # commit_id が `AS OF COMMIT '<id>'` のリテラルとして安全でない場合。
    class InvalidCommitId < ArgumentError
      def initialize(id)
        super("commit id #{id.inspect} is not a safe literal " \
              "(expected hex / [A-Za-z0-9_-], <=128 chars)")
      end
    end

    # `aruaru_commit()` が commit_id を返さなかった場合。
    class NoCommitId < StandardError
      def initialize
        super("aruaru_commit() returned no commit id")
      end
    end

    # aruaru-db の commit_id は英数字 + `-` `_`(ハッシュ/UUID 由来)。
    SAFE_COMMIT_ID = /\A[A-Za-z0-9_-]{1,128}\z/.freeze

    # commit_id が `AS OF COMMIT '<id>'` のリテラルとして安全か
    # (SQL インジェクション防止。aruaru-wire は `AS OF COMMIT` を
    # バインドパラメータとして受け付けないため、文字列連結の前に検証する)。
    def self.safe_commit_id?(id)
      id.is_a?(String) && !!SAFE_COMMIT_ID.match?(id)
    end

    # `PG::Connection` の薄いラッパー。
    class Client
      # @param dsn [String] libpq の接続文字列(例
      #   "host=localhost port=5433 user=app password=secret dbname=app")
      def self.connect(dsn)
        require "pg"
        new(PG.connect(dsn))
      end

      # 既存の `PG::Connection`(Rails の
      # `ActiveRecord::Base.connection.raw_connection` 等)を包む。
      def self.from_pg(pg_connection)
        new(pg_connection)
      end

      # @param conn [PG::Connection]
      def initialize(conn)
        @conn = conn
      end

      # 内部の `PG::Connection`。透過的に何でもできる。
      attr_reader :conn

      # DDL/DML をそのまま実行する薄い透過。
      def execute(sql, params = [])
        @conn.exec_params(sql, params)
      end

      # Git-on-SQL: 現在の全テーブル状態をスナップショットし commit_id を
      # 返す(`SELECT aruaru_commit($1)`)。
      #
      # 重要: aruaru-db はこの関数の結果列に `AS alias` を効かせない
      # ——結果列名は文字通り `aruaru_commit` になる。よって列名では
      # なく**位置(0番目)**で読む。
      def commit(message)
        result = @conn.exec_params("SELECT aruaru_commit($1)", [message])
        row = result.first
        commit_id = row && row.values.first
        raise NoCommitId if commit_id.nil? || commit_id.empty?

        commit_id
      end

      # VersionlessAPI: `base_select` の結果を過去のコミット時点で読む。
      # `base_select` は `AS OF COMMIT` を**含まない**通常の SELECT。
      # `commit_id` は {Aruaru::Db.safe_commit_id?} で検証し、非安全なら
      # {InvalidCommitId}(ネットワークに触れる前に拒否)。
      #
      # 結果列は現状すべて VARCHAR(text)として返る(`docs/CLIENTS.md`
      # §5.1)——数値として使う場合は呼び出し側で `to_i` 等すること。
      def query_as_of(base_select, commit_id, params = [])
        raise InvalidCommitId, commit_id unless Aruaru::Db.safe_commit_id?(commit_id)

        sql = "#{base_select.rstrip} AS OF COMMIT '#{commit_id}'"
        @conn.exec_params(sql, params)
      end
    end
  end
end
