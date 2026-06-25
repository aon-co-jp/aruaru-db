// aruaru-DB Java ドライバー
// パッケージ名: dev.aruaru:aruaru-db-java
// Maven:
//   <dependency>
//     <groupId>dev.aruaru</groupId>
//     <artifactId>aruaru-db-java</artifactId>
//     <version>0.5.0</version>
//   </dependency>
// 内部依存: org.postgresql:postgresql:42.7+
//
// aruaru-DB は PostgreSQL ワイヤ互換のため、標準 JDBC がそのまま使えます。
// このドライバーは Git-on-SQL 操作を型付き API で包むラッパーです。

import java.sql.*;
import java.util.*;

public class AruaruDBDriver {
    private final Connection conn;

    /**
     * aruaru-DB へ接続する。
     *
     * @param host   ホスト名 (例: "localhost")
     * @param port   ポート番号 (既定: 5432)
     * @param db     データベース名 (既定: "aruaru")
     * @param user   ユーザー名
     */
    public AruaruDBDriver(String host, int port, String db, String user) throws SQLException {
        String url = String.format("jdbc:postgresql://%s:%d/%s", host, port, db);
        Properties props = new Properties();
        props.setProperty("user", user);
        this.conn = DriverManager.getConnection(url, props);
    }

    /** URL 文字列から接続 (例: "jdbc:postgresql://localhost:5432/aruaru?user=root") */
    public AruaruDBDriver(String jdbcUrl) throws SQLException {
        this.conn = DriverManager.getConnection(jdbcUrl);
    }

    // ── Git-on-SQL ─────────────────────────────────────────────

    /** ブランチを作成する */
    public void branch(String name) throws SQLException {
        try (PreparedStatement ps = conn.prepareStatement("SELECT aruaru_branch(?)")) {
            ps.setString(1, name);
            ps.execute();
        }
    }

    /** ブランチを切り替える */
    public void checkout(String name) throws SQLException {
        try (PreparedStatement ps = conn.prepareStatement("SELECT aruaru_checkout(?)")) {
            ps.setString(1, name);
            ps.execute();
        }
    }

    /** コミットしてコミット ID を返す */
    public String commit(String message) throws SQLException {
        try (PreparedStatement ps = conn.prepareStatement(
                "SELECT aruaru_commit(?) AS commit_id")) {
            ps.setString(1, message);
            try (ResultSet rs = ps.executeQuery()) {
                return rs.next() ? rs.getString("commit_id") : null;
            }
        }
    }

    /** fast-forward マージしてコミット ID を返す */
    public String merge(String fromBranch) throws SQLException {
        try (PreparedStatement ps = conn.prepareStatement(
                "SELECT aruaru_merge(?) AS commit_id")) {
            ps.setString(1, fromBranch);
            try (ResultSet rs = ps.executeQuery()) {
                return rs.next() ? rs.getString("commit_id") : null;
            }
        }
    }

    /** コミットログを取得する */
    public List<Map<String, Object>> log(int limit) throws SQLException {
        List<Map<String, Object>> result = new ArrayList<>();
        try (PreparedStatement ps = conn.prepareStatement(
                "SELECT * FROM aruaru_log ORDER BY timestamp DESC LIMIT ?")) {
            ps.setInt(1, limit);
            try (ResultSet rs = ps.executeQuery()) {
                ResultSetMetaData meta = rs.getMetaData();
                while (rs.next()) {
                    Map<String, Object> row = new LinkedHashMap<>();
                    for (int i = 1; i <= meta.getColumnCount(); i++)
                        row.put(meta.getColumnName(i), rs.getObject(i));
                    result.add(row);
                }
            }
        }
        return result;
    }

    /** 任意の SQL を実行して変更行数を返す */
    public int execute(String sql, Object... params) throws SQLException {
        try (PreparedStatement ps = conn.prepareStatement(sql)) {
            for (int i = 0; i < params.length; i++)
                ps.setObject(i + 1, params[i]);
            return ps.executeUpdate();
        }
    }

    /** 任意の SELECT を実行して行リストを返す */
    public List<Map<String, Object>> query(String sql, Object... params) throws SQLException {
        List<Map<String, Object>> result = new ArrayList<>();
        try (PreparedStatement ps = conn.prepareStatement(sql)) {
            for (int i = 0; i < params.length; i++)
                ps.setObject(i + 1, params[i]);
            try (ResultSet rs = ps.executeQuery()) {
                ResultSetMetaData meta = rs.getMetaData();
                while (rs.next()) {
                    Map<String, Object> row = new LinkedHashMap<>();
                    for (int i = 1; i <= meta.getColumnCount(); i++)
                        row.put(meta.getColumnName(i), rs.getObject(i));
                    result.add(row);
                }
            }
        }
        return result;
    }

    /** 生の JDBC Connection を返す (高度な用途向け) */
    public Connection raw() { return conn; }

    public void close() throws SQLException { conn.close(); }

    // ── 使用例 ─────────────────────────────────────────────────
    public static void main(String[] args) throws Exception {
        // aruaru-DB に接続
        AruaruDBDriver db = new AruaruDBDriver("localhost", 5432, "aruaru", "root");

        // ブランチ作成
        db.branch("feature/java-test");

        // テーブル操作
        db.execute("CREATE TABLE IF NOT EXISTS orders (id INT, item TEXT)");
        db.execute("INSERT INTO orders (id, item) VALUES (?, ?)", 1, "Book");

        // コミット
        String commitId = db.commit("Add orders table via aruaru-db-java");
        System.out.println("Committed: " + commitId);

        // ログ
        db.log(5).forEach(row -> System.out.println(row));

        db.close();
    }
}
