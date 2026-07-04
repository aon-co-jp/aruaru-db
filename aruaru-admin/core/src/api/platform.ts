// プラットフォーム抽象: ディレクトリ選択
//
// win 版 (Tauri) では @tauri-apps/plugin-dialog を動的 import して使う。
// web 版 (ブラウザ) ではディレクトリ選択 API が無いため prompt で代替する。

export async function pickDirectory(title: string): Promise<string | null> {
  // Tauri 環境判定 (グローバルに __TAURI__ が注入される)
  const isTauri =
    typeof window !== "undefined" && (window as any).__TAURI__ !== undefined;

  if (isTauri) {
    try {
      // 動的 import: web ビルドではバンドルされない
      const mod = await import(/* @vite-ignore */ "@tauri-apps/plugin-dialog");
      const dir = await mod.open({ directory: true, title });
      return (dir as string) ?? null;
    } catch {
      // フォールバックへ
    }
  }
  // web フォールバック
  const input = typeof prompt !== "undefined" ? prompt(`${title} (パスを入力)`) : null;
  return input && input.length > 0 ? input : null;
}
