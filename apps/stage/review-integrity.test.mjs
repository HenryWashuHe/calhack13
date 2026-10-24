// Independent review probes against exact PR20 b91b9a4 inline source. No network.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
const html = fs.readFileSync(new URL('./index.html', import.meta.url), 'utf8');
const source = html.slice(html.indexOf('<script>')+8, html.lastIndexOf('</script>'));
function harness(fetchImpl) {
  const nodes = new Map(), intervals = new Map(), timeouts = new Map(); let id=0;
  function el() {
    const n={textContent:'',innerHTML:'',value:'',children:[],handlers:{},hidden:false,disabled:false,classes:new Set(),style:{setProperty(k,v){this[k]=v;}},setAttribute(k,v){this[k]=v;},addEventListener(k,f){this.handlers[k]=f;},appendChild(c){this.children.push(c);},replaceChildren(...c){this.children=c;},closest(){return null;}};
    n.classList={add(c){n.classes.add(c);},remove(c){n.classes.delete(c);},toggle(c,b){if(b)n.classes.add(c);else n.classes.delete(c);}};
    return n;
  }
  const get=k=>{if(!nodes.has(k))nodes.set(k,el());return nodes.get(k);};
  vm.runInNewContext(source,{document:{getElementById:get,createElement:el,querySelectorAll:()=>get('dots').children,addEventListener(){}},fetch:fetchImpl,setTimeout:f=>{timeouts.set(++id,f);return id;},clearTimeout:k=>timeouts.delete(k),setInterval:f=>{intervals.set(++id,f);return id;},clearInterval:k=>intervals.delete(k),console});
  const flush=()=>{const callbacks=[...timeouts.values()];timeouts.clear();for(const f of callbacks)f();};
  flush();
  get('urlA').value='http://local-a/api/v1';get('urlB').value='http://local-b/api/v1';
  return {get,intervals,flush,submit:()=>get('liveform').handlers.submit({preventDefault(){}}),poll:()=>{for(const f of intervals.values())f();}};
}
const ok=body=>({ok:true,status:200,json:async()=>body});
const settle=async()=>{for(let n=0;n<8;n++)await new Promise(r=>setImmediate(r));};
const state=(org_id, revision=1, installations={})=>({org_id,name:org_id,revision,roles:{},missions:{},responsibilities:{},capability_gaps:{},installations});
const event=seq=>({seq,revision:seq,type:'role_activated',timestamp:new Date(seq*1000).toISOString(),payload:{}});
const installation=digest=>({capability:{capability_id:'cap_x',version:'1.0.0'},status:'ACCEPTED',source_digest:digest});
const hasAdoption=h=>h.get('lineage').children.some(n=>n.innerHTML.includes('Org B · adopted'));

test('late replay animation must not overwrite a successful live heading',async()=>{
  const h=harness(async url=>ok(url.includes('/events/')?[]:state(url.includes('local-a')?'org_a':'org_b')));
  h.get('next').handlers.click(); // delayed 180ms scene update remains pending
  h.submit();await settle();
  assert.equal(h.get('mode').textContent,'Live kernel');
  h.flush();
  assert.equal(h.get('beat').textContent,"Live: this is the kernel's own state and event log.");
});

test('overlapping polls must not regress revision or duplicate the same event',async()=>{
  let stateCalls=0,eventCalls=0;const release=[];
  const h=harness(url=>{
    if(url.includes('/organization'))return Promise.resolve(ok(state(url.includes('local-a')?'org_a':'org_b',Math.floor(stateCalls++/2)+1)));
    if(url.includes('/events/')){
      const old=eventCalls++<2;
      const body=url.includes('local-a')?(old?[event(1)]:[event(1),event(2)]):[];
      if(old)return new Promise(r=>release.push(()=>r(ok(body))));
      return Promise.resolve(ok(body));
    }
    throw Error('unexpected URL');
  });
  h.submit();await settle();h.poll();await settle();
  assert.equal(h.get('revA').textContent,'r2');
  assert.equal(h.get('events').children.length,2);
  for(const f of release)f();await settle();
  assert.deepEqual({revision:h.get('revA').textContent,eventCount:h.get('events').children.length,countLabel:h.get('evcount').textContent},{revision:'r2',eventCount:2,countLabel:'2 events'});
});

test('matching labels with different source digests must not imply A to B inheritance',async()=>{
  const a=state('org_a',1,{x:installation('a'.repeat(64))}),b=state('org_b',1,{x:installation('b'.repeat(64))});
  const h=harness(async url=>ok(url.includes('/capabilities/')?{publisher:'org_a',capability_id:'cap_x',version:'1.0.0',source_digest:'b'.repeat(64)}:url.includes('/events/')?[]:url.includes('local-a')?a:b));
  h.submit();await settle();
  assert.equal(hasAdoption(h),false,'B accepted different executable content under the same label');
});

test('connecting both panels to one organization must not imply independent inheritance',async()=>{
  const a=state('org_a',1,{x:installation('a'.repeat(64))});
  const h=harness(async url=>ok(url.includes('/capabilities/')?{publisher:'org_a',capability_id:'cap_x',version:'1.0.0',source_digest:'a'.repeat(64)}:url.includes('/events/')?[]:a));
  h.submit();await settle();
  assert.equal(hasAdoption(h),false,'both responses identify the same organization');
});

test('all responses completing after returning to replay must leave replay intact',async()=>{
  const releases=[];
  const h=harness(url=>url.includes('/organization')?new Promise(r=>releases.push(()=>r(ok(state(url.includes('local-a')?'org_a':'org_b'))))):Promise.resolve(ok([])));
  h.submit();await settle();h.get('backreplay').handlers.click();h.flush();
  for(const f of releases)f();await settle();
  assert.equal(releases.length,2);
  assert.equal(h.get('mode').textContent,'Historical fixture replay');
  assert.ok(h.get('auditline').textContent.startsWith('fixture run:'));
});

test('three pages must reach event 1500 while retaining only a 200-event tail',async()=>{
  const urls=[];
  const h=harness(async url=>{
    urls.push(url);
    if(url.includes('/events/')){
      if(url.includes('local-b'))return ok([]);
      const after=Number(new URL(url).searchParams.get('after'));
      return ok(Array.from({length:Math.min(500,1500-after)},(_,i)=>event(after+i+1)));
    }
    return ok(state(url.includes('local-a')?'org_a':'org_b',1500));
  });
  h.submit();await settle();h.poll();await settle();h.poll();await settle();
  assert.equal(h.get('events').children.length,200);
  assert.equal(h.get('evcount').textContent,'last 200 of 1500 events');
  assert.ok(h.get('events').children.at(-1).innerHTML.includes('A1500'));
  assert.ok(urls.some(u=>u.includes('after=1000')));
});
