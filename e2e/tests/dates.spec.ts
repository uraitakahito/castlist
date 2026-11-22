// 期間の欄（browser の日時の欄 `datetime-local`）と、期間をすぐ選ぶ link を、本物の browser で見る。
// 見本の記録は seed.setup.ts が置く（どれも今の記録で、Key t-23zs のアクターは tpm.1・se.1・se.2）。
// 画面の時刻は日本時間（playwright.config.ts の timezoneId も Asia/Tokyo）。
import { expect, test, type Page } from '@playwright/test';

/** 一覧に出たアクターの名（出た順）。 */
async function names(page: Page): Promise<string[]> {
  return page.locator('table.actors tbody a.name').allTextContents();
}

/** console の誤り・page の誤り（画面は script を使わないので、何も出ないはず）。 */
function watch(page: Page): string[] {
  const problems: string[] = [];
  page.on('console', (m) => {
    if (m.type() === 'error') problems.push(`console: ${m.text()}`);
  });
  page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
  return problems;
}

test('期間の欄は日時の欄で、入れた日本時間で絞り、送った後も同じ値を出す', async ({ page }) => {
  const problems = watch(page);
  await page.goto('/?label=tpm.key:t-23zs');
  const from = page.locator('input[name="active_from"]');
  const to = page.locator('input[name="active_to"]');
  await expect(from).toHaveAttribute('type', 'datetime-local');
  await expect(to).toHaveAttribute('type', 'datetime-local');
  expect(await names(page)).toHaveLength(3);

  // 昔の期間: 条件に合うアクターはいない。browser は日本時間の字を送り、欄にはその値が戻る。
  await to.fill('2000-01-01T09:00');
  await page.getByRole('button', { name: '絞り込む' }).click();
  await expect(page).toHaveURL(/active_to=2000-01-01T09%3A00/);
  await expect(page.getByText('条件に合うアクターはいません。')).toBeVisible();
  await expect(page.locator('input[name="active_to"]')).toHaveValue('2000-01-01T09:00');
  await expect(page.locator('input[name="label"]').first()).toHaveValue('tpm.key:t-23zs');
  expect(problems).toEqual([]);
});

test('RFC 3339 の期間の link は、日本時間の欄の値で出し、そのまま送り直せる', async ({ page }) => {
  await page.goto('/graph/timeline?from=2000-01-01T00:00:00Z&to=2030-01-01T00:00:07.120%2B09:00');
  const from = page.locator('input[name="from"]');
  const to = page.locator('input[name="to"]');
  await expect(from).toHaveValue('2000-01-01T09:00');
  // castlist は 3 桁で出す。browser は、終わりの 0 を落とした字（.12）を値にする（送るのもその字で、castlist は読める）。
  await expect(to).toHaveValue('2030-01-01T00:00:07.12');
  // 秒やミリ秒の付いた値でも、browser は送るのを断らない。
  expect(await to.evaluate((e: HTMLInputElement) => e.checkValidity())).toBe(true);
  await page.getByRole('button', { name: '絞り込む' }).click();
  await expect(page).toHaveURL(/to=2030-01-01T00%3A00%3A07\.12(&|$)/);
  await expect(page.locator('input[name="to"]')).toHaveValue('2030-01-01T00:00:07.12');
  await expect(page.locator('input[name="to"]')).toHaveAttribute('value', '2030-01-01T00:00:07.120');
  await expect(page.locator('svg.timeline')).toBeVisible();
});

test('期間をすぐ選ぶ link は、ほかの条件のまま期間だけを替えて絞る', async ({ page }) => {
  const problems = watch(page);
  await page.goto('/?label=tpm.key:t-23zs');
  expect(await names(page)).toHaveLength(3);

  // 見本は今の記録なので、昨日には稼働していない。
  await page.getByRole('link', { name: '昨日', exact: true }).click();
  await expect(page).toHaveURL(/label=tpm\.key:t-23zs/);
  await expect(page).toHaveURL(/active_from=\d{4}-\d\d-\d\dT00:00:00%2B09:00&active_to=\d{4}-\d\d-\d\dT00:00:00%2B09:00/);
  await expect(page.getByText('条件に合うアクターはいません。')).toBeVisible();
  // 欄には、その期間の日本時間の 0 時。
  await expect(page.locator('input[name="active_from"]')).toHaveValue(/T00:00$/);

  // 直近 1 時間なら、同じラベルの 3 つ。
  await page.getByRole('link', { name: '直近 1 時間', exact: true }).click();
  await expect(page).toHaveURL(/label=tpm\.key:t-23zs/);
  expect((await names(page)).sort()).toEqual(['se.1', 'se.2', 'tpm.1']);
  expect(problems).toEqual([]);
});
