// 서버 bundle은 argv가 아닌 SSH의 private stdin 한 줄로 전달한다.
export const HOST_BOOTSTRAP_LOADER = String.raw`
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto');
process.stdout.write(JSON.stringify({type:'bootstrap-ready',platform:process.platform,arch:process.arch,napi:Number(process.versions.napi||0)})+'\n');
let bootstrap=Buffer.alloc(0);
const fail=error=>{process.stderr.write(String(error.message||error)+'\n');process.exitCode=1;process.stdin.destroy();};
const receive=chunk=>{
 try{
  bootstrap=Buffer.concat([bootstrap,chunk]);
  if(bootstrap.length>16777216)throw new Error('Host bootstrap size exceeded');
  const end=bootstrap.indexOf(10);if(end<0)return;
  process.stdin.off('data',receive);process.stdin.pause();
  const payload=JSON.parse(bootstrap.subarray(0,end).toString('utf8'));
  if(!/^(dev|prod|qa-[a-z0-9-]{1,24})$/.test(payload.namespace)||typeof payload.source!=='string'||!/^[a-f0-9]{64}$/.test(payload.hash)||crypto.createHash('sha256').update(payload.source).digest('hex')!==payload.hash)throw new Error('Invalid host bootstrap');
  if(process.platform==='win32'||Number(process.versions.node.split('.')[0])<18||!process.getuid)throw new Error('Unsupported host runtime');
  const uid=process.getuid(),parent='/tmp/kv-'+uid,directory=parent+'/'+payload.namespace;
  for(const folder of [parent,directory]){
   try{fs.mkdirSync(folder,{mode:0o700});}catch(error){if(error.code!=='EEXIST')throw error;}
   const stat=fs.lstatSync(folder);
   if(!stat.isDirectory()||stat.isSymbolicLink()||stat.uid!==uid||(stat.mode&0o777)!==0o700)throw new Error('Untrusted host private directory');
  }
  const sourcePath=path.join(directory,'server-'+payload.hash+'.cjs');
  const pendingPath=sourcePath+'.'+crypto.randomBytes(12).toString('hex')+'.tmp';
  try{
   const file=fs.openSync(pendingPath,fs.constants.O_WRONLY|fs.constants.O_CREAT|fs.constants.O_EXCL|fs.constants.O_NOFOLLOW,0o600);
   try{fs.writeFileSync(file,payload.source);fs.fsyncSync(file);}finally{fs.closeSync(file);}
   try{fs.linkSync(pendingPath,sourcePath);}catch(error){if(error.code!=='EEXIST')throw error;}
  }finally{try{fs.unlinkSync(pendingPath);}catch(error){if(error.code!=='ENOENT')throw error;}}
  const stat=fs.lstatSync(sourcePath);
  if(!stat.isFile()||stat.isSymbolicLink()||stat.uid!==uid||(stat.mode&0o777)!==0o600||crypto.createHash('sha256').update(fs.readFileSync(sourcePath)).digest('hex')!==payload.hash)throw new Error('Host bundle integrity mismatch');
  let ptyModulePath;
  if(payload.pty){
   const artifact=payload.pty;
   if(artifact.platform!==process.platform+'-'+process.arch||Number(process.versions.napi||0)<8||!Array.isArray(artifact.files)||artifact.files.length>32||!/^[a-f0-9]{64}$/.test(artifact.hash)||crypto.createHash('sha256').update(JSON.stringify(artifact.files)).digest('hex')!==artifact.hash)throw new Error('PTY artifact platform, ABI or manifest mismatch');
   const nativeRoot=path.join(directory,'pty-'+artifact.hash);
   try{fs.mkdirSync(nativeRoot,{mode:0o700});}catch(error){if(error.code!=='EEXIST')throw error;}
   const privateFolder=folder=>{const s=fs.lstatSync(folder);if(!s.isDirectory()||s.isSymbolicLink()||s.uid!==uid||(s.mode&0o777)!==0o700)throw new Error('Untrusted PTY directory');};
   privateFolder(nativeRoot);
   for(const entry of artifact.files){
    if(typeof entry.path!=='string'||!(/^(lib\/[A-Za-z0-9_-]+\.js|package\.json|LICENSE)$/.test(entry.path)||new RegExp('^prebuilds/'+artifact.platform+'/(pty\\.node|spawn-helper)$').test(entry.path))||!([384,448].includes(entry.mode))||!/^[a-f0-9]{64}$/.test(entry.sha256)||typeof entry.data!=='string')throw new Error('Invalid PTY artifact entry');
    const bytes=Buffer.from(entry.data,'base64');if(bytes.length>8388608||bytes.toString('base64')!==entry.data||crypto.createHash('sha256').update(bytes).digest('hex')!==entry.sha256)throw new Error('PTY artifact hash mismatch');
    const target=path.join(nativeRoot,entry.path);let folder=nativeRoot;
    for(const segment of entry.path.split('/').slice(0,-1)){folder=path.join(folder,segment);try{fs.mkdirSync(folder,{mode:0o700});}catch(error){if(error.code!=='EEXIST')throw error;}privateFolder(folder);}
    const pending=target+'.'+crypto.randomBytes(12).toString('hex')+'.tmp';
    try{fs.writeFileSync(pending,bytes,{mode:entry.mode,flag:'wx'});try{fs.linkSync(pending,target);}catch(error){if(error.code!=='EEXIST')throw error;}}finally{try{fs.unlinkSync(pending);}catch(error){if(error.code!=='ENOENT')throw error;}}
    const s=fs.lstatSync(target);if(!s.isFile()||s.isSymbolicLink()||s.uid!==uid||(s.mode&0o777)!==entry.mode||crypto.createHash('sha256').update(fs.readFileSync(target)).digest('hex')!==entry.sha256)throw new Error('Untrusted PTY artifact');
   }
   ptyModulePath=path.join(nativeRoot,'lib/index.js');
   require(ptyModulePath); // daemon 시작 전 libc/Node-API를 포함한 native 로딩 검증.
  }
  if(bootstrap.length>end+1)throw new Error('Unexpected data after host bootstrap');
  require(sourcePath).startRemoteHostBridge({namespace:payload.namespace,sourcePath,buildHash:payload.hash,ptyModulePath,ptyArtifactHash:payload.pty?.hash},process.stdin,process.stdout).catch(fail);
 }catch(error){fail(error);}
};
process.stdin.on('data',receive);
`;
