//! TS `projectService.test.ts` 이식. TS가 `vi.mock`으로 바꾸던 협력 모듈은 `fakes`의 가짜로, DB는 임시 SQLite로 대신한다.

mod deleted_paths;
mod fakes;
mod hook_install;
mod listing;
mod registration;
mod remote_hooks;
mod shared_state;
mod worktree_registration;
