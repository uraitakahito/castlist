// 画面の browser の試験の設定。同じ commit の castlist-server を作り、空いている port と一時の DB で起こして向ける。
//   CASTLIST_E2E_SERVER   起こす binary（既定は ../target/debug/castlist-server を cargo build してから）。
//                         作り直している新しい server（AI-36）なら ../target/debug/castlist-next（cargo build --locked --bin castlist-next）
//   CASTLIST_E2E_API      試験が API を叩く形（tests/wire.ts）。v1（今の server）か api（新しい server）。既定は、起こす binary の名が
//                         castlist-next なら api、ほかは v1。名を替えた binary や包みの script を向ける時は、これを付ける
//                         （付けないと v1 になり、見本を置く段が /v1/… に送って 404 で落ちる）
//   CASTLIST_E2E_OUT      落ちた時の写しと report を置く folder の名の後ろに付ける字（2 つの server を続けて流す CI で分けるため）
//   CASTLIST_E2E_PORT     待ち受ける port（既定は、空いている port を 1 つ選ぶ）
//   CASTLIST_E2E_CHANNEL  この機械に入っている browser を使う時の名（chrome など）。無ければ Playwright の Chromium
// 見本の記録は、tests/seed.setup.ts が API で置く（ほかの試験より先に 1 度だけ）。
import { defineConfig } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join } from 'node:path';

// 設定は、試験を流す worker の中でも読み直される。port と DB は初めの 1 度だけ決め、環境変数で worker に渡す。
process.env.CASTLIST_E2E_PORT ??= execFileSync(process.execPath, [
  '-e',
  'const s = require("node:net").createServer().listen(0, "127.0.0.1", () => { console.log(s.address().port); s.close(); });',
])
  .toString()
  .trim();
process.env.CASTLIST_E2E_DB ??= join(mkdtempSync(join(tmpdir(), 'castlist-e2e-')), 'castlist.db');
const port = Number(process.env.CASTLIST_E2E_PORT);
const db = process.env.CASTLIST_E2E_DB;
const server = process.env.CASTLIST_E2E_SERVER;
process.env.CASTLIST_E2E_API ??= server && basename(server) === 'castlist-next' ? 'api' : 'v1';
const build = server ? `exec "${server}"` : 'cargo build --locked --bin castlist-server && exec ../target/debug/castlist-server';

export default defineConfig({
  testDir: 'tests',
  // 見本の記録を数える試験があるので、1 本ずつ順に流す。
  fullyParallel: false,
  workers: 1,
  // やり直さない（時々落ちる試験を隠さない）。
  retries: 0,
  forbidOnly: !!process.env.CI,
  outputDir: `test-results${process.env.CASTLIST_E2E_OUT ?? ''}`,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never', outputFolder: `playwright-report${process.env.CASTLIST_E2E_OUT ?? ''}` }]] : 'list',
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    channel: process.env.CASTLIST_E2E_CHANNEL,
    timezoneId: 'Asia/Tokyo',
    locale: 'ja-JP',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects: [
    { name: 'seed', testMatch: /seed\.setup\.ts/ },
    { name: 'chromium', testMatch: /\.spec\.ts/, dependencies: ['seed'] },
  ],
  webServer: {
    // 前の回の DB を消してから起こす（試験は、いつも空の台帳から始まる）。
    command: `rm -f "${db}" "${db}-wal" "${db}-shm" && ${build}`,
    env: { CASTLIST_LISTEN: `127.0.0.1:${port}`, CASTLIST_DB: db },
    url: `http://127.0.0.1:${port}/healthz`,
    // cargo build の分も待つ。
    timeout: 600_000,
    reuseExistingServer: false,
  },
});
