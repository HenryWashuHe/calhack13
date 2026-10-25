import fs from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
import test from 'node:test';
const html=fs.readFileSync(new URL('./network.html', import.meta.url),'utf8');
const script=html.slice(html.indexOf('<script>')+8,html.lastIndexOf('</script>'));
function harness(fetcher=async()=>({ok:true,json:async()=>[]}), search='') {
 class Node{constructor(tag='div'){this.tag=tag;this.attrs={};this.children=[];this.style={};this.dataset={};this.textContent='';this.className='';this.handlers={};this.classList={toggle:(s,y)=>{const c=new Set(this.className.split(' ').filter(Boolean));if(y??!c.has(s))c.add(s);else c.delete(s);this.className=[...c].join(' ')}}}setAttribute(k,v){this.attrs[k]=String(v)}getAttribute(k){return this.attrs[k]??null}appendChild(n){n.parent=this;this.children.push(n);return n}replaceChildren(...ns){this.children=[];ns.forEach(n=>this.appendChild(n))}remove(){if(this.parent)this.parent.children=this.parent.children.filter(n=>n!==this)}addEventListener(k,fn){this.handlers[k]=fn}focus(){}}
 const ids=new Map(),get=id=>{if(!ids.has(id))ids.set(id,new Node());return ids.get(id)},buttons=[0,4,1,3,2].map(i=>{const n=new Node();n.dataset.act=String(i);return n}),listeners={},intervals=new Map();let tid=0;
 const document={getElementById:get,createElementNS:(_,t)=>new Node(t),querySelectorAll:()=>buttons,addEventListener:(k,fn)=>listeners[k]=fn,body:new Node(),documentElement:new Node()};
 const ctx=vm.createContext({document,matchMedia:()=>({matches:false}),requestAnimationFrame:()=>{},setInterval:fn=>{intervals.set(++tid,fn);return tid},clearInterval:id=>intervals.delete(id),fetch:fetcher,prompt:()=> 'mock-agent',location:{search,pathname:'/network.html',hash:''},history:{replaceState(_a,_b,url){ctx.location.search=url.includes('?')?'?'+url.split('?')[1]:''}},setTimeout:fn=>{intervals.set(++tid,fn);return tid},clearTimeout:id=>intervals.delete(id)});
 let wall=0; const run=s=>vm.runInContext(s,ctx);run(script);run('frame(0)');const advance=ms=>{for(let end=wall+ms;wall<end;){wall=Math.min(end,wall+100);run(`frame(${wall})`)}};const nodes=r=>[r,...r.children.flatMap(nodes)];const snap=id=>nodes(get(id)).map(n=>({tag:n.tag,attrs:{...n.attrs},text:n.textContent,style:{...n.style}}));
 return{run,get,intervals,snap,nodes,advance,location:ctx.location,keys:k=>listeners.keydown({key:k,preventDefault(){}})};
}
const org={org_id:'A',name:'A',revision:10,roles:{r:{name:'lead'}},missions:{m:{}}};
const resp=x=>({ok:true,status:200,json:async()=>x});
// Pure fake-DOM/fake-fetch regressions: no real networking, providers, methods or OS probes.
const flush = h => h.advance(2000);
const ev = (seq,type,payload={})=>({seq,type,payload,revision:10+seq});

test('control: successful mocked connection initializes the organization',async()=>{const h=harness(async u=>u.includes('/organization')?resp(org):resp([]));await h.run('startLive()');flush(h);assert.match(h.get('chip').textContent,/live kernel/);assert.equal(h.run('live.started'),true);assert.equal(h.run('live.state.org_id'),'A')});

test('stop during initial poll keeps scripted source and no timer',async()=>{let resolve;const pending=new Promise(r=>resolve=r);let calls=0;const h=harness(async()=>++calls===1?pending:resp([]));const startup=h.run('startLive()');h.run('stopLive()');resolve(resp(org));await startup;assert.equal(h.intervals.size,0);assert.match(h.get('chip').textContent,/scripted/)});

test('paused polling does not mutate the live drawing',async()=>{let revision=10;const h=harness(async u=>u.includes('/organization')?resp({...org,revision}):resp([]));await h.run('startLive()');flush(h);h.keys(' ');const before=h.snap('svg-kernel');revision=11;await h.run('pollLive()');flush(h);assert.deepEqual(h.snap('svg-kernel'),before)});

test('bootstrap drains a full historical page without replaying old events',async()=>{let eventCalls=0;const h=harness(async u=>u.includes('/organization')?resp(org):resp(++eventCalls===1?Array.from({length:200},(_,i)=>ev(i+1,'observation_recorded')):eventCalls===2?[ev(201,'observation_recorded')]:[]));await h.run('startLive()');flush(h);const before=h.snap('svg-kernel');await h.run('pollLive()');flush(h);assert.equal(h.run('live.cursor'),201);assert.deepEqual(h.snap('svg-kernel'),before)});

test('HTTP 401 observation response is refused rather than silently discarded',async()=>{const h=harness(async()=>({ok:false,status:401,json:async()=>({error:'unauthorized'})}));h.run('live.on=true;live.mission="m"');await h.run('postEvidence("machine failed")');assert.match(h.get('chip').textContent,/401|failed|refused/) });

test('successful observation response requires a receipt',async()=>{const h=harness(async()=>resp({revision:11}));h.run('live.on=true;live.mission="m"');await h.run('postEvidence("machine failed")');assert.match(h.get('chip').textContent,/unknown|unconfirmed|receipt/) });

test('planner summary after committed preserves the visible final state',async()=>{const h=harness();h.run('live.on=true;timeline.steps=[];live.state='+JSON.stringify(org)+';buildKernelLive($("svg-kernel"));applyLiveEvent({type:"change_proposed",payload:{change_id:"c",operations:["AssignResponsibility"]}});applyLiveEvent({type:"change_status_changed",payload:{change_id:"c",from:"READY",to:"COMMITTED"}});clock.t=1000;frame(0)');const token=h.nodes(h.get('svg-kernel')).find(n=>n.attrs.class==='token');assert.equal(token.attrs.r,'9');h.run('applyLiveEvent({type:"planner_decision",payload:{change_id:"c",verdict:"change_proposed"}})');assert.equal(token.attrs.r,'9')});

test('recorded playback does not invent intermediate revisions',async()=>{const h=harness();h.run('show(4)');for(let t=0;t<=23000;t+=100)h.run(`frame(${t})`);const rev=h.nodes(h.get('svg-recorded')).find(n=>n.attrs.class==='big');assert.match(rev.textContent,/r23.*r31|r31/);assert.notEqual(rev.textContent,'r25')});

test('old pending poll cannot resurrect live mode after exit',async()=>{let pendingResolve;let pending=false;const h=harness(async u=>u.includes('/organization')?(pending?new Promise(r=>pendingResolve=r):resp(org)):resp([]));await h.run('startLive()');pending=true;const p=h.run('pollLive()');h.run('stopLive()');pendingResolve(resp({...org,revision:99}));await p;assert.match(h.get('chip').textContent,/scripted/)});

test('polling is single-flight while an organization request is pending',async()=>{let calls=0,resolve;const h=harness(async()=>{calls++;return new Promise(r=>resolve=r)});h.run('live.on=true');const a=h.run('pollLive()'), b=h.run('pollLive()');assert.equal(calls,1);h.run('stopLive()');resolve(resp(org));await Promise.all([a,b])});

test('a multi-mission organization requires an explicit matching mission',async()=>{const h=harness(async u=>u.includes('/organization')?resp({...org,missions:{old:{},factory:{}}}):resp([]));await h.run('startLive()');assert.equal(h.run('live.mission'),null)});

test('a configured mission is selected only when present in the organization',async()=>{const h=harness(async u=>u.includes('/organization')?resp({...org,missions:{old:{},factory:{}}}):resp([]),'?mission=factory');await h.run('startLive()');assert.equal(h.run('live.mission'),'factory')});

test('legacy token URL is removed and never used as the agent credential',()=>{const h=harness(undefined,'?tok=secret-from-url&a=local&mission=factory');assert.doesNotMatch(h.location.search,/tok=|secret-from-url/);assert.notEqual(h.run('live.tok'),'secret-from-url')});

test('an event without a selected rung must not invent a rejected create-role proposal',()=>{const h=harness();h.run('live.on=true;timeline.steps=[];live.state='+JSON.stringify(org)+';buildKernelLive($("svg-kernel"));applyLiveEvent({type:"planner_decision",payload:{verdict:"escalated"}})');flush(h);const rungs=h.nodes(h.get('svg-kernel')).filter(n=>(n.attrs.class||'').includes('rung'));assert.equal(rungs.some(n=>(n.attrs.class||'').includes('refused')),false)});


test('a failed bootstrap snapshot does not mark the session initialized',async()=>{let orgCalls=0;const h=harness(async u=>u.includes('/organization')?(++orgCalls===2?{ok:false,status:503}:resp(org)):resp([]));await h.run('startLive()');assert.equal(h.run('live.started'),false);await h.run('pollLive()');flush(h);assert.equal(h.run('live.started'),true);assert.ok(h.run('live.k'))});

test('new event visuals wait while paused and apply after resume',async()=>{let events=[];const h=harness(async u=>u.includes('/organization')?resp(org):resp(events.splice(0)));await h.run('startLive()');flush(h);h.keys(' ');const before=h.snap('svg-kernel');events=[ev(1,'role_created',{name:'new specialist'})];await h.run('pollLive()');flush(h);assert.deepEqual(h.snap('svg-kernel'),before);h.keys(' ');flush(h);assert.ok(h.nodes(h.get('svg-kernel')).some(n=>n.textContent==='new specialist'))});

test('duplicate sequence responses are not animated twice',async()=>{let events=[];const h=harness(async u=>u.includes('/organization')?resp(org):resp(events));await h.run('startLive()');flush(h);h.run('globalThis.applied=[];applyLiveEvent=e=>applied.push(e.seq)');events=[ev(1,'observation_recorded')];await h.run('pollLive()');flush(h);await h.run('pollLive()');flush(h);assert.equal(h.run('JSON.stringify(applied)'),'[1]')});

test('confirmed observation clears the input only after receiving its ID',async()=>{let resolve;const h=harness(async()=>new Promise(r=>resolve=r));h.run('live.on=true;live.mission="m";evidenceInput.value="factory outage";evidenceInput.hidden=false');const pending=h.run('evidenceInput.handlers.keydown({key:"Enter",stopPropagation(){}})');assert.equal(h.run('evidenceInput.value'),'factory outage');resolve(resp({observation_id:'obs-confirmed'}));await pending;assert.equal(h.run('evidenceInput.value'),'');assert.equal(h.run('evidenceInput.hidden'),true);assert.match(h.get('chip').textContent,/obs-confirmed/)});

test('unknown report outcome preserves the draft and blocks automatic duplicate submission',async()=>{let count=0;const h=harness(async()=>{count++;throw Error('simulated transport error')});h.run('live.on=true;live.mission="m";evidenceInput.value="factory outage";evidenceInput.hidden=false');await h.run('evidenceInput.handlers.keydown({key:"Enter",stopPropagation(){}})');assert.equal(h.run('evidenceInput.value'),'factory outage');assert.equal(h.run('evidenceInput.hidden'),false);await h.run('postEvidence("factory outage")');assert.equal(count,1);assert.equal(h.run('live.reportUnknown'),true)});

test('leaving with an observation in flight retains unknown-outcome protection',async()=>{let resolve;const h=harness(async()=>new Promise(r=>resolve=r));h.run('live.on=true;live.mission="m"');const pending=h.run('postEvidence("factory outage")');h.run('stopLive()');assert.equal(h.run('live.reportUnknown'),true);resolve(resp({observation_id:'obs-late'}));await pending;assert.match(h.get('chip').textContent,/scripted/);assert.equal(h.run('live.reportUnknown'),true)});

test('a retired session response cannot overwrite the replacement session',async()=>{let pendingResolve,old=false;const h=harness(async u=>u.includes('/organization')?(old?new Promise(r=>{pendingResolve=r;old=false}):resp(org)):resp([]));await h.run('startLive()');old=true;const pending=h.run('pollLive()');h.run('stopLive()');await h.run('startLive()');pendingResolve(resp({...org,name:'old stale session',revision:99}));await pending;flush(h);assert.doesNotMatch(h.get('chip').textContent,/old stale/);assert.equal(h.run('live.state.revision'),10)});
