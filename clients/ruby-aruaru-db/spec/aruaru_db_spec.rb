# frozen_string_literal: true

require "spec_helper"
require "aruaru/db"

RSpec.describe Aruaru::Db do
  describe ".safe_commit_id?" do
    it "accepts hashes and uuids" do
      expect(described_class.safe_commit_id?("a1b2c3d4e5f6")).to be true
      expect(described_class.safe_commit_id?("9f8e7d6c-1234-4abc-9def-000011112222")).to be true
      expect(described_class.safe_commit_id?("commit_42-X")).to be true
    end

    it "rejects unsafe or empty ids" do
      expect(described_class.safe_commit_id?("")).to be false
      expect(described_class.safe_commit_id?(nil)).to be false
      expect(described_class.safe_commit_id?("abc'; DROP TABLE items; --")).to be false
      expect(described_class.safe_commit_id?("abc def")).to be false
      expect(described_class.safe_commit_id?("x" * 200)).to be false
    end
  end

  describe Aruaru::Db::Client do
    it "rejects an unsafe commit id in #query_as_of before touching the network" do
      # No PG::Connection is ever constructed -- passing nil proves the
      # rejection happens before any network I/O would be attempted.
      client = described_class.from_pg(nil)
      expect do
        client.query_as_of("SELECT qty FROM items", "' OR 1=1 --")
      end.to raise_error(Aruaru::Db::InvalidCommitId, /not a safe literal/)
    end

    it "raises NoCommitId when #commit gets no row (fake connection)" do
      fake_result = []
      fake_conn = double("PG::Connection")
      allow(fake_conn).to receive(:exec_params).and_return(fake_result)
      client = described_class.from_pg(fake_conn)

      expect { client.commit("msg") }.to raise_error(Aruaru::Db::NoCommitId)
    end

    it "reads the commit id by position, not by result column name" do
      fake_row = { "aruaru_commit" => "abc123" }
      fake_result = [fake_row]
      fake_conn = double("PG::Connection")
      allow(fake_conn).to receive(:exec_params)
        .with("SELECT aruaru_commit($1)", ["msg"])
        .and_return(fake_result)
      client = described_class.from_pg(fake_conn)

      expect(client.commit("msg")).to eq("abc123")
    end
  end

  # 実サーバ相手の往復。環境変数 ARUARU_DB_TEST_DSN があるときだけ走る。
  # 例: ARUARU_DB_TEST_DSN="host=127.0.0.1 port=5433 user=app password=secret dbname=app"
  describe "live round trip", if: ENV["ARUARU_DB_TEST_DSN"] do
    it "commits and reads back the historical value via AS OF COMMIT" do
      client = Aruaru::Db::Client.connect(ENV.fetch("ARUARU_DB_TEST_DSN"))
      client.execute("CREATE TABLE IF NOT EXISTS items (id TEXT PRIMARY KEY, qty INT)")
      client.execute(
        "INSERT INTO items(id, qty) VALUES ('sword', 1) " \
        "ON CONFLICT (id) DO UPDATE SET qty = EXCLUDED.qty"
      )
      first = client.commit("first import")
      client.execute("UPDATE items SET qty = 5 WHERE id = 'sword'")
      client.commit("restock")

      latest = client.execute("SELECT qty FROM items WHERE id = 'sword'").first["qty"]
      expect(latest.to_i).to eq(5)

      old = client.query_as_of("SELECT qty FROM items WHERE id = 'sword'", first).first["qty"]
      expect(old.to_i).to eq(1)
    end
  end
end
