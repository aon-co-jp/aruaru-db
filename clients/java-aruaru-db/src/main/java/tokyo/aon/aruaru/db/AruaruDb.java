package tokyo.aon.aruaru.db;

import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.SQLException;
import java.sql.Statement;
import java.util.regex.Pattern;

/**
 * aruaru-db 公式 Java コネクタ(薄いラッパー) / official Java connector.
 *
 * <p>これは独自の PostgreSQL ドライバではない。業界標準の
 * {@code org.postgresql:postgresql}(素の JDBC、純 Java Type-4 ——
 * Windows/macOS/Linux/UNIX/IBM z/OS/Android で同一 jar が動く)をそのまま
 * 使い、その上に aruaru-db の Git-on-SQL 機能を Java の慣用 API で
 * 足しただけの薄い層。Spring Boot / Quarkus / 素の JDBC、どれでも同じ。
 *
 * <p>This is NOT a custom PostgreSQL driver. It wraps the standard JDBC
 * driver ({@code org.postgresql:postgresql}) and only adds idiomatic
 * helpers for aruaru-db's Git-on-SQL surface ({@code aruaru_commit} and
 * {@code AS OF COMMIT}).
 *
 * <p>正本 / source of truth: {@code ../../docs/CLIENTS.md}
 */
public final class AruaruDb implements AutoCloseable {

    /** aruaru-db commit ids are alnum + '-'/'_' (hash/UUID-ish), &lt;=128 chars. */
    private static final Pattern SAFE_COMMIT_ID = Pattern.compile("^[A-Za-z0-9_-]{1,128}$");

    private final Connection connection;

    private AruaruDb(Connection connection) {
        this.connection = connection;
    }

    /**
     * jdbc:postgresql://host:5433/db?ssl=true の形の URL で接続する。
     */
    public static AruaruDb connect(String url, String user, String password) throws SQLException {
        return new AruaruDb(DriverManager.getConnection(url, user, password));
    }

    /** 既存の {@link Connection}(コネクションプール由来等)を包む。 */
    public static AruaruDb fromConnection(Connection connection) {
        return new AruaruDb(connection);
    }

    /** 内部の {@link Connection}。透過的に何でもできる。 */
    public Connection connection() {
        return connection;
    }

    /**
     * commit_id が {@code AS OF COMMIT '<id>'} のリテラルとして安全か
     * (SQL インジェクション防止。aruaru-wire は {@code AS OF COMMIT} を
     * バインドパラメータとして受け付けないため、文字列連結の前に検証する)。
     */
    public static boolean isSafeCommitId(String id) {
        return id != null && SAFE_COMMIT_ID.matcher(id).matches();
    }

    /** {@code sql} を実行するだけの薄い透過。 */
    public void execute(String sql) throws SQLException {
        try (Statement st = connection.createStatement()) {
            st.execute(sql);
        }
    }

    /**
     * Git-on-SQL: 現在の全テーブル状態をスナップショットし commit_id を
     * 返す({@code SELECT aruaru_commit(?)})。
     *
     * <p><b>重要</b>: aruaru-db はこの関数の結果列に {@code AS alias} を
     * 効かせない——結果列名は文字通り {@code aruaru_commit} になる。
     * よって列名ではなく <b>位置(1列目)</b> で読む。
     */
    public String commit(String message) throws SQLException {
        try (PreparedStatement ps = connection.prepareStatement("SELECT aruaru_commit(?)")) {
            ps.setString(1, message);
            try (ResultSet rs = ps.executeQuery()) {
                if (!rs.next()) {
                    throw new SQLException("aruaru_commit() returned no commit id");
                }
                String commitId = rs.getString(1); // by position, not by name
                if (commitId == null) {
                    throw new SQLException("aruaru_commit() returned no commit id");
                }
                return commitId;
            }
        }
    }

    /**
     * VersionlessAPI: {@code baseSelect} の結果を過去のコミット時点で読む。
     * {@code baseSelect} は {@code AS OF COMMIT} を含まない通常の SELECT
     * (例 {@code "SELECT qty FROM items WHERE id = ?"})。commitId は
     * {@link #isSafeCommitId} で検証し、非安全なら
     * {@link IllegalArgumentException} を投げる(ネットワークに触れる前)。
     *
     * <p>結果列は現状すべて VARCHAR(text)として返る
     * ({@code docs/CLIENTS.md} §5.1)——{@code getInt} 等の型付き
     * getter ではなく {@code getString} で受けて parse すること。
     */
    public ResultSet queryAsOf(String baseSelect, String commitId, Object... params) throws SQLException {
        if (!isSafeCommitId(commitId)) {
            throw new IllegalArgumentException(
                "commit id " + commitId + " is not a safe literal (expected hex / [A-Za-z0-9_-], <=128 chars)");
        }
        String sql = baseSelect.stripTrailing() + " AS OF COMMIT '" + commitId + "'";
        PreparedStatement ps = connection.prepareStatement(sql);
        for (int i = 0; i < params.length; i++) {
            ps.setObject(i + 1, params[i]);
        }
        return ps.executeQuery();
    }

    @Override
    public void close() throws SQLException {
        connection.close();
    }
}
