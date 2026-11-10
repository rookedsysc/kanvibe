const fs=require('node:fs');const path=require('node:path');const assert=require('node:assert/strict');const {execFileSync}=require('node:child_process');
const {launchKanVibeTauri}=require('../lib/launchTauri.cjs');
const out=path.resolve(process.argv[2]||`qa-results/mobile-stream-${Date.now()}`);
const checks=[];const sockets=[];
const delay=ms=>new Promise(r=>setTimeout(r,ms));
const visible = s => {
 let out='';
 for(let i=0;i<s.length;i++) {
  if(s.charCodeAt(i)===27 && s[i+1]==='[') { i+=2;while(i<s.length && (s.charCodeAt(i)<64 || s.charCodeAt(i)>126))i++;continue; }
  if(s.charCodeAt(i)===27 && s[i+1]===']') { i+=2;while(i<s.length && s.charCodeAt(i)!==7 && !(s.charCodeAt(i)===27 && s.charCodeAt(i+1)===92))i++;if(s.charCodeAt(i)===27)i++;continue; }
  out+=s[i];
 }
 return out;
};
async function until(read,test,label){let last;for(let i=0;i<150;i++){last=await read();if(test(last))return last;await delay(100);}throw Error(label+': '+JSON.stringify(last));}
async function run(){
 if(fs.existsSync(path.join(out,'app-data')))throw new Error('Use a fresh QA directory');fs.mkdirSync(out,{recursive:true});const repo=path.join(out,'mobile-native-fixture-'+Date.now());fs.mkdirSync(repo);
 const git=(...args)=>execFileSync('git',['-C',repo,...args],{encoding:'utf8'});
 git('init','-b','main');git('config','user.name','KanVibe QA');git('config','user.email','qa@kanvibe.local');fs.writeFileSync(path.join(repo,'README.md'),'QA\n');git('add','.');git('commit','-m','QA fixture');
 const {app,page}=await launchKanVibeTauri({appDataDir:path.join(out,'app-data'),hookPort:19740});
 const invoke=(namespace,method,...args)=>page.evaluate(({namespace,method,args})=>window.kanvibeDesktop.invoke(namespace,method,args),{namespace,method,args});
 let token;const created=[];
 const request=async(method,url,body,auth=token)=>{const r=await fetch('http://127.0.0.1:19740/api/mobile'+url,{method,headers:{'content-type':'application/json',...(auth?{authorization:'Bearer '+auth}:{})},...(body===undefined?{}:{body:JSON.stringify(body)}),signal:AbortSignal.timeout(60000)});return {status:r.status,body:await r.json()};};
 const mark=(name,detail)=>{checks.push({name,ok:true,detail});console.log('PASS '+name);};
 try{
  await invoke('appSettings','setBackgroundSyncEnabled',false);
  const project=await invoke('project','registerProject','Mobile Native QA',repo);assert.equal(project.success,true,project.error);
  const pairing=await invoke('mobileBridge','startMobilePairing');const paired=await request('POST','/pair',{code:pairing.code,deviceName:'Native protocol QA'});assert.equal(paired.status,200);token=paired.body.token;
  async function create(kind){await invoke('appSettings','setDefaultSessionType',kind);const r=await request('POST','/tasks',{title:kind+' mobile',branchName:'qa-'+kind,baseBranch:'main',projectId:project.project.id,sessionType:kind});assert.equal(r.status,200,JSON.stringify(r));created.push(r.body.task.id);return r.body.task;}
  const task=await create('herdr');mark('mobile explicit Herdr task creation');
  const result=await request('GET',`/tasks/${task.id}/surfaces`);assert.equal(result.status,200,JSON.stringify(result));assert.equal(result.body.surfaces.canResize,false);assert.equal(result.body.surfaces.sessionType,'herdr');
  const pane=result.body.surfaces.tabs[0].panes[0];assert.ok(pane.width>0&&pane.height>0);mark('actual Herdr session bootstrap, pane grouping and dimensions',result.body.surfaces);
  async function stream(){const ws=new WebSocket(`ws://127.0.0.1:19740/api/mobile/stream?taskId=${task.id}&paneId=${encodeURIComponent(pane.id)}`,{headers:{authorization:'Bearer '+token}});const data={ws,messages:[],closed:false};sockets.push(ws);ws.addEventListener('message',event=>data.messages.push(String(event.data)));ws.addEventListener('close',()=>{data.closed=true;});await new Promise((resolve,reject)=>{ws.addEventListener('open',resolve,{once:true});ws.addEventListener('error',reject,{once:true});});await until(()=>data.messages,a=>a.length>0,'No initial Herdr screen');return data;}
  const first=await stream();
  const marker='HERDR_UTF8_한글';const escaped=[...Buffer.from(marker+'\n')].map(x=>'\\'+x.toString(8).padStart(3,'0')).join('');
  first.ws.send(`printf '${escaped}'\r`);await until(()=>first.messages.join(''),s=>visible(s).includes(marker),'Herdr input/output missing');mark('actual Herdr observe frames and UTF8 keyboard input');
  first.ws.send(`i=1; while [ "$i" -le 100 ]; do printf '\\110\\111\\123\\124\\117\\122\\131-%s\\n' "$i"; i=$((i+1)); done\r`);
  await until(()=>first.messages.join(''),s=>visible(s).includes('HISTORY-100'),'Live frame missing');
  await delay(1500);
  const late=await stream();await until(()=>late.messages.join(''),s=>visible(s).includes('HISTORY-1')&&visible(s).includes('HISTORY-100'),'Late subscriber lost history');
  assert.equal(first.closed,false);mark('quiet history refresh and late subscriber complete scrollback');
  const action=async(p,act,id=task.id,auth=token)=>request('POST',`/tasks/${id}/panes/${encodeURIComponent(p)}/shortcuts`,{action:act},auth);
  const unauthorized=await action(pane.id,'split-down',task.id,'invalid');assert.equal(unauthorized.status,401);
  const invalid=await action(pane.id,'bad-action');assert.equal(invalid.status,400);
  const foreign=await action('w999:p999','split-right');assert.equal(foreign.status,409);
  const unsupported=await action(pane.id,'copy-mode');assert.equal(unsupported.status,400);mark('shortcut auth, action and foreign pane guards');
  const zoom=await action(pane.id,'toggle-fullscreen');assert.equal(zoom.status,200,JSON.stringify(zoom));
  const split=await action(pane.id,'split-down');assert.equal(split.status,200,JSON.stringify(split));assert.ok(split.body.surfaces.tabs.some(tab=>tab.panes.length>1));mark('Herdr zoom and split discovery',split.body.surfaces);
  late.ws.close();first.ws.send(`printf '\\123\\125\\122\\126\\111\\126\\105\\104\\n'\r`);await until(()=>first.messages.join(''),s=>visible(s).includes('SURVIVED'),'Dropping second subscriber killed first');first.ws.close();await delay(300);mark('dropping one subscriber retains live observer until last client drops');
  const tmux=await create('tmux');const ts=await request('GET',`/tasks/${tmux.id}/surfaces`);assert.equal(ts.status,200,JSON.stringify(ts));const tp=ts.body.surfaces.tabs[0].panes[0];
  const copy=await action(tp.id,'copy-mode',tmux.id);assert.equal(copy.status,200,JSON.stringify(copy));assert.equal(typeof copy.body.screen,'string');
  const bottom=await action(tp.id,'scroll-bottom',tmux.id);assert.equal(bottom.status,200);assert.equal(typeof bottom.body.screen,'string');
  const splitTmux=await action(tp.id,'split-right',tmux.id);assert.equal(splitTmux.status,200);assert.ok(splitTmux.body.surfaces.tabs.some(tab=>tab.panes.length>1));mark('tmux copy mode screen, scroll bottom and split discovery');
  fs.writeFileSync(path.join(out,'result.json'),JSON.stringify({ok:true,checks},null,2));
 }catch(e){fs.writeFileSync(path.join(out,'result.json'),JSON.stringify({ok:false,checks,error:e.stack},null,2));throw e;}
 finally{for(const s of sockets)s.close();for(const id of created)await invoke('kanban','deleteTask',id).catch(e=>console.error('cleanup',e));fs.writeFileSync(path.join(out,'runtime.log'),app.output());await app.close();}
}
run().catch(e=>{console.error(e);process.exitCode=1;});
