// 서버 bundle은 argv가 아닌 SSH의 private stdin 한 줄로 전달한다.
export const HOST_BOOTSTRAP_LOADER = String.raw`
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto');
process.stdout.write('{"type":"bootstrap-ready"}\n');
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
  if(bootstrap.length>end+1)throw new Error('Unexpected data after host bootstrap');
  require(sourcePath).startRemoteHostBridge({namespace:payload.namespace,sourcePath,buildHash:payload.hash},process.stdin,process.stdout).catch(fail);
 }catch(error){fail(error);}
};
process.stdin.on('data',receive);
`;
