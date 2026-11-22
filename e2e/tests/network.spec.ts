// 相関図（/graph/network。図 C）を、本物の browser で見る。見本の記録は seed.setup.ts が置く。
// どの試験も、console の誤り・page の誤り・CSP の違反が 1 つも無いことを、終わりに見る。
import { expect, test, type Page } from '@playwright/test';

type Node = { id: string; name: string; kind: string; current_settings: { model: string; effort: string } | null };
type Edge = { id: string; type: string; from: string; to: string };
type Graph = { nodes: Node[]; edges: Edge[] };

/** 図の API の答え（browser の外から引く）。 */
async function graph(page: Page, query = ''): Promise<Graph> {
  const response = await page.request.get(`/v1/graph${query}`);
  expect(response.ok()).toBeTruthy();
  return response.json();
}

function byName(g: Graph, name: string): Node {
  const node = g.nodes.find((n) => n.name === name);
  expect(node, `${name} が無い`).toBeTruthy();
  return node!;
}

/**
 * 見張りを付ける。page を開く前に呼ぶ。返す関数は、それまでに出た誤りと CSP の違反を返す。
 * CSP の違反は、page の中の `securitypolicyviolation` を拾う（Playwright が CSP の外から差し込む script で聞く）。
 */
async function watch(page: Page): Promise<() => Promise<string[]>> {
  const problems: string[] = [];
  page.on('console', (m) => {
    if (m.type() === 'error') problems.push(`console: ${m.text()}`);
  });
  page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
  await page.addInitScript(() => {
    const seen: string[] = [];
    (window as unknown as { __csp: string[] }).__csp = seen;
    document.addEventListener('securitypolicyviolation', (e) => {
      seen.push(`csp: ${e.violatedDirective} ${e.blockedURI} ${e.sourceFile}:${e.lineNumber}`);
    });
  });
  return async () => {
    const csp = page.isClosed() ? [] : await page.evaluate(() => (window as unknown as { __csp: string[] }).__csp ?? []);
    return [...problems, ...csp];
  };
}

/** 相関図を開き、描き終わるまで待つ。 */
async function open(page: Page, path = '/graph/network'): Promise<void> {
  await page.goto(path);
  await expect(page.locator('#network')).toHaveAttribute('data-state', 'ready');
}

type Counts = { nodes: number; edges: number; visibleNodes: number; visibleEdges: number };

/** 描いた点と線の数（cytoscape の中の物。network.js が window.castlistNetwork に置く）。 */
function counts(page: Page): Promise<Counts> {
  return page.evaluate(() => {
    const cy = (window as unknown as { castlistNetwork: any }).castlistNetwork;
    return {
      nodes: cy.nodes().length,
      edges: cy.edges().length,
      visibleNodes: cy.nodes(':visible').length,
      visibleEdges: cy.edges(':visible').length,
    };
  });
}

let check: () => Promise<string[]>;

test.beforeEach(async ({ page }) => {
  check = await watch(page);
});

test.afterEach(async () => {
  expect(await check()).toEqual([]);
});

test('点はアクターの数、線は線の数だけ描き、継いだ線は点線', async ({ page }) => {
  const g = await graph(page);
  await open(page);
  expect(await counts(page)).toEqual({
    nodes: g.nodes.length,
    edges: g.edges.length,
    visibleNodes: g.nodes.length,
    visibleEdges: g.edges.length,
  });
  expect(g.nodes).toHaveLength(5);
  expect(g.edges).toHaveLength(4);
  const drawn = await page.evaluate(() => {
    const cy = (window as unknown as { castlistNetwork: any }).castlistNetwork;
    return {
      ids: cy.nodes().map((n: any) => n.id()).sort(),
      labels: cy.nodes().map((n: any) => n.style('label')).sort(),
      lines: cy.edges().map((e: any) => `${e.data('type')}:${e.style('line-style')}:${e.style('target-arrow-shape')}`).sort(),
      human: cy.nodes('[kind = "human"]').map((n: any) => n.style('shape')),
      ai: [...new Set(cy.nodes('[kind = "ai"]').map((n: any) => n.style('shape')))],
    };
  });
  expect(drawn.ids).toEqual(g.nodes.map((n) => n.id).sort());
  expect(drawn.labels).toEqual(g.nodes.map((n) => n.name).sort());
  expect(drawn.lines).toEqual([
    'continued:dashed:triangle',
    'launched:solid:triangle',
    'launched:solid:triangle',
    'launched:solid:triangle',
  ]);
  // 人は形を替える。
  expect(drawn.human).toHaveLength(1);
  expect(drawn.ai).toHaveLength(1);
  expect(drawn.human[0]).not.toEqual(drawn.ai[0]);
  await expect(page.locator('#network-status')).toContainText('5 のアクター');
});

test('点の色は model の系統で決まり、明るい色と暗い色で site.css の色に替わる', async ({ page }) => {
  const g = await graph(page);
  const ids = { opus: byName(g, 'tpm.1').id, sonnet: byName(g, 'se.1').id, haiku: byName(g, 'other.1').id };
  const colours = () =>
    page.evaluate((ids) => {
      const cy = (window as unknown as { castlistNetwork: any }).castlistNetwork;
      // site.css の色を、CSSOM で rgb の字に直す（inline の style の属性は使わない）。
      const css = (name: string) => {
        const probe = document.createElement('span');
        probe.style.color = `var(${name})`;
        document.body.append(probe);
        const rgb = getComputedStyle(probe).color;
        probe.remove();
        return rgb.replace(/\s/g, '');
      };
      const node = (id: string) => cy.getElementById(id).style('background-color').replace(/\s/g, '');
      return {
        got: [node(ids.opus), node(ids.sonnet), node(ids.haiku)],
        want: [css('--m-opus'), css('--m-sonnet'), css('--m-haiku')],
      };
    }, ids);
  await page.emulateMedia({ colorScheme: 'light' });
  await open(page);
  const light = await colours();
  expect(light.got).toEqual(light.want);
  await page.emulateMedia({ colorScheme: 'dark' });
  await expect.poll(async () => (await colours()).got).not.toEqual(light.got);
  const dark = await colours();
  expect(dark.got).toEqual(dark.want);
});

test('点を押すと、そのアクターのプロフィールへ行く', async ({ page }) => {
  const se2 = byName(await graph(page), 'se.2');
  await open(page);
  await page.locator('#network').scrollIntoViewIfNeeded();
  const point = await page.evaluate((id) => {
    const cy = (window as unknown as { castlistNetwork: any }).castlistNetwork;
    const p = cy.getElementById(id).renderedPosition();
    const box = cy.container().getBoundingClientRect();
    return { x: box.left + p.x, y: box.top + p.y };
  }, se2.id);
  await page.mouse.click(point.x, point.y);
  await expect(page).toHaveURL(`/actors/${se2.id}`);
  await expect(page.locator('h1')).toContainText('se.2');
});

test('点に重ねた時の説明は、人にだけ種類（人）を付け、AI とは書かない', async ({ page }) => {
  const g = await graph(page);
  const se1 = byName(g, 'se.1');
  const pm1 = byName(g, 'pm.1');
  await open(page);
  const title = (id: string) =>
    page.evaluate((nodeId) => {
      const cy = (window as unknown as { castlistNetwork: any }).castlistNetwork;
      cy.getElementById(nodeId).emit('mouseover');
      return cy.container().title as string;
    }, id);
  const ai = await title(se1.id);
  expect(ai.split('\n')[0]).toBe('se.1');
  expect(ai).not.toContain('AI');
  expect((await title(pm1.id)).split('\n')[0]).toBe('pm.1（人）');
});

test('model と effort の絞りは、合わない点とその線を隠し、外すと戻す', async ({ page }) => {
  await open(page);
  // 出てくる model（短い名）と effort が選べる。
  await expect(page.locator('#f-model option')).toHaveText(['すべて', 'haiku-5', 'opus-5-5', 'sonnet-5', '記録なし']);
  await expect(page.locator('#f-effort option')).toHaveText(['すべて', 'high', 'low', '記録なし']);

  await page.selectOption('#f-model', 'haiku-5');
  expect(await counts(page)).toMatchObject({ nodes: 5, edges: 4, visibleNodes: 1, visibleEdges: 0 });
  await expect(page.locator('#network-status')).toContainText('見えているのは 1');

  await page.selectOption('#f-model', '');
  await page.selectOption('#f-effort', 'high');
  // tpm.1・se.1・se.2 と、その間の線（tpm.1 → se.1・se.1 → se.2）。
  expect(await counts(page)).toMatchObject({ visibleNodes: 3, visibleEdges: 2 });

  await page.selectOption('#f-model', 'opus-5-5');
  expect(await counts(page)).toMatchObject({ visibleNodes: 2, visibleEdges: 0 });

  await page.selectOption('#f-model', '-');
  await page.selectOption('#f-effort', '-');
  // 記録なし: 人の pm.1 だけ。
  expect(await counts(page)).toMatchObject({ visibleNodes: 1, visibleEdges: 0 });

  await page.selectOption('#f-model', '');
  await page.selectOption('#f-effort', '');
  expect(await counts(page)).toMatchObject({ visibleNodes: 5, visibleEdges: 4 });
});

test('ラベルと窓の絞りは、図の API に渡し、その答えだけを描く', async ({ page }) => {
  const g = await graph(page, '?label=tpm.key:t-99');
  // other.1 と、線の向こうの端の tpm.1（一歩だけ）。
  expect(g.nodes.map((n) => n.name).sort()).toEqual(['other.1', 'tpm.1']);
  await open(page, '/graph/network?label=tpm.key:t-99&from=&to=');
  expect(await counts(page)).toEqual({ nodes: 2, edges: 1, visibleNodes: 2, visibleEdges: 1 });
  await expect(page.locator('input[name="label"]')).toHaveValue('tpm.key:t-99');
});

test('条件に合うアクターがいなければ、そう書く', async ({ page }) => {
  await open(page, '/graph/network?from=9000-01-01T00:00:00Z');
  expect(await counts(page)).toMatchObject({ nodes: 0, edges: 0 });
  await expect(page.locator('#network-status')).toContainText('条件に合うアクターはいません');
});

test('見張りは、CSP の違反を本当に当てる', async ({ page }) => {
  await open(page);
  // inline の style の属性は CSP が断る。その違反を拾えなければ、ほかの試験の「違反が無い」は当てにならない。
  await page.evaluate(() => {
    const div = document.createElement('div');
    div.innerHTML = '<span style="color: red">x</span>';
    document.body.append(div);
  });
  await expect.poll(async () => (await check()).filter((p) => p.startsWith('csp: style-src')).length).toBeGreaterThan(0);
  // この試験の違反は、わざと起こした物。終わりの見張りには数えない。
  check = async () => [];
});

test.describe('JavaScript を切った browser', () => {
  test.use({ javaScriptEnabled: false });

  test('タイムラインを使うよう書き、絞りの欄は出さない', async ({ page }) => {
    await page.goto('/graph/network?label=tpm.key:t-23zs');
    const note = page.locator('.noscript');
    await expect(note).toBeVisible();
    await expect(note).toContainText('JavaScript');
    await expect(note.locator('a[href="/graph/timeline?label=tpm.key:t-23zs"]')).toBeVisible();
    await expect(page.locator('#network-controls')).toBeHidden();
  });
});
