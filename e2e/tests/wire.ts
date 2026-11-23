// 試験が API を叩く時の道と欄の名を、向けた server に合わせる（playwright.config.ts の CASTLIST_E2E_API）。
//   v1   今の server（castlist-server）: 道は /v1/…、欄は snake_case（既定）
//   api  作り直している新しい server（castlist-next。AI-36）: 道は /api/…、欄は camelCase
// 画面（/・/actors/…・/graph/…）の URL と字は、どちらの server でも同じ。

/** 新しい server に向けているか。 */
export const next = process.env.CASTLIST_E2E_API === 'api';

/** 今の server の道（/v1/…）を、向けた server の道にする。 */
export const path = (v1: string): string => (next ? v1.replace(/^\/v1\//, '/api/') : v1);

/** 本文の頭の欄の名（snake_case）を、向けた server の名にする（中の物の鍵は替えない: ラベルの鍵や detail は送った字のまま）。 */
export function body(data: Record<string, unknown>): Record<string, unknown> {
  if (!next) return data;
  return Object.fromEntries(Object.entries(data).map(([k, v]) => [k.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase()), v]));
}
