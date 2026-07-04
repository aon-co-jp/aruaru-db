// @aruaru/admin-core — win/web で共有する管理UIコア
export { default as App } from "./App";
export { invoke } from "./api/invoke";
export { serverBase, gqlEndpoint, setServerBase, clearServerBase } from "./api/config";
export { pickDirectory } from "./api/platform";
