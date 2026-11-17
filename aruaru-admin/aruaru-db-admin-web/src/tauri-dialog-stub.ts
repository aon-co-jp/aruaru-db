// Web 版では Tauri のダイアログは使えない。core の `pickDirectory` が
// `@tauri-apps/plugin-dialog` を動的 import するため、ここで空実装へ解決して
// vite の依存解析エラー(白画面)を避ける。null を返すと prompt へフォールバックする。
export async function open(_opts?: unknown): Promise<string | null> {
  return null;
}
