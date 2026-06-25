/**
 * aruaru-DB C/C++ Client
 * libaruaru: libpq ラッパー (PostgreSQL クライアントライブラリ)
 * 
 * コンパイル: cc -o app app.c -laruaru -lpq
 *            c++ -o app app.cpp -laruaru -lpq -std=c++17
 */
#ifndef ARUARU_H
#define ARUARU_H

#ifdef __cplusplus
extern "C" {
#endif

#include <stddef.h>

typedef struct AruaruConn AruaruConn;
typedef struct AruaruResult AruaruResult;

/** 接続を開く */
AruaruConn* aruaru_connect(const char* host, int port,
                            const char* dbname, const char* user);

/** 接続を閉じる */
void aruaru_close(AruaruConn* conn);

/** SQL 実行 */
AruaruResult* aruaru_exec(AruaruConn* conn, const char* sql);

/** Git-on-SQL: ブランチ作成 */
int aruaru_branch(AruaruConn* conn, const char* name);

/** Git-on-SQL: コミット (戻り値は commit_id 文字列, 呼び出し元が free() する) */
char* aruaru_commit(AruaruConn* conn, const char* message);

/** Git-on-SQL: ログ (JSON 文字列, 呼び出し元が free() する) */
char* aruaru_log(AruaruConn* conn, int limit);

/** 結果の行数 */
int aruaru_nrows(AruaruResult* res);

/** 結果の列数 */
int aruaru_ncols(AruaruResult* res);

/** セル値取得 */
const char* aruaru_value(AruaruResult* res, int row, int col);

/** 結果を解放 */
void aruaru_free_result(AruaruResult* res);

/** エラーメッセージ */
const char* aruaru_error(AruaruConn* conn);

#ifdef __cplusplus
}

// ── C++17 RAII ラッパー ────────────────────────────────────
#include <string>
#include <vector>
#include <memory>

namespace aruaru {
    class Connection {
        AruaruConn* conn_;
    public:
        Connection(const std::string& host, int port = 5432,
                   const std::string& db = "aruaru",
                   const std::string& user = "root")
            : conn_(aruaru_connect(host.c_str(), port, db.c_str(), user.c_str())) {}

        ~Connection() { if (conn_) aruaru_close(conn_); }

        void branch(const std::string& name) { aruaru_branch(conn_, name.c_str()); }

        std::string commit(const std::string& msg) {
            char* id = aruaru_commit(conn_, msg.c_str());
            std::string result(id ? id : "");
            free(id);
            return result;
        }

        Connection(const Connection&) = delete;
        Connection& operator=(const Connection&) = delete;
    };
}
#endif /* __cplusplus */

#endif /* ARUARU_H */
