# aruaru-DB Ruby Client
# gem install aruaru-db
# 内部依存: gem 'pg', '~> 1.5'

require 'pg'

module AruaruDB
  class Client
    def initialize(host: 'localhost', port: 5432, dbname: 'aruaru', user: 'root')
      @conn = PG.connect(host:, port:, dbname:, user:)
    end

    def branch(name)
      @conn.exec_params("SELECT aruaru_branch($1)", [name])
    end

    def commit(message)
      result = @conn.exec_params("SELECT aruaru_commit($1) AS commit_id", [message])
      result[0]['commit_id']
    end

    def log(limit: 20)
      @conn.exec_params(
        "SELECT * FROM aruaru_log ORDER BY timestamp DESC LIMIT $1", [limit]
      ).to_a
    end

    def exec(sql, *params)
      @conn.exec_params(sql, params)
    end

    def close
      @conn.close
    end
  end
end

# ── 使用例 ──────────────────────────────────────────────────
# db = AruaruDB::Client.new
# db.branch('feature/ruby-test')
# db.exec("CREATE TABLE IF NOT EXISTS posts (id SERIAL PRIMARY KEY, title TEXT)")
# commit_id = db.commit('Add posts table')
# puts "Committed: #{commit_id}"
# db.log(limit: 5).each { |row| p row }
# db.close
