// 関係の網（/graph/network。相関図 C）を描く。castlist が /static/network.js で配る（CSP の script-src 'self'）。
// - 点と線は、#network の data-src（同じ絞りの GET /v1/graph。connect-src 'self'）から引く。
// - 描くのは cytoscape（static/vendor/。canvas に描くので、inline の style は要らない）。
// - 色は site.css の色（--m-opus など）を読む。明るい色と暗い色が替われば、描き直す。
// - 字は DOM の textContent でだけ置く（innerHTML は使わない）。
(() => {
  'use strict';

  const container = document.getElementById('network');
  const status = document.getElementById('network-status');
  const controls = document.getElementById('network-controls');
  const modelSelect = document.getElementById('f-model');
  const effortSelect = document.getElementById('f-effort');
  if (!container || !status || typeof cytoscape !== 'function') return;

  // cytoscape は初めに <style> を head に差し込もうとする（CSP の style-src が断り、違反になる）。
  // 同じ id の印が既にあれば差し込まない。差し込むはずだった決まり（容れ物を position: relative に）は site.css に書いてある。
  const MARK = '__________cytoscape_stylesheet';
  if (!document.getElementById(MARK)) {
    const mark = document.createElement('meta');
    mark.id = MARK;
    document.head.append(mark);
  }

  /** model と effort の絞りの「記録なし」の値。 */
  const NONE = '-';

  /** model の名から、色の class（時間の帯と同じ決め方。記録が無ければ m-none）。 */
  function family(model) {
    if (!model) return 'm-none';
    const found = ['opus', 'sonnet', 'haiku'].find((f) => model.includes(f));
    return found ? `m-${found}` : 'm-other';
  }

  const shortModel = (model) => model.replace(/^claude-/, '');

  /** site.css の色。 */
  function palette() {
    const css = getComputedStyle(document.documentElement);
    const get = (name) => css.getPropertyValue(name).trim();
    const p = { fg: get('--fg'), muted: get('--muted'), card: get('--card'), on: get('--on') };
    for (const f of ['m-opus', 'm-sonnet', 'm-haiku', 'm-other', 'm-none']) p[f] = get(`--${f}`);
    return p;
  }

  function stylesheet(p) {
    const families = ['m-opus', 'm-sonnet', 'm-haiku', 'm-other', 'm-none'].map((f) => ({
      selector: `node.${f}`,
      style: { 'background-color': p[f] },
    }));
    return [
      {
        selector: 'node',
        style: {
          label: 'data(name)',
          width: 26,
          height: 26,
          'border-width': 2,
          'border-color': p.card,
          color: p.fg,
          'font-size': 12,
          'font-weight': 700,
          'font-family': 'system-ui, "Hiragino Sans", "Noto Sans JP", sans-serif',
          'text-valign': 'bottom',
          'text-margin-y': 5,
          // 名の下地を絵の地の色で塗り、名を横切る線を隠す（点は線の上に描く）。
          'text-background-color': p.card,
          'text-background-opacity': 0.9,
          'text-background-shape': 'round-rectangle',
          'text-background-padding': '2px',
        },
      },
      ...families,
      { selector: 'node[kind = "human"]', style: { shape: 'round-rectangle', width: 34, height: 28, 'border-width': 3, 'border-color': p.on } },
      // 今いない者は、塗りを薄く（名は薄くしない）。
      { selector: 'node.absent', style: { 'background-opacity': 0.4 } },
      {
        selector: 'edge',
        style: {
          width: 1.5,
          'curve-style': 'bezier',
          'line-color': p.muted,
          'target-arrow-color': p.muted,
          'target-arrow-shape': 'triangle',
        },
      },
      { selector: 'edge[type = "continued"]', style: { 'line-style': 'dashed', 'line-dash-pattern': [6, 4] } },
      { selector: '.hidden', style: { display: 'none' } },
    ];
  }

  /** API の答えを、cytoscape の点と線に。点の初めの置き場は円に並べ（同じ答えからは同じ図）、cose で整える。 */
  function elements(graph) {
    const n = graph.nodes.length;
    const radius = Math.max(80, n * 24);
    const nodes = graph.nodes.map((node, i) => {
      const s = node.current_settings;
      const angle = (2 * Math.PI * i) / Math.max(n, 1);
      return {
        group: 'nodes',
        data: {
          id: node.id,
          name: node.name,
          kind: node.kind,
          model: s ? shortModel(s.model) : NONE,
          effort: s ? s.effort : NONE,
          present: Boolean(node.state && node.state.present),
        },
        classes: [family(s && s.model), node.state && node.state.present ? '' : 'absent'].join(' ').trim(),
        position: { x: radius * Math.cos(angle), y: radius * Math.sin(angle) },
      };
    });
    const known = new Set(graph.nodes.map((node) => node.id));
    const edges = graph.edges
      .filter((e) => known.has(e.from) && known.has(e.to))
      .map((e) => ({ group: 'edges', data: { id: `edge-${e.id}`, source: e.from, target: e.to, type: e.type } }));
    return [...nodes, ...edges];
  }

  /** 字に重ねた時の説明（名・種類・model · effort・居るか）。 */
  function describe(node) {
    const d = node.data();
    const kind = d.kind === 'human' ? '人' : 'AI';
    const setting = d.model === NONE ? 'model の記録なし' : `${d.model} · ${d.effort}`;
    return `${d.name}（${kind}）\n${setting}\n${d.present ? 'いる' : 'いない'}`;
  }

  /** 日本の時刻の字（2026-11-23 09:00:00）。 */
  function local(iso) {
    const t = new Date(iso);
    if (Number.isNaN(t.getTime())) return iso;
    return new Intl.DateTimeFormat('sv-SE', { timeZone: 'Asia/Tokyo', dateStyle: 'short', timeStyle: 'medium' }).format(t);
  }

  function fillSelect(select, values) {
    for (const value of values) {
      const option = document.createElement('option');
      option.value = value;
      option.textContent = value === NONE ? '記録なし' : value;
      select.append(option);
    }
  }

  /** 選べる値（名の順。記録なしは終わりに）。 */
  function choices(cy, key) {
    const values = new Set(cy.nodes().map((n) => n.data(key)));
    const named = [...values].filter((v) => v !== NONE).sort();
    return values.has(NONE) ? [...named, NONE] : named;
  }

  function say(text) {
    status.textContent = text;
  }

  function render(graph) {
    // 絞りの欄は、cytoscape を起こす前に出す。後で出すと、絵の場所が下にずれ、cytoscape が覚えた場所と押した所が食い違う。
    controls.hidden = graph.nodes.length === 0;
    const cy = cytoscape({
      container,
      elements: elements(graph),
      style: stylesheet(palette()),
      layout: { name: 'preset' },
      boxSelectionEnabled: false,
      minZoom: 0.3,
      maxZoom: 3,
    });
    // 試験と調べ物のため（browser の console から見られる）。
    window.castlistNetwork = cy;

    const total = `${cy.nodes().length} のアクター・${cy.edges().length} の線（${local(graph.as_of)} に答えた）`;
    // 見えている数は、絞りの class で数える（:visible は、次に描くまで前の様子を返す）。
    const update = () => {
      const shown = cy.nodes().not('.hidden');
      if (shown.length === cy.nodes().length) {
        say(total);
      } else {
        say(`${total}。見えているのは ${shown.length} のアクター・${shown.edgesWith(shown).length} の線`);
      }
    };

    if (cy.nodes().length === 0) {
      container.hidden = true;
      say('当たるアクターはいない。絞りを緩めるか、外す。');
    } else {
      cy.layout({
        name: 'cose',
        animate: false,
        randomize: false,
        fit: true,
        padding: 40,
        nodeDimensionsIncludeLabels: true,
        idealEdgeLength: () => 90,
        nodeRepulsion: () => 12000,
        componentSpacing: 80,
      }).run();
      update();

      fillSelect(modelSelect, choices(cy, 'model'));
      fillSelect(effortSelect, choices(cy, 'effort'));
      const apply = () => {
        const model = modelSelect.value;
        const effort = effortSelect.value;
        cy.batch(() => {
          cy.nodes().forEach((n) => {
            const ok = (model === '' || n.data('model') === model) && (effort === '' || n.data('effort') === effort);
            n.toggleClass('hidden', !ok);
          });
        });
        // class の替わりを、すぐ様子に移す（cytoscape は次に描く時まで移さず、:visible などが前の様子を返す）。
        cy.elements().cleanStyle();
        update();
      };
      modelSelect.addEventListener('change', apply);
      effortSelect.addEventListener('change', apply);
    }

    // cytoscape は、絵の場所（画面の中の位置）を覚えておき、押した所の点を探す。覚え直すのは window の scroll と
    // 大きさの替わりの知らせが来た時だけなので、ほかの訳で絵が動いた後（上の字の折り返し・scroll の知らせの前に押した時など）は、
    // 押した所と違う点を探す。指が入った時と押す前に覚え直させる（cytoscape より先に聞くよう capture で）。
    for (const type of ['pointerenter', 'pointerdown']) {
      container.addEventListener(type, () => cy.resize(), { capture: true });
    }
    cy.on('tap', 'node', (e) => {
      window.location.assign(`/actors/${encodeURIComponent(e.target.id())}`);
    });
    cy.on('mouseover', 'node', (e) => {
      container.title = describe(e.target);
      container.classList.add('pointing');
    });
    cy.on('mouseout', 'node', () => {
      container.title = '';
      container.classList.remove('pointing');
    });
    // 明るい色と暗い色が替われば、site.css の色を読み直す。
    window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
      cy.style().fromJson(stylesheet(palette())).update();
    });
    container.dataset.state = 'ready';
  }

  fetch(container.dataset.src, { headers: { accept: 'application/json' } })
    .then(async (response) => {
      const body = await response.json().catch(() => ({}));
      if (!response.ok) throw new Error(`${response.status} ${body.error ?? ''}`.trim());
      return body;
    })
    .then(render)
    .catch((e) => {
      container.hidden = true;
      container.dataset.state = 'error';
      say(`相関図を引けない: ${e.message}`);
    });
})();
