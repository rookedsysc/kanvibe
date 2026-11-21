import { createRequire } from 'node:module';
import { describe,expect,it } from 'vitest';
const {registerDiagramWorkerWindow,isDiagramWorkerWindow,getUserWindows}=createRequire(import.meta.url)('./diagramWorkerWindows.js');
describe('diagram worker window isolation',()=>{
 it('excludes only registered workers from ordinary user-window discovery',()=>{
  const user={id:1},other={id:3},worker={id:2};
  registerDiagramWorkerWindow(worker);
  expect(isDiagramWorkerWindow(worker)).toBe(true);
  expect(isDiagramWorkerWindow(user)).toBe(false);
  expect(getUserWindows({getAllWindows:()=>[user,worker,other]})).toEqual([user,other]);
  const BrowserWindow={getAllWindows:()=>[user,worker,other]};
  const first=getUserWindows(BrowserWindow); first.pop();
  expect(getUserWindows(BrowserWindow)).toEqual([user,other]);
 });
});
