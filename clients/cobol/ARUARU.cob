      ******************************************************************
      * aruaru-db  COBOL 参照実装(埋め込み SQL / EXEC SQL)
      *
      * 独自ドライバは無い。aruaru-db は pgwire(PostgreSQL 互換)なので、
      * COBOL からは標準の 3 経路のいずれかでそのまま繋がる:
      *   (1) ODBC  : psqlODBC(psqlodbc)を DSN 登録。Windows の ODBC
      *               データソース / Linux・UNIX・z/OS USS の unixODBC。
      *               EXEC SQL は Micro Focus / OpenCOBOL ESQL(ODBC)で
      *               プリコンパイル。
      *   (2) libpq : GnuCOBOL から  CALL "PQconnectdb" / "PQexec" 等。
      *   (3) OCESQL: open-cobol-esql(ocesql)で EXEC SQL → libpq。
      *
      * このプログラムは (1)/(3) 共通の埋め込み SQL 形。ポートは 5433。
      * Git-on-SQL の aruaru_commit / AS OF COMMIT は「ただの SQL」なので
      * ホスト変数へバインドするだけ。
      *
      * 正本: ../../docs/CLIENTS.md
      ******************************************************************
       IDENTIFICATION DIVISION.
       PROGRAM-ID. ARUARU.

       ENVIRONMENT DIVISION.

       DATA DIVISION.
       WORKING-STORAGE SECTION.

           EXEC SQL BEGIN DECLARE SECTION END-EXEC.
       01  DB-DSN        PIC X(128)
           VALUE "host=localhost port=5433 dbname=app user=app".
       01  DB-PASS       PIC X(64)  VALUE "secret".
       01  H-ID          PIC X(32)  VALUE "sword".
       01  H-QTY         PIC S9(9) COMP-5.
       01  H-MSG         PIC X(64).
       01  H-COMMIT-ID   PIC X(128).
           EXEC SQL END DECLARE SECTION END-EXEC.

           EXEC SQL INCLUDE SQLCA END-EXEC.

       PROCEDURE DIVISION.
       MAIN.
      *    --- 接続 ---
           EXEC SQL
               CONNECT :DB-DSN IDENTIFIED BY :DB-PASS
           END-EXEC.
           PERFORM CHECK-SQL.

      *    --- 現在値を書く ---
           MOVE 1 TO H-QTY.
           EXEC SQL
               INSERT INTO items (id, qty) VALUES (:H-ID, :H-QTY)
               ON CONFLICT (id) DO UPDATE SET qty = EXCLUDED.qty
           END-EXEC.
           PERFORM CHECK-SQL.

      *    --- Git-on-SQL コミット(commit_id をホスト変数へ) ---
           MOVE "first import" TO H-MSG.
           EXEC SQL
               SELECT aruaru_commit(:H-MSG) INTO :H-COMMIT-ID
           END-EXEC.
           PERFORM CHECK-SQL.
           DISPLAY "commit_id = " H-COMMIT-ID.

      *    --- 更新して再コミット ---
           MOVE 5 TO H-QTY.
           EXEC SQL
               UPDATE items SET qty = :H-QTY WHERE id = :H-ID
           END-EXEC.
           PERFORM CHECK-SQL.
           MOVE "restock" TO H-MSG.
           EXEC SQL
               SELECT aruaru_commit(:H-MSG) INTO :H-COMMIT-ID
           END-EXEC.
           PERFORM CHECK-SQL.

      *    --- VersionlessAPI: 最初のコミット時点を読む(qty = 1) ---
      *    NOTE: AS OF COMMIT のリテラルはホスト変数から。commit_id は
      *          aruaru-db が発行した英数字 + - _ のみなので安全。
           EXEC SQL
               SELECT qty INTO :H-QTY
               FROM items WHERE id = :H-ID
               AS OF COMMIT :H-COMMIT-ID
           END-EXEC.
           PERFORM CHECK-SQL.
           DISPLAY "as-of qty = " H-QTY.

           EXEC SQL COMMIT WORK END-EXEC.
           EXEC SQL DISCONNECT ALL END-EXEC.
           STOP RUN.

       CHECK-SQL.
           IF SQLCODE NOT = 0
               DISPLAY "SQL error: " SQLCODE " " SQLERRMC
               EXEC SQL ROLLBACK WORK END-EXEC
               STOP RUN
           END-IF.
