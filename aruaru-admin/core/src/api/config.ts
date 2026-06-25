/**
 * aruaru-DB Admin — 接続設定
 *
 * 案Z: aruaru-server /graphql への直接接続（REST 廃止・GraphQL 一本化）
 *
 * 将来 Hive Gateway (MIT) を差し込む場合は VITE_ARUARU_GQL_ENDPOINT を
 * ゲートウェイ URL に変えるだけ。コードの変更は不要。
 *
 * 優先順位: localStorage > 環境変数 > 既定
 */

const LS_GQL_KEY = "aruaru.gqlEndpoint";

function env(name: string): string | undefined {
  try {
    const v = (import.meta as any)?.env?.[name];
    return typeof v === "string" && v.length > 0 ? v : undefined;
  } catch {
    return undefined;
  }
}

function lsGet(key: string): string | undefined {
  if (typeof localStorage === "undefined") return undefined;
  const v = localStorage.getItem(key);
  return v && v.length > 0 ? v : undefined;
}

/**
 * GraphQL エンドポイント URL。
 * 開発時: http://localhost:4000/graphql (aruaru-server 直接)
 * 本番時: https://api.example.com/graphql (Hive Gateway 等)
 * 将来の切替: VITE_ARUARU_GQL_ENDPOINT=https://hive.example.com/graphql pnpm build
 */
export function gqlEndpoint(): string {
  const v =
    lsGet(LS_GQL_KEY) ??
    env("VITE_ARUARU_GQL_ENDPOINT") ??
    "http://localhost:4000/graphql";
  return v.replace(/\/+$/, "");
}

/** 設定画面からエンドポイントを上書き保存 */
export function setGqlEndpoint(url: string): void {
  if (typeof localStorage !== "undefined") {
    localStorage.setItem(LS_GQL_KEY, url.replace(/\/+$/, ""));
  }
}

/** 上書き設定をリセット（env/既定値に戻る） */
export function clearGqlEndpoint(): void {
  if (typeof localStorage !== "undefined") {
    localStorage.removeItem(LS_GQL_KEY);
  }
}

/** 現在のエンドポイントが Hive Gateway 経由かどうかの判定（表示用） */
export function isViaGateway(): boolean {
  const ep = gqlEndpoint();
  return !ep.includes("localhost") && !ep.includes("127.0.0.1");
}
