//! daemon PTY를 띄우는 경계(TS node-pty). 실제 구현은 portable-pty이고, 테스트는 종료와 출력 끝을
//! 따로 일으키는 가짜로 "종료 후 출력을 비우는 동안" 같은 구간을 재현한다.

use std::io::{Read, Write};
use std::sync::Mutex;

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::environment::current_child_environment;

/// PTY 프로세스가 끝난 결과
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtyExitStatus {
    pub exit_code: i64,
}

/// 떠 있는 PTY의 크기 조절과 종료(SIGHUP)
pub trait PtyControl: Send + Sync {
    fn resize(&self, columns: u16, rows: u16) -> Result<(), String>;
    fn kill(&self);
}

/// 프로세스가 끝날 때까지 기다린다
pub trait PtyWaiter: Send {
    fn wait(self: Box<Self>) -> PtyExitStatus;
}

pub struct LaunchedPty {
    pub pid: u32,
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub control: Box<dyn PtyControl>,
    pub waiter: Box<dyn PtyWaiter>,
}

pub trait PtyLauncher: Send + Sync {
    /// `/bin/sh -lc <command>`를 `xterm-256color` PTY로 띄운다
    fn launch(&self, command: &str, columns: u16, rows: u16) -> Result<LaunchedPty, String>;
}

/// portable-pty로 띄운다. 자식에는 서버 런타임 값을 걷어 낸 환경만 넘기고, 작업 디렉터리는 HOME이다
pub struct NativePtyLauncher;

impl PtyLauncher for NativePtyLauncher {
    fn launch(&self, command: &str, columns: u16, rows: u16) -> Result<LaunchedPty, String> {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols: columns,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| error.to_string())?;
        let environment = current_child_environment();
        let mut builder = CommandBuilder::new("/bin/sh");
        builder.args(["-lc", command]);
        builder.env_clear();
        for (key, value) in &environment {
            builder.env(key, value);
        }
        builder.env("TERM", "xterm-256color");
        builder.cwd(environment.get("HOME").map(String::as_str).unwrap_or("/"));
        let child = pair
            .slave
            .spawn_command(builder)
            .map_err(|error| error.to_string())?;
        drop(pair.slave);
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| error.to_string())?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| error.to_string())?;
        let pid = child.process_id().ok_or("PTY process has no pid")?;
        let control = NativePtyControl {
            master: Mutex::new(pair.master),
            killer: Mutex::new(child.clone_killer()),
        };
        Ok(LaunchedPty {
            pid,
            reader,
            writer,
            control: Box::new(control),
            waiter: Box::new(NativePtyWaiter { child }),
        })
    }
}

struct NativePtyControl {
    master: Mutex<Box<dyn MasterPty + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
}

impl PtyControl for NativePtyControl {
    fn resize(&self, columns: u16, rows: u16) -> Result<(), String> {
        let size = PtySize {
            rows,
            cols: columns,
            pixel_width: 0,
            pixel_height: 0,
        };
        self.master
            .lock()
            .unwrap()
            .resize(size)
            .map_err(|error| error.to_string())
    }

    fn kill(&self) {
        let _ = self.killer.lock().unwrap().kill();
    }
}

struct NativePtyWaiter {
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl PtyWaiter for NativePtyWaiter {
    fn wait(mut self: Box<Self>) -> PtyExitStatus {
        let exit_code = self
            .child
            .wait()
            .map(|status| i64::from(status.exit_code()))
            .unwrap_or(255);
        PtyExitStatus { exit_code }
    }
}
