/* Bench page hook. Shared by every scenario page; served at /_shared/bench.js.
 *
 * It does three things and nothing else:
 * 1. Seed: reads ?seed=N (or the seed carried from the previous page) and gives
 *    pages a deterministic PRNG, so a seed reorders twins and rows without
 *    changing what a task means. Same-origin links keep the seed.
 * 2. Journal: every click, text entry, selection, toggle, and submit the page
 *    sees is posted to the bench server as a page action. The harness counts
 *    steps and detects "stuck" from this journal, from outside every arm.
 * 3. Ground truth: pages call BENCH.emit(type, data) for semantic outcomes
 *    (sent, archived, booked, ...) and BENCH.set(patch) for current state.
 *    Checkers read only these, never an agent's own report.
 */
(function () {
  const params = new URLSearchParams(location.search);
  let seed = params.get('seed');
  try {
    if (seed === null) seed = sessionStorage.getItem('bench-seed');
    if (seed !== null) sessionStorage.setItem('bench-seed', seed);
  } catch (_) {}
  seed = Number(seed || 1) >>> 0;

  function mulberry32(a) {
    return function () {
      a |= 0; a = (a + 0x6d2b79f5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }
  const rand = mulberry32(seed || 1);
  const page = location.pathname;
  let state = {};
  let lastAction = '';

  function post(path, body) {
    try {
      fetch(path, { method: 'POST', keepalive: true, headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body) }).catch(() => {});
    } catch (_) {}
  }

  function labelOf(el) {
    if (!el || el.nodeType !== 1) return '';
    const aria = el.getAttribute('aria-label');
    if (aria) return aria.trim().slice(0, 60);
    const labelled = el.getAttribute('aria-labelledby');
    if (labelled) {
      const ref = (el.getRootNode() || document).getElementById
        ? (el.getRootNode().getElementById ? el.getRootNode().getElementById(labelled) : document.getElementById(labelled))
        : null;
      if (ref) return ref.textContent.trim().slice(0, 60);
    }
    if (el.id && el.labels && el.labels.length) return el.labels[0].textContent.trim().slice(0, 60);
    if (el.placeholder) return el.placeholder.slice(0, 60);
    return (el.textContent || el.value || '').replace(/\s+/g, ' ').trim().slice(0, 60);
  }

  const ACTIONABLE = 'button,a[href],input,select,textarea,summary,label,[role=button],[role=link],[role=menuitem],[role=menuitemradio],[role=option],[role=tab],[role=checkbox],[role=switch],[role=radio],[role=gridcell],[data-bench-target]';
  function actionable(path) {
    for (const node of path) {
      if (node && node.nodeType === 1 && node.matches && node.matches(ACTIONABLE)) return node;
    }
    return path[0] && path[0].nodeType === 1 ? path[0] : null;
  }
  function describe(el) {
    if (!el) return 'none';
    const tag = el.tagName.toLowerCase();
    const id = el.getAttribute('data-bench-id') || el.id || '';
    return `${tag}${id ? '#' + id : ''}|${labelOf(el)}`;
  }
  function act(kind, el, extra) {
    const target = describe(el);
    const sig = `${kind}:${target}`;
    if (kind === 'fill' && sig === lastAction) return; // one entry per field until something else happens
    lastAction = sig;
    post('/_bench/act', { kind, target, page, url: location.href, t: Date.now(), ...(extra || {}) });
  }

  document.addEventListener('click', (e) => {
    const el = actionable(e.composedPath());
    if (!el) return;
    const tag = el.tagName.toLowerCase();
    if ((tag === 'input' || tag === 'textarea') && !['checkbox', 'radio', 'button', 'submit'].includes(el.type)) return;
    if (tag === 'select') return;
    if (tag === 'label' && el.control) return; // the control's own click follows
    act('click', el, { trusted: e.isTrusted });
  }, true);
  document.addEventListener('input', (e) => {
    const el = e.composedPath()[0];
    if (!el || !el.tagName) return;
    const tag = el.tagName.toLowerCase();
    if (tag === 'select' || el.type === 'checkbox' || el.type === 'radio') return;
    act('fill', el);
  }, true);
  document.addEventListener('change', (e) => {
    const el = e.composedPath()[0];
    if (el && el.tagName && el.tagName.toLowerCase() === 'select') act('select', el, { value: el.value });
  }, true);
  document.addEventListener('submit', (e) => act('submit', e.target), true);
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') act('key', e.composedPath()[0], { key: 'Enter' });
  }, true);

  function keepSeed(a) {
    try {
      const url = new URL(a.getAttribute('href'), location.href);
      if (url.origin !== location.origin || url.searchParams.has('seed')) return;
      url.searchParams.set('seed', String(seed));
      a.setAttribute('href', url.pathname + url.search + url.hash);
    } catch (_) {}
  }
  document.addEventListener('click', (e) => {
    const a = e.composedPath().find((n) => n && n.tagName === 'A' && n.getAttribute && n.getAttribute('href'));
    if (a && !a.getAttribute('href').startsWith('#')) keepSeed(a);
  }, true);

  function report() {
    post('/_bench/state', { page, url: location.href, title: document.title, state });
  }

  window.BENCH = {
    seed,
    rand,
    pick(list) { return list[Math.floor(rand() * list.length)]; },
    shuffle(list) {
      const out = list.slice();
      for (let i = out.length - 1; i > 0; i--) {
        const j = Math.floor(rand() * (i + 1));
        [out[i], out[j]] = [out[j], out[i]];
      }
      return out;
    },
    emit(type, data) {
      post('/_bench/event', { type, data: data || {}, page, url: location.href, t: Date.now() });
    },
    set(patch) { state = { ...state, ...patch }; report(); },
    get state() { return { ...state }; },
  };
  window.addEventListener('hashchange', report);
  window.addEventListener('load', report);
  // Titles change on hash routes; report after the page's own handlers ran.
  window.addEventListener('hashchange', () => setTimeout(report, 50));
})();
