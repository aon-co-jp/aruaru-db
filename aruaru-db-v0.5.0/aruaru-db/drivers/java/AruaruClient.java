// aruaru-DB Java Client
// Maven: dev.aruaru:client:0.2.0
// 標準 PostgreSQL JDBC 42+ がそのまま使えます
// <dependency>
//   <groupId>org.postgresql</groupId>
//   <artifactId>postgresql</artifactId>
//   <version>42.7.3</version>
// </dependency>

import java.sql.*;
import java.util.*;

public class AruaruClient {
    private final Connection conn;

    public AruaruClient(String host, int port, String db, String user) throws SQLException {
        String url = String.format("jdbc:postgresql://%s:%d/%s", host, port, db);
        Properties props = new Properties();
        props.setProperty("user", user);
        this.conn = DriverManager.getConnection(url, props);
    }

    // Git-on-SQL メソッド
    public void branch(String name) throws SQLException {
        try (PreparedStatement ps = conn.prepareStatement("SELECT aruaru_branch(?)")) {
            ps.setString(1, name);
            ps.execute();
        }
    }

    public String commit(String author, String message) throws SQLException {
        try (PreparedStatement ps = conn.prepareStatement(
                "SELECT aruaru_commit(?) as commit_id")) {
            ps.setString(1, message);
            try (ResultSet rs = ps.executeQuery()) {
                return rs.next() ? rs.getString("commit_id") : null;
            }
        }
    }

    public List<Map<String, Object>> log(int limit) throws SQLException {
        List<Map<String, Object>> result = new ArrayList<>();
        try (PreparedStatement ps = conn.prepareStatement(
                "SELECT * FROM aruaru_log ORDER BY timestamp DESC LIMIT ?")) {
            ps.setInt(1, limit);
            try (ResultSet rs = ps.executeQuery()) {
                ResultSetMetaData meta = rs.getMetaData();
                while (rs.next()) {
                    Map<String, Object> row = new LinkedHashMap<>();
                    for (int i = 1; i <= meta.getColumnCount(); i++) {
                        row.put(meta.getColumnName(i), rs.getObject(i));
                    }
                    result.add(row);
                }
            }
        }
        return result;
    }

    public void close() throws SQLException { conn.close(); }

    // ── 使用例 ──────────────────────────────────────────────
    public static void main(String[] args) throws Exception {
        AruaruClient db = new AruaruClient("localhost", 5432, "aruaru", "root");

        db.branch("feature/java-test");
        
        try (Statement st = db.conn.createStatement()) {
            st.execute("CREATE TABLE IF NOT EXISTS orders (id SERIAL PRIMARY KEY, item TEXT)");
            st.execute("INSERT INTO orders (item) VALUES ('Book')");
        }

        String commitId = db.commit("JavaDev", "Add orders table");
        System.out.println("Committed: " + commitId);

        db.log(5).forEach(row -> System.out.println(row));
        db.close();
    }
}
