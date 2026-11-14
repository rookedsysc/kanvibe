/** @vitest-environment node */
import { EventEmitter } from 'node:events';
import { createRequire } from 'node:module';
import { describe, expect, it, vi } from 'vitest';
const require = createRequire(import.meta.url);
const { handleMobileRequest } = require('./mobileRoutes.js');
function request(method, url, body, authorization='Bearer good') {
  const req = new EventEmitter();
  Object.assign(req, {method,url,headers:{authorization},socket:{remoteAddress:'127.0.0.1'},destroy:vi.fn()});
  setImmediate(()=>{ if(body!==undefined)req.emit('data',JSON.stringify(body)); req.emit('end'); });
  return req;
}
async function run(bridge,method,url,body,authorization) {
  let status;let payload;
  const response={writeHead:(code)=>status=code,end:(json)=>payload=JSON.parse(json)};
  await handleMobileRequest(request(method,url,body,authorization),response,{bridge,host:'127.0.0.1',port:4885});
  return {status,payload};
}
function fixture() {return {authorizeMobileRequest:vi.fn(async(header)=>header==='Bearer good'?'device':null),getMobileSearchableTasks:vi.fn(async()=>[{id:'task-1',title:'searchable'}]),getMobileDoneTasks:vi.fn(async()=>({tasks:[{id:'old-done'}],doneTotal:21})),updateMobileTaskDetails:vi.fn(async(id,body)=>({id,...body})),getMobileTask:vi.fn(async(id)=>({id}))};}
describe('authenticated mobile parity routes',()=>{
  it('returns all searchable tasks from the provider',async()=>{const bridge=fixture();expect(await run(bridge,'GET','/api/mobile/tasks/search')).toEqual({status:200,payload:{success:true,tasks:[{id:'task-1',title:'searchable'}]}});expect(bridge.getMobileSearchableTasks).toHaveBeenCalledOnce();});
  it('does not invoke feature providers before authorization',async()=>{const bridge=fixture();const response=await run(bridge,'GET','/api/mobile/tasks/search',undefined,'Bearer bad');expect(response.status).toBe(401);expect(bridge.getMobileSearchableTasks).not.toHaveBeenCalled();});
  it('forwards validated done pagination and rejects negative offsets before delegation',async()=>{const bridge=fixture();expect((await run(bridge,'GET','/api/mobile/tasks/done?offset=20&limit=20')).status).toBe(200);expect(bridge.getMobileDoneTasks).toHaveBeenCalledWith(20,20);expect((await run(bridge,'GET','/api/mobile/tasks/done?offset=-1')).status).toBe(400);expect(bridge.getMobileDoneTasks).toHaveBeenCalledTimes(1);});
  it('updates only task details through the explicit endpoint',async()=>{const bridge=fixture();const response=await run(bridge,'PATCH','/api/mobile/tasks/task-1/details',{description:'updated',priority:null});expect(response.status).toBe(200);expect(bridge.updateMobileTaskDetails).toHaveBeenCalledWith('task-1',{description:'updated',priority:null});});
  it('returns safe generic errors for unexpected provider failures',async()=>{const bridge=fixture();bridge.getMobileSearchableTasks.mockRejectedValueOnce(new Error('secret command'));const response=await run(bridge,'GET','/api/mobile/tasks/search');expect(response).toEqual({status:500,payload:{success:false,error:'서버 오류'}});});
});
