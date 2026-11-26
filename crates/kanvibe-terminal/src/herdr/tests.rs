//! TS `src/lib/__tests__/herdr.test.ts` 대응 테스트. 실제 herdr는 띄우지 않고, 명령은 가짜 실행기로, 서버와 터널은 임시 디렉터리의 가짜 프로그램과 소켓으로 대신한다.

mod api;
mod bootstrap;
mod client_config;
mod fixtures;
mod mirror_panes;
mod pane_size;
mod remote_machine;
mod session_names;
mod shared_spaces;
mod tab_move;
mod workspace_mapping;
