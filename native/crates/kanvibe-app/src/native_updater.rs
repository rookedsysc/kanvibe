use std::{
    fs,
    path::{Path, PathBuf},
};
#[cfg(target_os = "macos")]
use std::{
    fs::File,
    io::{Read, Write},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
#[cfg(target_os = "macos")]
use sha2::{Digest, Sha256};

const UPDATE_JOURNAL_SCHEMA_VERSION: u32 = 1;
const APP_BUNDLE_NAME: &str = "KanVibe.app";
const MAX_UPDATE_JOURNAL_BYTES: u64 = 64 * 1024;
pub const QA_FORCE_UPDATE_HEALTH_TIMEOUT_ENV: &str = "KANVIBE_QA_FORCE_UPDATE_HEALTH_TIMEOUT";

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeUpdateState {
    Armed,
    CurrentBackedUp,
    AwaitingHealth,
    Committed,
    RolledBack,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeUpdateJournal {
    schema_version: u32,
    pub current_app: PathBuf,
    pub staged_app: PathBuf,
    pub backup_app: PathBuf,
    pub health_file: PathBuf,
    pub expected_version: String,
    pub nonce: String,
    pub health_token: String,
    pub state: NativeUpdateState,
}

impl NativeUpdateJournal {
    pub fn new(
        current_app: impl Into<PathBuf>,
        expected_version: &str,
        nonce: &str,
        health_token: &str,
    ) -> Result<Self, String> {
        let current_app = current_app.into();
        if !current_app.is_absolute()
            || current_app.file_name().and_then(|name| name.to_str()) != Some(APP_BUNDLE_NAME)
        {
            return Err("current update target must be an absolute KanVibe.app path".to_owned());
        }
        if !is_release_version(expected_version) {
            return Err("expected update version must be MAJOR.MINOR.PATCH".to_owned());
        }
        if !is_lower_hex(nonce, 32) || !is_lower_hex(health_token, 64) {
            return Err("update nonce/token must use their exact lowercase hex format".to_owned());
        }
        let parent = current_app
            .parent()
            .ok_or_else(|| "current app has no parent directory".to_owned())?;
        let staged_app = parent.join(format!(".KanVibe.update-{nonce}.app"));
        let backup_app = parent.join(format!(".KanVibe.rollback-{nonce}.app"));
        let health_file = parent.join(format!(".KanVibe.update-{nonce}.healthy"));
        let journal = Self {
            schema_version: UPDATE_JOURNAL_SCHEMA_VERSION,
            current_app,
            staged_app,
            backup_app,
            health_file,
            expected_version: expected_version.to_owned(),
            nonce: nonce.to_owned(),
            health_token: health_token.to_owned(),
            state: NativeUpdateState::Armed,
        };
        journal.validate()?;
        Ok(journal)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != UPDATE_JOURNAL_SCHEMA_VERSION
            || !is_release_version(&self.expected_version)
            || !is_lower_hex(&self.nonce, 32)
            || !is_lower_hex(&self.health_token, 64)
        {
            return Err("invalid native update journal metadata".to_owned());
        }
        if !self.current_app.is_absolute()
            || self.current_app.file_name().and_then(|name| name.to_str()) != Some(APP_BUNDLE_NAME)
        {
            return Err("current update target must be an absolute KanVibe.app path".to_owned());
        }
        let parent = self
            .current_app
            .parent()
            .ok_or_else(|| "current app has no parent directory".to_owned())?;
        if self.staged_app != parent.join(format!(".KanVibe.update-{}.app", self.nonce))
            || self.backup_app != parent.join(format!(".KanVibe.rollback-{}.app", self.nonce))
            || self.health_file != parent.join(format!(".KanVibe.update-{}.healthy", self.nonce))
        {
            return Err("native update paths escaped their installed-app directory".to_owned());
        }
        Ok(())
    }

    pub fn read(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let metadata = fs::metadata(path)
            .map_err(|error| format!("update journal is unavailable: {error}"))?;
        if metadata.len() == 0 || metadata.len() > MAX_UPDATE_JOURNAL_BYTES {
            return Err("update journal size is invalid".to_owned());
        }
        let payload = fs::read(path)
            .map_err(|error| format!("could not read native update journal: {error}"))?;
        let journal = serde_json::from_slice::<Self>(&payload)
            .map_err(|error| format!("invalid native update journal: {error}"))?;
        journal.validate()?;
        Ok(journal)
    }

    pub fn write(&self, path: impl AsRef<Path>) -> Result<(), String> {
        self.validate()?;
        let path = path.as_ref();
        if !path.is_absolute() {
            return Err("update journal path must be absolute".to_owned());
        }
        let parent = path
            .parent()
            .ok_or_else(|| "update journal has no parent directory".to_owned())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create update journal directory: {error}"))?;
        let temporary = path.with_extension("json.tmp");
        let payload = serde_json::to_vec_pretty(self)
            .map_err(|error| format!("could not serialize update journal: {error}"))?;
        fs::write(&temporary, payload)
            .map_err(|error| format!("could not write update journal: {error}"))?;
        fs::rename(&temporary, path)
            .map_err(|error| format!("could not commit update journal: {error}"))
    }
}

pub fn replace_staged_app(
    journal: &mut NativeUpdateJournal,
    journal_path: impl AsRef<Path>,
) -> Result<(), String> {
    journal.validate()?;
    if journal.state != NativeUpdateState::Armed
        || !journal.current_app.is_dir()
        || !journal.staged_app.is_dir()
        || journal.backup_app.exists()
    {
        return Err("native update is not in a replaceable state".to_owned());
    }

    fs::rename(&journal.current_app, &journal.backup_app)
        .map_err(|error| format!("could not preserve current app for rollback: {error}"))?;
    journal.state = NativeUpdateState::CurrentBackedUp;
    if let Err(error) = journal.write(&journal_path) {
        let rollback = fs::rename(&journal.backup_app, &journal.current_app);
        journal.state = NativeUpdateState::RolledBack;
        let _ = journal.write(&journal_path);
        return match rollback {
            Ok(()) => Err(format!(
                "could not persist replacement state and restored the prior app: {error}"
            )),
            Err(rollback_error) => Err(format!(
                "could not persist replacement state ({error}); restoring the prior app also failed ({rollback_error})"
            )),
        };
    }

    if let Err(error) = fs::rename(&journal.staged_app, &journal.current_app) {
        let rollback = fs::rename(&journal.backup_app, &journal.current_app);
        journal.state = NativeUpdateState::RolledBack;
        let _ = journal.write(&journal_path);
        return match rollback {
            Ok(()) => Err(format!(
                "candidate install failed and was rolled back: {error}"
            )),
            Err(rollback_error) => Err(format!(
                "candidate install failed ({error}); restoring the prior app also failed ({rollback_error})"
            )),
        };
    }

    journal.state = NativeUpdateState::AwaitingHealth;
    if let Err(error) = journal.write(&journal_path) {
        let _ = fs::remove_dir_all(&journal.current_app);
        let rollback = fs::rename(&journal.backup_app, &journal.current_app);
        journal.state = NativeUpdateState::RolledBack;
        let _ = journal.write(&journal_path);
        return match rollback {
            Ok(()) => Err(format!(
                "could not persist candidate state and restored the prior app: {error}"
            )),
            Err(rollback_error) => Err(format!(
                "could not persist candidate state ({error}); restoring the prior app also failed ({rollback_error})"
            )),
        };
    }
    Ok(())
}

pub fn acknowledge_update_health(
    journal_path: impl AsRef<Path>,
    token: &str,
    running_version: &str,
) -> Result<bool, String> {
    let journal = NativeUpdateJournal::read(journal_path)?;
    if journal.state != NativeUpdateState::AwaitingHealth
        || token != journal.health_token
        || running_version != journal.expected_version
    {
        return Ok(false);
    }
    fs::write(&journal.health_file, token)
        .map_err(|error| format!("could not acknowledge native update health: {error}"))?;
    Ok(true)
}

pub fn acknowledge_update_health_from_process_args(running_version: &str) -> Result<bool, String> {
    if std::env::var_os(QA_FORCE_UPDATE_HEALTH_TIMEOUT_ENV).is_some() {
        return Ok(false);
    }
    let mut arguments = std::env::args_os().skip(1);
    let mut journal_path = None::<PathBuf>;
    let mut token = None::<String>;
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--native-update-health-journal") => {
                journal_path = arguments.next().map(PathBuf::from);
            }
            Some("--native-update-health-token") => {
                token = arguments.next().and_then(|value| value.into_string().ok());
            }
            _ => {}
        }
    }
    let (Some(journal_path), Some(token)) = (journal_path, token) else {
        return Ok(false);
    };
    let journal = NativeUpdateJournal::read(&journal_path)?;
    let executable = std::env::current_exe()
        .map_err(|error| format!("could not identify updated executable: {error}"))?;
    let running_app = executable
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or_else(|| "updated executable is not inside a macOS app bundle".to_owned())?;
    let running_app = fs::canonicalize(running_app)
        .map_err(|error| format!("could not resolve running app bundle: {error}"))?;
    let installed_app = fs::canonicalize(&journal.current_app)
        .map_err(|error| format!("could not resolve installed app bundle: {error}"))?;
    if running_app != installed_app {
        return Err("health acknowledgement did not come from the installed app".to_owned());
    }
    acknowledge_update_health(journal_path, &token, running_version)
}

#[cfg(all(target_os = "macos", feature = "native-ui"))]
pub fn prepare_native_update(
    installer: &crate::NativeReleaseInstaller,
    expected_version: &str,
    app_data_dir: impl AsRef<Path>,
) -> Result<PathBuf, String> {
    let current_executable = std::env::current_exe()
        .map_err(|error| format!("could not identify running KanVibe app: {error}"))?;
    let current_app = current_executable
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or_else(|| "KanVibe updater requires a packaged macOS app".to_owned())?
        .to_path_buf();
    let mut random = [0_u8; 48];
    File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut random))
        .map_err(|error| format!("could not create update nonce: {error}"))?;
    let nonce = hex_bytes(&random[..16]);
    let health_token = hex_bytes(&random[16..]);
    let journal = NativeUpdateJournal::new(current_app, expected_version, &nonce, &health_token)?;
    if journal.staged_app.exists() || journal.backup_app.exists() || journal.health_file.exists() {
        return Err("generated native update paths already exist".to_owned());
    }

    let update_dir = app_data_dir.as_ref().join("native-updates");
    fs::create_dir_all(&update_dir)
        .map_err(|error| format!("could not create native update directory: {error}"))?;
    let journal_path = update_dir.join(format!("update-{nonce}.json"));
    let dmg_path = update_dir.join(format!("{nonce}-{}", installer.asset_name));
    download_verified_dmg(installer, &dmg_path)?;
    let result = verify_and_stage_dmg(&dmg_path, &journal);
    let _ = fs::remove_file(&dmg_path);
    result?;
    journal.write(&journal_path)?;
    Ok(journal_path)
}

#[cfg(target_os = "macos")]
pub fn spawn_update_helper(journal_path: impl AsRef<Path>) -> Result<(), String> {
    let journal_path = journal_path.as_ref();
    let journal = NativeUpdateJournal::read(journal_path)?;
    if journal.state != NativeUpdateState::Armed {
        return Err("native update journal is not armed".to_owned());
    }
    let helper = journal.current_app.join("Contents/Helpers/KanVibeUpdater");
    if !helper.is_file() {
        return Err(format!(
            "signed native update helper is missing: {}",
            helper.display()
        ));
    }
    Command::new(helper)
        .arg("--journal")
        .arg(journal_path)
        .arg("--old-pid")
        .arg(std::process::id().to_string())
        .spawn()
        .map_err(|error| format!("could not start native update helper: {error}"))?;
    Ok(())
}

#[cfg(all(target_os = "macos", feature = "native-ui"))]
fn download_verified_dmg(
    installer: &crate::NativeReleaseInstaller,
    destination: &Path,
) -> Result<(), String> {
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(60)))
        .user_agent("KanVibe")
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = agent
        .get(&installer.download_url)
        .header("Accept", "application/octet-stream")
        .call()
        .map_err(|error| format!("release DMG download failed: {error}"))?;
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(installer.size.saturating_add(1))
        .reader();
    let partial = destination.with_extension("dmg.part");
    let mut output =
        File::create(&partial).map_err(|error| format!("could not create release DMG: {error}"))?;
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("could not read release DMG: {error}"))?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > installer.size {
            let _ = fs::remove_file(&partial);
            return Err("release DMG exceeded its declared size".to_owned());
        }
        digest.update(&buffer[..read]);
        output
            .write_all(&buffer[..read])
            .map_err(|error| format!("could not write release DMG: {error}"))?;
    }
    output
        .sync_all()
        .map_err(|error| format!("could not flush release DMG: {error}"))?;
    let actual_digest = format!("{:x}", digest.finalize());
    if total != installer.size || actual_digest != installer.sha256 {
        let _ = fs::remove_file(&partial);
        return Err("release DMG size or SHA-256 did not match GitHub metadata".to_owned());
    }
    fs::rename(&partial, destination)
        .map_err(|error| format!("could not commit verified release DMG: {error}"))
}

#[cfg(all(target_os = "macos", feature = "native-ui"))]
fn verify_and_stage_dmg(dmg: &Path, journal: &NativeUpdateJournal) -> Result<(), String> {
    verify_status(
        "/usr/bin/codesign",
        &["--verify", "--strict", "--verbose=2"],
        dmg,
        "DMG code signature",
    )?;
    verify_status(
        "/usr/bin/xcrun",
        &["stapler", "validate"],
        dmg,
        "DMG notarization ticket",
    )?;
    verify_status(
        "/usr/sbin/spctl",
        &[
            "--assess",
            "--type",
            "open",
            "--context",
            "context:primary-signature",
            "--verbose=2",
        ],
        dmg,
        "DMG Gatekeeper assessment",
    )?;
    let current_team = signature_team_id(&journal.current_app)?;
    if signature_team_id(dmg)? != current_team {
        return Err("release DMG signing team does not match installed KanVibe".to_owned());
    }

    let mount_dir = std::env::temp_dir().join(format!("kanvibe-update-mount-{}", journal.nonce));
    if mount_dir.exists() {
        return Err("native update mount directory already exists".to_owned());
    }
    fs::create_dir(&mount_dir)
        .map_err(|error| format!("could not create native update mount: {error}"))?;
    let attach = Command::new("/usr/bin/hdiutil")
        .args(["attach", "-readonly", "-nobrowse", "-mountpoint"])
        .arg(&mount_dir)
        .arg(dmg)
        .status()
        .map_err(|error| format!("could not mount release DMG: {error}"))?;
    if !attach.success() {
        let _ = fs::remove_dir(&mount_dir);
        return Err(format!("mounting release DMG failed with {attach}"));
    }

    let candidate = mount_dir.join(APP_BUNDLE_NAME);
    let verification = (|| -> Result<(), String> {
        verify_app_bundle(&candidate, &journal.expected_version, &current_team)?;
        let status = Command::new("/usr/bin/ditto")
            .arg(&candidate)
            .arg(&journal.staged_app)
            .status()
            .map_err(|error| format!("could not stage verified KanVibe app: {error}"))?;
        if !status.success() {
            return Err(format!("staging verified KanVibe app failed with {status}"));
        }
        verify_app_bundle(
            &journal.staged_app,
            &journal.expected_version,
            &current_team,
        )
    })();
    let detach = Command::new("/usr/bin/hdiutil")
        .arg("detach")
        .arg(&mount_dir)
        .status();
    if let Ok(status) = &detach
        && status.success()
    {
        let _ = fs::remove_dir(&mount_dir);
    }
    if !matches!(detach, Ok(status) if status.success()) {
        let _ = fs::remove_dir_all(&journal.staged_app);
        return Err("could not detach verified release DMG".to_owned());
    }
    if verification.is_err() {
        let _ = fs::remove_dir_all(&journal.staged_app);
    }
    verification
}

#[cfg(all(target_os = "macos", feature = "native-ui"))]
fn verify_app_bundle(
    app: &Path,
    expected_version: &str,
    expected_team: &str,
) -> Result<(), String> {
    if !app.is_dir() {
        return Err("release DMG does not contain KanVibe.app".to_owned());
    }
    verify_status(
        "/usr/bin/codesign",
        &["--verify", "--deep", "--strict", "--verbose=2"],
        app,
        "candidate app code signature",
    )?;
    verify_status(
        "/usr/bin/xcrun",
        &["stapler", "validate"],
        app,
        "candidate app notarization ticket",
    )?;
    verify_status(
        "/usr/sbin/spctl",
        &["--assess", "--type", "execute", "--verbose=2"],
        app,
        "candidate app Gatekeeper assessment",
    )?;
    if signature_team_id(app)? != expected_team {
        return Err("candidate app signing team does not match installed KanVibe".to_owned());
    }
    let plist = app.join("Contents/Info.plist");
    if plist_value(&plist, "CFBundleIdentifier")? != "com.kanvibe.desktop" {
        return Err("candidate app bundle identifier is invalid".to_owned());
    }
    if plist_value(&plist, "CFBundleShortVersionString")? != expected_version {
        return Err("candidate app version does not match the selected release".to_owned());
    }
    Ok(())
}

#[cfg(all(target_os = "macos", feature = "native-ui"))]
fn verify_status(
    program: &str,
    arguments: &[&str],
    target: &Path,
    label: &str,
) -> Result<(), String> {
    let status = Command::new(program)
        .args(arguments)
        .arg(target)
        .status()
        .map_err(|error| format!("could not run {label}: {error}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("{label} failed with {status}"))
}

#[cfg(all(target_os = "macos", feature = "native-ui"))]
fn signature_team_id(target: &Path) -> Result<String, String> {
    let output = Command::new("/usr/bin/codesign")
        .args(["-d", "--verbose=4"])
        .arg(target)
        .output()
        .map_err(|error| format!("could not inspect code signature: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "code signature inspection failed with {}",
            output.status
        ));
    }
    let details = String::from_utf8_lossy(&output.stderr);
    let team = details
        .lines()
        .find_map(|line| line.strip_prefix("TeamIdentifier="))
        .filter(|team| !team.is_empty() && *team != "not set")
        .ok_or_else(|| "code signature has no TeamIdentifier".to_owned())?;
    if !details
        .lines()
        .any(|line| line.starts_with("Authority=Developer ID Application:"))
    {
        return Err("code signature is not a Developer ID Application signature".to_owned());
    }
    Ok(team.to_owned())
}

#[cfg(all(target_os = "macos", feature = "native-ui"))]
fn plist_value(plist: &Path, key: &str) -> Result<String, String> {
    let output = Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(plist)
        .output()
        .map_err(|error| format!("could not inspect candidate Info.plist: {error}"))?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .ok_or_else(|| format!("candidate Info.plist is missing {key}"))
}

#[cfg(target_os = "macos")]
fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

pub fn finish_or_rollback_update(
    journal: &mut NativeUpdateJournal,
    journal_path: impl AsRef<Path>,
    healthy: bool,
) -> Result<(), String> {
    if journal.state != NativeUpdateState::AwaitingHealth {
        return Err("native update is not awaiting a health acknowledgement".to_owned());
    }
    if healthy {
        fs::remove_dir_all(&journal.backup_app)
            .map_err(|error| format!("could not remove committed rollback app: {error}"))?;
        let _ = fs::remove_file(&journal.health_file);
        journal.state = NativeUpdateState::Committed;
        return journal.write(journal_path);
    }

    if journal.current_app.exists() {
        fs::remove_dir_all(&journal.current_app)
            .map_err(|error| format!("could not remove failed candidate app: {error}"))?;
    }
    fs::rename(&journal.backup_app, &journal.current_app).map_err(|error| {
        format!("could not restore prior app after failed health check: {error}")
    })?;
    let _ = fs::remove_file(&journal.health_file);
    journal.state = NativeUpdateState::RolledBack;
    journal.write(journal_path)
}

#[cfg(target_os = "macos")]
pub fn run_update_helper(
    journal_path: impl AsRef<Path>,
    old_pid: u32,
    health_timeout: Duration,
) -> Result<NativeUpdateState, String> {
    let journal_path = journal_path.as_ref();
    let mut journal = NativeUpdateJournal::read(journal_path)?;
    wait_for_process_exit(old_pid, Duration::from_secs(30))?;
    replace_staged_app(&mut journal, journal_path)?;
    if let Err(error) = launch_app(
        &journal.current_app,
        Some((journal_path, &journal.health_token)),
    ) {
        finish_or_rollback_update(&mut journal, journal_path, false)?;
        launch_app(&journal.current_app, None)?;
        return Err(format!(
            "updated KanVibe did not launch; restored the prior app: {error}"
        ));
    }

    let deadline = Instant::now() + health_timeout;
    let healthy = loop {
        if let Ok(token) = fs::read_to_string(&journal.health_file)
            && token == journal.health_token
        {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        thread::sleep(Duration::from_millis(200));
    };
    finish_or_rollback_update(&mut journal, journal_path, healthy)?;
    if !healthy {
        launch_app(&journal.current_app, None)?;
    }
    Ok(journal.state)
}

#[cfg(target_os = "macos")]
fn wait_for_process_exit(pid: u32, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        let output = Command::new("/bin/ps")
            .args(["-p", &pid.to_string(), "-o", "pid="])
            .output()
            .map_err(|error| format!("could not inspect running KanVibe process: {error}"))?;
        if output.stdout.iter().all(u8::is_ascii_whitespace) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("running KanVibe process did not exit before update timeout".to_owned());
        }
        thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(target_os = "macos")]
fn launch_app(app: &Path, health: Option<(&Path, &str)>) -> Result<(), String> {
    let mut command = Command::new("/usr/bin/open");
    command.arg("-n").arg(app);
    if let Some((journal, token)) = health {
        command
            .arg("--args")
            .arg("--native-update-health-journal")
            .arg(journal)
            .arg("--native-update-health-token")
            .arg(token);
    }
    let status = command
        .status()
        .map_err(|error| format!("could not launch updated KanVibe app: {error}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("launching updated KanVibe failed with {status}"))
}

fn is_release_version(value: &str) -> bool {
    let mut parts = value.split('.');
    let valid = (0..3).all(|_| {
        parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    });
    valid && parts.next().is_none()
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "kanvibe-native-updater-{label}-{}",
            std::process::id()
        ))
    }

    fn journal(root: &Path) -> NativeUpdateJournal {
        NativeUpdateJournal::new(
            root.join(APP_BUNDLE_NAME),
            "1.2.3",
            "0123456789abcdef0123456789abcdef",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .expect("valid journal")
    }

    #[test]
    fn update_journal_confines_generated_paths_to_installed_app_parent() {
        let root = test_root("paths");
        let journal = journal(&root);
        assert_eq!(
            journal.staged_app,
            root.join(".KanVibe.update-0123456789abcdef0123456789abcdef.app")
        );
        assert_eq!(
            journal.backup_app,
            root.join(".KanVibe.rollback-0123456789abcdef0123456789abcdef.app")
        );
        assert!(
            NativeUpdateJournal::new(
                root.join("Other.app"),
                "1.2.3",
                &journal.nonce,
                &journal.health_token
            )
            .is_err()
        );
        assert!(
            NativeUpdateJournal::new(
                root.join(APP_BUNDLE_NAME),
                "1.2.3-beta",
                &journal.nonce,
                &journal.health_token
            )
            .is_err()
        );
    }

    #[test]
    fn replacement_commits_only_after_matching_health_acknowledgement() {
        let root = test_root("commit");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(APP_BUNDLE_NAME)).expect("current app");
        let mut journal = journal(&root);
        fs::create_dir_all(&journal.staged_app).expect("staged app");
        let journal_path = root.join("update.json");
        journal.write(&journal_path).expect("write journal");

        replace_staged_app(&mut journal, &journal_path).expect("replace app");
        assert_eq!(journal.state, NativeUpdateState::AwaitingHealth);
        assert!(journal.backup_app.is_dir());
        assert!(
            !acknowledge_update_health(&journal_path, "wrong", "1.2.3").expect("reject health")
        );
        assert!(
            acknowledge_update_health(&journal_path, &journal.health_token, "1.2.3")
                .expect("ack health")
        );
        finish_or_rollback_update(&mut journal, &journal_path, true).expect("commit update");
        assert_eq!(journal.state, NativeUpdateState::Committed);
        assert!(!journal.backup_app.exists());
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn missing_health_acknowledgement_restores_previous_app() {
        let root = test_root("rollback");
        let _ = fs::remove_dir_all(&root);
        let current = root.join(APP_BUNDLE_NAME);
        fs::create_dir_all(&current).expect("current app");
        fs::write(current.join("version"), "old").expect("old marker");
        let mut journal = journal(&root);
        fs::create_dir_all(&journal.staged_app).expect("staged app");
        fs::write(journal.staged_app.join("version"), "new").expect("new marker");
        let journal_path = root.join("update.json");
        journal.write(&journal_path).expect("write journal");

        replace_staged_app(&mut journal, &journal_path).expect("replace app");
        finish_or_rollback_update(&mut journal, &journal_path, false).expect("rollback update");
        assert_eq!(journal.state, NativeUpdateState::RolledBack);
        assert_eq!(
            fs::read_to_string(current.join("version")).expect("restored marker"),
            "old"
        );
        fs::remove_dir_all(&root).expect("cleanup");
    }
}
