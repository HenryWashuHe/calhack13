// Inert UI only: fake SVG nodes, fake clock/timers. No browser or networking.
import fs from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';
const html = fs.readFileSync(new URL('./network.html', import.meta.url), 'utf8');
const script = html.slice(html.indexOf('<script>') + 8, html.lastIndexOf('</script>'));
function harness() {
  class Node {
    constructor(tag = 'div') { this.tag = tag; this.attrs = {}; this.children = []; this.style = {}; this.dataset = {}; this.textContent = ''; this.className = ''; this.classList = { toggle: (s, yes) => { const c = new Set(this.className.split(' ').filter(Boolean)); if (yes ?? !c.has(s)) c.add(s); else c.delete(s); this.className = [...c].join(' '); } }; }
    setAttribute(k, v) { this.attrs[k] = String(v); }
    getAttribute(k) { return this.attrs[k] ?? null; }
    appendChild(n) { n.parent = this; this.children.push(n); return n; }
    replaceChildren(...ns) { this.children = []; ns.forEach(n => this.appendChild(n)); }
    remove() { if (this.parent) this.parent.children = this.parent.children.filter(n => n !== this); }
    addEventListener() {}
  }
  const ids = new Map();
  const get = id => { if (!ids.has(id)) ids.set(id, new Node()); return ids.get(id); };
  const buttons = [0, 1, 2].map(i => { const n = new Node('button'); n.dataset.act = String(i); return n; });
  const listeners = {}, timers = []; let wall = 0;
  const document = { getElementById: get, createElementNS: (_, tag) => new Node(tag), querySelectorAll: () => buttons, addEventListener: (name, cb) => listeners[name] = cb, body: new Node(), documentElement: new Node() };
  const context = vm.createContext({ document, matchMedia: () => ({matches: false}), requestAnimationFrame: () => {}, setTimeout: (cb, delay) => timers.push({at: wall + delay, cb}), fetch: () => { throw new Error('network forbidden'); } });
  vm.runInContext(script, context);
  const run = code => vm.runInContext(code, context);
  const flush = now => { wall = now; for (;;) { const i = timers.findIndex(t => t.at <= wall); if (i < 0) break; timers.splice(i, 1)[0].cb(); } };
  const tick = now => { wall = now; run(`frame(${now})`); flush(now); };
  const nodes = root => [root, ...root.children.flatMap(nodes)];
  const css = (id, cls) => nodes(get(id)).filter(n => (n.attrs.class ?? '').split(' ').includes(cls));
  const snapshot = id => nodes(get(id)).map(n => ({tag:n.tag, attrs:{...n.attrs},style:{...n.style},text:n.textContent}));
  const pause = () => listeners.keydown({key:' ',preventDefault(){}});
  return {run,tick,flush,css,snapshot,pause,get, snapshotNode: root => nodes(root).map(n => ({tag:n.tag,attrs:{...n.attrs},style:{...n.style},text:n.textContent}))};
}

test('control: the kernel ladder advances while running', () => {
  const h=harness(); h.tick(0); h.tick(1800); h.tick(2600);
  assert.equal(h.css('svg-kernel','rung').findIndex(n=>(n.attrs.class??'').includes('lit')),4);
});

test('pausing freezes already scheduled kernel ladder transitions', () => {
  const h=harness(); h.tick(0); h.tick(1800); h.pause();
  const before=h.snapshot('svg-kernel'); h.tick(2600);
  assert.equal(h.run('clock.t'),1800);
  assert.deepEqual(h.snapshot('svg-kernel'),before);
});

test('pausing freezes receiver verification completion', () => {
  const h=harness(); h.run('show(2)'); h.tick(0); h.tick(1900); h.tick(3200); h.pause();
  const before=h.snapshot('svg-network'); h.tick(4600);
  assert.equal(h.run('clock.t'),3200);
  assert.deepEqual(h.snapshot('svg-network'),before);
});

function endpoint(d) {
  const t=d.match(/[MHV]|-?[\d.]+/g); let x=0,y=0;
  for(let i=0;i<t.length;) { const c=t[i++]; if(c==='M'){x=+t[i++];y=+t[i++];} else if(c==='H') x=+t[i++]; else if(c==='V') y=+t[i++]; else throw Error(c); }
  return {x,y};
}
test('each generation displays its own survivor score, including no promotion', () => {
  for (const [seed, survivors] of [[1,[18,17,16,16,19]], [2,[15,15,18,15,19]]]) {
    const h=harness(); h.run(`seedIndex=${seed}; show(1)`); let now=0;
    for (const [i, score] of survivors.entries()) {
      for(;now<=3700+i*3300;now+=100) h.tick(now);
      const last=endpoint(h.css('svg-improve','frontier')[0].attrs.d);
      assert.equal(last.y,560-(score/20)*(560-110),`seed ${seed} generation ${i+1}`);
    }
  }
});

test('verification resumes only after the remaining animation-clock delay', () => {
  const h=harness(); h.run('show(2)'); h.tick(0); h.tick(1900); h.tick(3200); h.pause(); h.tick(4600);
  assert.equal(h.css('svg-network','verify').filter(n => n.attrs.class.includes('on')).length,1);
  h.pause(); h.tick(5800); // Only 1200 of the pending 1300 animation milliseconds passed.
  assert.equal(h.css('svg-network','verify').filter(n => n.attrs.class.includes('on')).length,1);
  h.tick(5900);
  assert.equal(h.css('svg-network','verify').filter(n => n.attrs.class.includes('on')).length,0);
  assert.equal(h.css('svg-network','org').filter(n => n.attrs.class.includes('lit')).length,2);
});

test('restarting an act cancels callbacks that captured its old SVG', () => {
  const h=harness(); h.run('show(2)'); h.tick(0); h.tick(1900); h.tick(3200);
  const old=h.get('svg-network').children[0], before=h.snapshotNode(old);
  h.run('show(2)'); h.tick(4500);
  assert.deepEqual(h.snapshotNode(old),before);
});

test('fresh-panel controls stay in their separate confirmation bars', () => {
  const h=harness(); h.run('show(1)'); h.tick(0);
  assert.equal(h.css('svg-improve','control').length,0);
  for(let t=100;t<=18200;t+=100) h.tick(t);
  assert.equal(h.css('svg-improve','bar').length,4);
  assert.equal(h.css('svg-improve','ctrl').length,3);
});
