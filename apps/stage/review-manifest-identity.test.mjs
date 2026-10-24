// Independent review probes against PR20 9a6234a inline source. No network.
// Preserve the original review-integrity.test.mjs unchanged; these cover the remaining manifest binding and opt-in replay path.
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


test('actual replay callback cannot overwrite live after opt-in',async()=>{
  const h=harness(async url=>ok(url.includes('/events/')?[]:state(url.includes('local-a')?'org_a':'org_b')));
  h.get('playreplay').handlers.click();
  h.get('next').handlers.click();
  h.submit();await settle();h.flush();
  assert.equal(h.get('beat').textContent,"Live: this is the kernel's own state and event log.");
});

test('manifest lacking source identity must not certify exact inheritance',async()=>{
  const digest='a'.repeat(64);
  const a=state('org_a',1,{x:installation(digest)}),b=state('org_b',1,{x:installation(digest)});
  const h=harness(async url=>ok(url.includes('/capabilities/')?{publisher:'org_a',capability_id:'cap_x',version:'1.0.0'}:url.includes('/events/')?[]:url.includes('local-a')?a:b));
  h.submit();await settle();
  assert.equal(hasAdoption(h),false,'publisher alone does not bind the manifest to the accepted source');
});

test('manifest with a different capability identity must not certify requested inheritance',async()=>{
  const digest='a'.repeat(64);
  const a=state('org_a',1,{x:installation(digest)}),b=state('org_b',1,{x:installation(digest)});
  const h=harness(async url=>ok(url.includes('/capabilities/')?{publisher:'org_a',capability_id:'different_capability',version:'9.0.0',source_digest:digest}:url.includes('/events/')?[]:url.includes('local-a')?a:b));
  h.submit();await settle();
  assert.equal(hasAdoption(h),false,'matching publisher and source do not bind a different capability label');
});
