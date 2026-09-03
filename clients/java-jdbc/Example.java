// aruaru-db + plain JDBC (sync) — minimal connector example.
//
// 正本: ../../docs/CLIENTS.md
// aruaru-db 側に独自ドライバは不要。標準の PostgreSQL JDBC ドライバで
// そのまま繋がる(純 Java Type-4 = Windows/macOS/Linux/UNIX/z/OS/Android で同一)。
// 非同期が要るなら R2DBC(io.r2dbc:r2dbc-postgresql、末尾コメント参照)。
// 同期でも Java 21+ の仮想スレッドと組み合わせれば高並行(ハイブリッド)。
//
// build.gradle:  implementation 'org.postgresql:postgresql:42.7.4'
// Run:  java -cp postgresql-42.7.4.jar Example.java

import java.sql.*;

public class Example {
    public static void main(String[] args) throws Exception {
        String url = System.getenv().getOrDefault(
            "ARUARU_DB_URL", "jdbc:postgresql://localhost:5433/app?ssl=true");
        try (Connection c = DriverManager.getConnection(url, "app", "secret")) {
            try (Statement st = c.createStatement()) {
                st.execute("CREATE TABLE IF NOT EXISTS items (id TEXT PRIMARY KEY, qty INT)");
                st.execute("INSERT INTO items(id, qty) VALUES ('sword', 1) "
                         + "ON CONFLICT (id) DO UPDATE SET qty = EXCLUDED.qty");
            }
            // Git-on-SQL commit
            String firstCommit;
            try (ResultSet rs = c.createStatement()
                    .executeQuery("SELECT aruaru_commit('first import')")) {
                rs.next();
                firstCommit = rs.getString(1);
            }
            try (Statement st = c.createStatement()) {
                st.execute("UPDATE items SET qty = 5 WHERE id = 'sword'");
                st.execute("SELECT aruaru_commit('restock')");
            }
            // VersionlessAPI: read as of the first commit (expect qty = 1)
            try (PreparedStatement ps = c.prepareStatement(
                    "SELECT qty FROM items WHERE id = ? AS OF COMMIT ?")) {
                ps.setString(1, "sword");
                ps.setString(2, firstCommit);
                try (ResultSet rs = ps.executeQuery()) {
                    rs.next();
                    System.out.println("latest should be 5, as-of-first = " + rs.getInt(1));
                }
            }
        }
    }
}

// --- 非同期 (R2DBC) ---
// ConnectionFactory f = ConnectionFactories.get(
//     "r2dbc:postgresql://app:secret@localhost:5433/app");
// Mono.from(f.create()).flatMapMany(conn ->
//     conn.createStatement("SELECT aruaru_commit($1)").bind("$1", "msg").execute());
//
// Spring Boot: spring.datasource.url=jdbc:postgresql://localhost:5433/app  (同期)
//              spring.r2dbc.url=r2dbc:postgresql://app:secret@localhost:5433/app  (WebFlux)
// Quarkus:     quarkus.datasource.jdbc.url=jdbc:postgresql://localhost:5433/app
