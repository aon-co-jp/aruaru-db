package tokyo.aon.aruaru.db;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.ResultSet;
import java.sql.Statement;

import org.junit.jupiter.api.Test;

class AruaruDbTest {

    @Test
    void isSafeCommitIdAcceptsHashesAndUuidsRejectsSql() {
        assertTrue(AruaruDb.isSafeCommitId("a1b2c3d4e5f6"));
        assertTrue(AruaruDb.isSafeCommitId("9f8e7d6c-1234-4abc-9def-000011112222"));
        assertTrue(AruaruDb.isSafeCommitId("commit_42-X"));

        assertFalse(AruaruDb.isSafeCommitId(""));
        assertFalse(AruaruDb.isSafeCommitId(null));
        assertFalse(AruaruDb.isSafeCommitId("abc'; DROP TABLE items; --"));
        assertFalse(AruaruDb.isSafeCommitId("abc def"));
        assertFalse(AruaruDb.isSafeCommitId("x".repeat(200)));
    }

    @Test
    void queryAsOfRejectsUnsafeCommitIdBeforeTouchingTheNetwork() throws Exception {
        // A connection is never opened for this test at all -- if
        // queryAsOf() reached the network it would NPE on the null
        // Connection, proving rejection happens before any I/O.
        AruaruDb db = AruaruDb.fromConnection(null);
        IllegalArgumentException ex = org.junit.jupiter.api.Assertions.assertThrows(
            IllegalArgumentException.class,
            () -> db.queryAsOf("SELECT qty FROM items", "' OR 1=1 --"));
        assertTrue(ex.getMessage().contains("not a safe literal"));
    }

    /**
     * 実サーバ相手の往復。環境変数 ARUARU_DB_TEST_URL があるときだけ走る。
     * 例: jdbc:postgresql://127.0.0.1:5433/app
     */
    @Test
    void liveCommitAndAsOfRoundTrip() throws Exception {
        String url = System.getenv("ARUARU_DB_TEST_URL");
        assumeTrue(url != null && !url.isEmpty(), "set ARUARU_DB_TEST_URL to run against a live aruaru-server");

        Connection conn = DriverManager.getConnection(url, "app", "secret");
        try (AruaruDb db = AruaruDb.fromConnection(conn)) {
            db.execute("CREATE TABLE IF NOT EXISTS items (id TEXT PRIMARY KEY, qty INT)");
            db.execute("INSERT INTO items(id, qty) VALUES ('sword', 1) "
                + "ON CONFLICT (id) DO UPDATE SET qty = EXCLUDED.qty");

            String first = db.commit("first import");
            db.execute("UPDATE items SET qty = 5 WHERE id = 'sword'");
            db.commit("restock");

            try (Statement st = conn.createStatement();
                 ResultSet rs = st.executeQuery("SELECT qty FROM items WHERE id = 'sword'")) {
                assertTrue(rs.next());
                assertEquals(5, Integer.parseInt(rs.getString(1)));
            }

            try (ResultSet rs = db.queryAsOf("SELECT qty FROM items WHERE id = 'sword'", first)) {
                assertTrue(rs.next());
                assertEquals(1, Integer.parseInt(rs.getString(1)), "AS OF COMMIT must return the historical value");
            }
        }
    }
}
