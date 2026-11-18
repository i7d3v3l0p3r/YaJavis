use axum::{
    Json, Router,
    body::Body,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, ErrorKind, Read, Seek},
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

mod adb;
mod discovery;

const REPO: &str = match option_env!("JARVIS_REPO") {
    Some(s) => s,
    None => "newfpv/YaJavis",
};
const VERSION: &str = match option_env!("JARVIS_VERSION") {
    Some(s) => s,
    None => "dev",
};
const STAGE: &str = "/tmp/jarvis-setup-stage";
const AUTOSTART_MANUAL: &str = "jarvis-autostart-manual";
const MAX_PAYLOAD: usize = 40 << 20;
const INSTALL_RESERVE_KIB: u64 = 1024;
const MODELS: [&str; 3] = ["wake.bin", "oww.bin", "wake_oww.bin"];
const REQUIRED: &[&str] = &[
    "jarvisd",
    "jarvis-supervisor",
    "jarvisd.sh",
    "S98jarvisd",
    "wake.bin",
    "oww.bin",
    "wake_oww.bin",
    "vqe_tap-armhf.so",
    "vqe_tap-arm64.so",
];
type App = Arc<Setup>;

struct Setup {
    token: String,
    address: SocketAddr,
    job: Mutex<Job>,
    adb: tokio::sync::OnceCell<PathBuf>,
    scanning: tokio::sync::Mutex<()>,
}

#[derive(Default, Serialize)]
struct Job {
    lines: Vec<String>,
    done: bool,
    error: String,
    warning: String,
    url: String,
    #[serde(skip)]
    busy: bool,
}

#[derive(Default, Serialize)]
struct Speaker {
    ip: String,
    model: String,
    arch: String,
    userland: String,
    installed: String,
    root: bool,
    android: bool,
    free_mb: u64,
    staging_mb: u64,
    ram_mb: u64,
    need_mb: Option<u64>,
    space_ok: Option<bool>,
    problems: Vec<String>,
    #[serde(skip)]
    storage: Storage,
}

#[derive(Default)]
struct Storage {
    flash_kib: u64,
    previous_kib: u64,
    temp_kib: u64,
    present_models: [bool; MODELS.len()],
}

struct Footprint {
    flash_kib: u64,
    temp_kib: u64,
}

impl Footprint {
    fn from_files(files: &BTreeMap<&str, u64>, userland: &str, present_models: &[bool; MODELS.len()]) -> Self {
        let tap = format!("vqe_tap-{userland}.so");
        let mut result = Self { flash_kib: INSTALL_RESERVE_KIB, temp_kib: INSTALL_RESERVE_KIB };
        for (&name, &bytes) in files {
            let kib = bytes.div_ceil(4096) * 4;
            result.temp_kib += kib;
            let kept = MODELS.iter().position(|model| *model == name).is_some_and(|i| present_models[i]);
            let unused_tap = matches!(name, "vqe_tap-armhf.so" | "vqe_tap-arm64.so") && name != tap;
            if !unused_tap && !kept {
                result.flash_kib += kib;
            }
        }
        result
    }
}

impl Speaker {
    fn check_space(&mut self, files: &BTreeMap<&str, u64>) -> Result<(), String> {
        let needed = Footprint::from_files(files, &self.userland, &self.storage.present_models);
        self.need_mb = Some(needed.flash_kib.div_ceil(1024));
        let flash_ok = self.storage.flash_kib + self.storage.previous_kib >= needed.flash_kib;
        let temp_ok = self.storage.temp_kib >= needed.temp_kib;
        self.space_ok = Some(flash_ok && temp_ok);
        if !flash_ok {
            return Err(format!("На колонке нужно {} МиБ для этого пакета", needed.flash_kib.div_ceil(1024)));
        }
        if !temp_ok {
            return Err(format!("Для распаковки на колонке нужно {} МиБ в /tmp", needed.temp_kib.div_ceil(1024)));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct Address {
    ip: Ipv4Addr,
}

#[derive(Deserialize)]
struct Install {
    ip: Ipv4Addr,
    #[serde(default)]
    password: String,
    #[serde(default)]
    timezone: String,
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("jarvis-setup-{}", token()?));
        std::fs::create_dir(&path).map_err(|e| e.to_string())?;
        Ok(Self(path))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let port = std::env::var("JARVIS_SETUP_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
    let address = listener.local_addr()?;
    let app = Arc::new(Setup {
        token: token()?,
        address,
        job: Mutex::default(),
        adb: tokio::sync::OnceCell::new(),
        scanning: tokio::sync::Mutex::new(()),
    });
    let api = Router::new()
        .route("/api/scan", get(scan))
        .route("/api/check", post(check))
        .route("/api/job", get(job))
        .route("/api/install", post(install))
        .route("/api/uninstall", post(uninstall))
        .route_layer(middleware::from_fn_with_state(app.clone(), authorize));
    let router = Router::new()
        .merge(api)
        .route("/token.js", get(token_js))
        .fallback(get(ui))
        .layer(middleware::from_fn_with_state(app.clone(), loopback))
        .with_state(app);
    let url = format!("http://{address}/");
    println!("Установщик Джарвиса {VERSION}: {url}");
    if std::env::var_os("JARVIS_SETUP_NOBROWSER").is_none() {
        let (cmd, args) = if cfg!(windows) {
            (
                "rundll32",
                vec!["url.dll,FileProtocolHandler", url.as_str()],
            )
        } else if cfg!(target_os = "macos") {
            ("open", vec![url.as_str()])
        } else {
            ("xdg-open", vec![url.as_str()])
        };
        let _ = std::process::Command::new(cmd).args(args).spawn();
    }
    axum::serve(listener, router).await?;
    Ok(())
}

async fn authorize(State(app): State<App>, request: Request, next: Next) -> Response {
    if request
        .headers()
        .get("X-Token")
        .and_then(|v| v.to_str().ok())
        != Some(app.token.as_str())
    {
        return reply(
            StatusCode::FORBIDDEN,
            json!({"error":"Доступ только из установщика"}),
        );
    }
    next.run(request).await
}

async fn loopback(State(app): State<App>, request: Request, next: Next) -> Response {
    if request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        != Some(app.address.to_string().as_str())
    {
        return reply(
            StatusCode::FORBIDDEN,
            json!({"error":"Откройте адрес из окна установщика"}),
        );
    }
    let mut res = next.run(request).await;
    res.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    res.headers_mut().insert(header::CONTENT_SECURITY_POLICY, header::HeaderValue::from_static("default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'"));
    res
}

fn token() -> Result<String, String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(hex::encode(bytes))
}

async fn token_js(State(app): State<App>) -> Response {
    let text = format!(
        "window.TOKEN={};window.VERSION={};",
        json!(app.token),
        json!(VERSION)
    );
    Response::builder()
        .header(header::CONTENT_TYPE, "text/javascript; charset=utf-8")
        .body(Body::from(text))
        .unwrap()
}

async fn ui(request: Request) -> Response {
    macro_rules! asset {
        ($name:literal) => {
            include_bytes!(concat!("../../../installer/ui/", $name)) as &[u8]
        };
    }
    let (mime, content) = match request.uri().path() {
        "/" | "/index.html" => ("text/html; charset=utf-8", asset!("index.html")),
        "/setup.js" => ("text/javascript; charset=utf-8", asset!("setup.js")),
        "/setup.css" => ("text/css; charset=utf-8", asset!("setup.css")),
        "/manrope-cyrillic.woff2" => ("font/woff2", asset!("manrope-cyrillic.woff2")),
        "/manrope-latin.woff2" => ("font/woff2", asset!("manrope-latin.woff2")),
        "/screenshots/dashboard.png" => ("image/png", asset!("screenshots/dashboard.png")),
        "/screenshots/connections.png" => ("image/png", asset!("screenshots/connections.png")),
        "/screenshots/assistant.png" => ("image/png", asset!("screenshots/assistant.png")),
        "/screenshots/charts.png" => ("image/png", asset!("screenshots/charts.png")),
        _ => return Response::builder().status(404).body(Body::empty()).unwrap(),
    };
    Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .body(Body::from(content))
        .unwrap()
}

fn reply(status: StatusCode, value: Value) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(value.to_string()))
        .unwrap()
}
fn bad(message: impl Into<String>) -> Response {
    reply(StatusCode::BAD_REQUEST, json!({"error":message.into()}))
}

impl Setup {
    async fn adb(&self, args: &[&str], seconds: u64) -> Result<String, String> {
        let path = self.adb.get_or_try_init(adb::prepare).await?;
        let mut cmd = tokio::process::Command::new(path);
        let result = tokio::time::timeout(
            Duration::from_secs(seconds),
            cmd.args(args).kill_on_drop(true).output(),
        )
        .await
        .map_err(|_| "ADB не ответил вовремя")?
        .map_err(|error| format!("Не удалось запустить ADB: {error}"))?;
        if !result.status.success() {
            return Err(format!(
                "Команда ADB завершилась с ошибкой {}",
                result.status
            ));
        }
        Ok(String::from_utf8_lossy(&result.stdout).trim().to_owned())
    }

    async fn shell(&self, ip: Ipv4Addr, script: &str) -> Result<String, String> {
        self.adb(&["-s", &format!("{ip}:5555"), "shell", script], 40)
            .await
    }

    async fn inspect(&self, ip: Ipv4Addr) -> Result<Speaker, String> {
        if !ip.is_private() {
            return Err("Нужен IP вашей колонки в домашней сети".into());
        }
        self.adb(&["connect", &format!("{ip}:5555")], 10).await?;
        let info = self.shell(ip, r#"id -u; uname -m; head -c 160 /proc/device-tree/model 2>/dev/null | tr -d '\000\n'; printf '\n'; df -k /data 2>/dev/null | awk 'END{print $4}'; awk '/MemTotal/{print $2}' /proc/meminfo; od -An -tu1 -N20 /system/vendor/quasar/maind 2>/dev/null | tr '\n' ' '; printf '\n'; if test -d /etc/init.d && command -v aplay >/dev/null && command -v start-stop-daemon >/dev/null; then echo ready; else echo missing; fi
d=/data/opt/jarvisd
if [ -f "$d/jarvisd.prev-rust" ]; then du -k "$d/jarvisd.prev-rust" 2>/dev/null | awk '{print $1}'; else echo 0; fi
df -k /tmp 2>/dev/null | awk 'END{print $4}'
for model in wake.bin oww.bin wake_oww.bin; do if [ -e "$d/$model" ]; then echo 1; else echo 0; fi; done"#).await?;
        let fields: Vec<_> = info.lines().collect();
        let field = |i: usize| fields.get(i).map_or("", |s| s.trim());
        let root = field(0) == "0";
        let arch = match field(1) {
            "aarch64" | "arm64" => "arm64",
            "armv7l" | "armv8l" => "armv7",
            _ => "",
        };
        let bytes: Vec<_> = field(5)
            .split_whitespace()
            .map_while(|n| n.parse::<u8>().ok())
            .collect();
        let userland = if bytes.len() >= 20 && &bytes[..4] == b"\x7fELF" && bytes[5] == 1 {
            match (bytes[4], u16::from_le_bytes([bytes[18], bytes[19]])) {
                (1, 40) => "armhf",
                (2, 183) => "arm64",
                _ => "",
            }
        } else {
            ""
        };
        let installed = self
            .shell(
                ip,
                "test ! -x /data/opt/jarvisd/jarvisd || /data/opt/jarvisd/jarvisd version",
            )
            .await
            .unwrap_or_default();
        let free_kb = field(3).parse::<u64>().unwrap_or(0);
        let rollback_kb = field(7).parse::<u64>().unwrap_or(0);
        let mut sp = Speaker {
            ip: ip.to_string(),
            model: field(2).into(),
            arch: arch.into(),
            userland: userland.into(),
            installed,
            root,
            free_mb: free_kb / 1024,
            staging_mb: (free_kb + rollback_kb) / 1024,
            ram_mb: field(4).parse::<u64>().unwrap_or(0) / 1024,
            storage: Storage {
                flash_kib: free_kb,
                previous_kib: rollback_kb,
                temp_kib: field(8).parse().unwrap_or_default(),
                present_models: std::array::from_fn(|i| field(9 + i) == "1"),
            },
            ..Default::default()
        };
        let profile = self
            .shell(ip, "cat /system/vendor/quasar/launch.json 2>/dev/null")
            .await
            .unwrap_or_default();
        if !root {
            sp.problems
                .push("На выбранной колонке нужны разрешённый ADB и root".into());
        }
        if arch.is_empty()
            || userland.is_empty()
            || field(6) != "ready"
            || !profile.contains("--platformd")
        {
            sp.problems.push(
                "Не подтверждены Linux-прошивка, профиль maind или аудиоинтерфейс колонки".into(),
            );
        }
        if sp.problems.is_empty()
            && let Some(dir) = payload_dir().0
            && let Ok(file) = std::fs::File::open(dir.join(format!("station-{}.zip", sp.arch)))
            && let Ok(files) = package_sizes(file)
            && let Err(error) = sp.check_space(&files)
        {
            sp.problems.push(error);
        }
        Ok(sp)
    }

    fn start(
        self: &Arc<Self>,
        work: impl std::future::Future<Output = Result<String, String>> + Send + 'static,
    ) -> Response {
        let mut job = self.job.lock().unwrap();
        if job.busy {
            return bad("Уже выполняется другая операция");
        }
        *job = Job {
            busy: true,
            ..Default::default()
        };
        let app = self.clone();
        tokio::spawn(async move {
            let result = work.await;
            let mut job = app.job.lock().unwrap();
            job.busy = false;
            job.done = true;
            match result {
                Ok(url) => job.url = url,
                Err(error) => job.error = error,
            }
        });
        reply(StatusCode::OK, json!({"ok":true}))
    }

    fn log(&self, text: &str) {
        self.job.lock().unwrap().lines.push(text.into());
    }
    async fn push(&self, ip: Ipv4Addr, path: &Path, address: &str) -> Result<(), String> {
        let path = path.to_str().ok_or("Недопустимый путь к файлу")?;
        self.adb(&["-s", &format!("{ip}:5555"), "push", path, address], 120)
            .await?;
        Ok(())
    }
}

async fn scan(State(app): State<App>) -> Response {
    let Ok(_scanning) = app.scanning.try_lock() else {
        return bad("Поиск колонок уже идёт");
    };
    let devices = match app.adb(&["devices"], 10).await {
        Ok(d) => d,
        Err(e) => return bad(e),
    };
    let mut already = std::collections::BTreeSet::new();
    let mut candidates = Vec::new();
    for line in devices.lines().skip(1) {
        let mut fields = line.split_whitespace();
        let Some(ip) = fields
            .next()
            .and_then(|s| s.strip_suffix(":5555"))
            .and_then(|s| s.parse::<Ipv4Addr>().ok())
        else {
            continue;
        };
        already.insert(ip);
        if fields.next() == Some("device") {
            candidates.push((ip, false));
        }
    }
    let nearby = discovery::nearby(&already).await;
    candidates.extend(nearby.into_iter().take(16).map(|ip| (ip, true)));
    let mut probes = tokio::task::JoinSet::new();
    for (ip, new) in candidates {
        let app = app.clone();
        probes.spawn(async move {
            let found = tokio::time::timeout(Duration::from_secs(9), app.inspect(ip)).await
                .ok().and_then(Result::ok).filter(|speaker| !new || !speaker.userland.is_empty());
            if new && found.is_none() {
                let _ = app.adb(&["disconnect", &format!("{ip}:5555")], 2).await;
            }
            found
        });
    }
    let mut speakers = Vec::new();
    while let Some(probe) = probes.join_next().await {
        if let Ok(Some(speaker)) = probe { speakers.push(speaker); }
    }
    speakers.sort_by(|a, b| a.ip.cmp(&b.ip));
    reply(StatusCode::OK, json!({"speakers":speakers}))
}
async fn check(State(app): State<App>, Json(new): Json<Address>) -> Response {
    match app.inspect(new.ip).await {
        Ok(s) => reply(StatusCode::OK, json!(s)),
        Err(e) => bad(e),
    }
}
async fn job(State(app): State<App>) -> Response {
    reply(StatusCode::OK, json!(*app.job.lock().unwrap()))
}
async fn install(State(app): State<App>, Json(input): Json<Install>) -> Response {
    let work = app.clone();
    app.start(async move { install_on(&work, input).await })
}
async fn uninstall(State(app): State<App>, Json(input): Json<Address>) -> Response {
    let work = app.clone();
    app.start(async move {
        let speaker = work.inspect(input.ip).await?;
        if !speaker.root || speaker.installed.is_empty() {
            return Err("Джарвис на колонке не найден".into());
        }
        work.shell(
            input.ip,
            "start-stop-daemon -S -b -x /bin/sh -- /data/opt/jarvisd/jarvisd.sh uninstall",
        )
        .await?;
        Ok(String::new())
    })
}

fn verify_payload(bytes: &[u8], sums: &str, name: &str) -> Result<(), String> {
    let wanted = jarvis_station::checksum(sums, name)
        .ok_or_else(|| format!("В SHA256SUMS нет контрольной суммы для {name}"))?;
    if format!("{:x}", Sha256::digest(bytes)) != wanted {
        return Err(format!("Контрольная сумма {name} не совпала"));
    }
    Ok(())
}

fn local_payload(dir: &Path, name: &str, require_checksum: bool) -> Result<Option<Vec<u8>>, String> {
    let path = dir.join(name);
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Не удалось открыть {name}: {error}")),
    };
    let mut bytes = Vec::new();
    file.take(MAX_PAYLOAD as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Не удалось прочитать {name}: {error}"))?;
    if bytes.len() > MAX_PAYLOAD {
        return Err(format!("{name} больше допустимого размера"));
    }
    match std::fs::read_to_string(dir.join("SHA256SUMS")) {
        Ok(sums) => verify_payload(&bytes, &sums, name)?,
        Err(error) if !require_checksum && error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Не удалось прочитать SHA256SUMS рядом с {name}: {error}")),
    }
    Ok(Some(bytes))
}

fn payload_dir() -> (Option<PathBuf>, bool) {
    match std::env::var_os("JARVIS_SETUP_PAYLOAD") {
        Some(path) => (Some(PathBuf::from(path)), true),
        None => (
            std::env::current_exe().ok().and_then(|path| path.parent().map(Path::to_path_buf)),
            false,
        ),
    }
}

async fn payload(app: &Setup, arch: &str) -> Result<Vec<u8>, String> {
    let name = format!("station-{arch}.zip");
    let (dir, explicit) = payload_dir();
    if let Some(dir) = dir {
        let local_name = name.clone();
        let local = tokio::task::spawn_blocking(move || local_payload(&dir, &local_name, !explicit))
            .await
            .map_err(|error| error.to_string())??;
        if let Some(bytes) = local {
            app.log(&format!("Использую локальную прошивку {name}"));
            return Ok(bytes);
        }
        if explicit {
            return Err(format!("В указанной папке нет {name}"));
        }
    }
    app.log("Скачиваю прошивку с GitHub…");
    let url = if let Some(url) = option_env!("JARVIS_SETUP_ASSETS") {
        url.to_owned()
    } else if VERSION == "dev" {
        format!("https://github.com/{REPO}/releases/latest/download")
    } else {
        format!("https://github.com/{REPO}/releases/download/v{VERSION}")
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(100))
        .build()
        .map_err(|e| e.to_string())?;
    let mut response = client
        .get(format!("{url}/{name}"))
        .send()
        .await
        .map_err(|e| format!("Не удалось скачать {name}: {e}"))?
        .error_for_status()
        .map_err(|_| "Пакет для вашей колонки недоступен")?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Скачивание {name} с GitHub оборвалось: {e}"))?
    {
        if bytes.len() + chunk.len() > MAX_PAYLOAD {
            return Err("Пакет больше допустимого размера".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let sums = client
        .get(format!("{url}/SHA256SUMS"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|_| "Контрольные суммы пакета недоступны")?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    verify_payload(&bytes, &sums, &name)?;
    Ok(bytes)
}

#[cfg(test)]
mod payload_tests {
    use super::*;

    #[test]
    fn offline_bundle_uses_only_the_verified_local_archive() {
        let scratch = Scratch::new().unwrap();
        let dir = &scratch.0;
        let name = "station-arm64.zip";
        assert!(local_payload(dir, name, true).unwrap().is_none());
        let bytes = b"test payload";
        std::fs::write(dir.join(name), bytes).unwrap();
        assert!(local_payload(dir, name, true).is_err());
        assert_eq!(local_payload(dir, name, false).unwrap().unwrap(), bytes);
        let digest = format!("{:x}", Sha256::digest(bytes));
        std::fs::write(dir.join("SHA256SUMS"), format!("{digest}  {name}\n")).unwrap();
        assert_eq!(local_payload(dir, name, true).unwrap().unwrap(), bytes);
        std::fs::write(dir.join(name), b"corrupt").unwrap();
        assert!(local_payload(dir, name, true).is_err());
    }

    #[test]
    fn footprint_distinguishes_flash_ram_and_existing_models() {
        let files = BTreeMap::from([
            ("jarvisd", 6_039_752),
            ("jarvis-supervisor", 441_992),
            ("vqe_tap-arm64.so", 5_600),
            ("vqe_tap-armhf.so", 4_648),
            ("spotcmd", 6_908),
            ("wake.bin", 175_678),
            ("oww.bin", 1_348_172),
            ("wake_oww.bin", 627_900),
            ("jarvisd.sh", 2_750),
            ("S98jarvisd", 355),
        ]);
        let clean = Footprint::from_files(&files, "armhf", &[false; 3]);
        let update = Footprint::from_files(&files, "armhf", &[true; 3]);
        assert_eq!(clean.flash_kib.div_ceil(1024), 10);
        assert_eq!(update.flash_kib.div_ceil(1024), 8);
        assert_eq!(clean.temp_kib, update.temp_kib);
        let mut speaker = Speaker {
            userland: "armhf".into(),
            storage: Storage { flash_kib: clean.flash_kib, temp_kib: clean.temp_kib - 1, ..Default::default() },
            ..Default::default()
        };
        assert!(speaker.check_space(&files).unwrap_err().contains("/tmp"));
        speaker.storage.temp_kib = clean.temp_kib;
        assert!(speaker.check_space(&files).is_ok());
    }
}

fn package_entries<R: Read + Seek>(
    reader: R,
    mut visit: impl FnMut(&'static str, zip::read::ZipFile<'_, R>) -> Result<(), String>,
) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| e.to_string())?;
    for name in REQUIRED.iter().copied().chain(std::iter::once("spotcmd")) {
        let entry = match zip.by_name(name) {
            Ok(e) => e,
            Err(zip::result::ZipError::FileNotFound) if name == "spotcmd" => continue,
            Err(_) => return Err(format!("В пакете отсутствует {name}")),
        };
        if !entry.is_file() || entry.is_symlink() || entry.size() > jarvis_station::MAX_BINARY {
            return Err(format!("Некорректный файл {name}"));
        }
        visit(name, entry)?;
    }
    Ok(())
}

fn package_sizes<R: Read + Seek>(reader: R) -> Result<BTreeMap<&'static str, u64>, String> {
    let mut sizes = BTreeMap::new();
    package_entries(reader, |name, entry| {
        sizes.insert(name, entry.size());
        Ok(())
    })?;
    Ok(sizes)
}

fn unpack(bytes: Vec<u8>, dir: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut sums = BTreeMap::new();
    package_entries(Cursor::new(bytes), |name, mut entry| {
        let mut data = Vec::new();
        entry
            .by_ref()
            .take(jarvis_station::MAX_BINARY + 1)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        if data.len() as u64 > jarvis_station::MAX_BINARY {
            return Err(format!("Слишком большой файл {name}"));
        }
        let path = dir.join(name);
        std::fs::write(&path, &data).map_err(|e| e.to_string())?;
        sums.insert(name.to_string(), format!("{:x}", Sha256::digest(&data)));
        Ok(())
    })?;
    Ok(sums)
}

async fn install_on(app: &Setup, input: Install) -> Result<String, String> {
    let ip = input.ip;
    app.log("Подключаюсь к выбранной колонке…");
    let mut sp = app.inspect(ip).await?;
    if !sp.problems.is_empty() {
        return Err(sp.problems.join(". "));
    }
    if (sp.installed.is_empty() || !input.password.is_empty())
        && !(6..=72).contains(&input.password.len())
    {
        return Err("Пароль панели должен содержать от 6 до 72 байт".into());
    }
    app.log(&format!("Определена архитектура колонки: {}", sp.arch));
    let bytes = payload(app, &sp.arch).await?;
    let files = package_sizes(Cursor::new(&bytes))?;
    sp.check_space(&files)?;
    if let Some(need) = sp.need_mb {
        app.log(&format!("Проверено место на колонке. Пакету нужно до {need} МиБ на флеше"));
    }
    let temp = Scratch::new()?;
    let sums = unpack(bytes, &temp.0)?;
    let stage = format!("{STAGE}-{}", token()?);
    app.shell(ip, &format!("umask 077; mkdir {stage}")).await?;
    let result = stage_install(app, input, &sp, &stage, &temp.0, &sums).await;
    let _ = app.shell(ip, &format!("rm -rf {stage}")).await;
    result
}

async fn stage_install(
    app: &Setup,
    input: Install,
    sp: &Speaker,
    stage: &str,
    temp: &Path,
    sums: &BTreeMap<String, String>,
) -> Result<String, String> {
    let ip = input.ip;
    for (name, digest) in sums {
        app.log(&format!("Копирую {name}"));
        app.push(ip, &temp.join(name), &format!("{stage}/{name}"))
            .await?;
        let actual = app.shell(ip, &format!("sha256sum {stage}/{name}")).await?;
        if actual.split_whitespace().next() != Some(digest.as_str()) {
            return Err(format!("Файл {name} повреждён при передаче"));
        }
    }
    app.shell(
        ip,
        &format!("chmod 700 {stage}/jarvisd {stage}/jarvis-supervisor"),
    )
    .await?;
    let image = app.shell(ip, &format!("{stage}/jarvisd core-info")).await?;
    let image: jarvis_station::Image =
        serde_json::from_str(&image).map_err(|_| "Колонка не запускает Rust-сборку")?;
    if image.format != jarvis_station::IDENTITY || image.arch != sp.arch {
        return Err("Архитектура сборки не совпадает".into());
    }
    let mut required = std::collections::BTreeSet::new();
    if !sp.installed.is_empty() {
        let manifests = app.shell(ip, r#"printf '['; comma=; for f in /data/opt/jarvisd/plugins/*/plugin.json; do if [ -f "$f" ]; then printf '%s' "$comma"; cat "$f"; comma=,; fi; done; printf ']'"#).await?;
        let manifests: Vec<Value> = serde_json::from_str(&manifests)
            .map_err(|_| "Не удалось проверить установленные модули")?;
        required.extend(
            manifests
                .iter()
                .filter(|m| m["kind"] == "module")
                .filter_map(|m| m["id"].as_str().map(str::to_owned)),
        );
    }
    let shipped: std::collections::BTreeSet<_> = image.modules.iter().cloned().collect();
    if !required.is_subset(&shipped) || (!sp.installed.is_empty() && required != shipped) {
        return Err("Для этого пакета сначала удалите прежнюю версию. Затем поставьте нужные плагины в панели.".into());
    }
    for id in &image.modules {
        if !(2..=32).contains(&id.len())
            || !id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err("Неверный идентификатор модуля в пакете".into());
        }
        let manifest = app
            .shell(ip, &format!("{stage}/jarvisd module-manifest {id}"))
            .await?;
        let filename = format!("module-{id}.json");
        let path = temp.join(&filename);
        tokio::fs::write(&path, manifest)
            .await
            .map_err(|e| e.to_string())?;
        app.push(ip, &path, &format!("{stage}/{filename}")).await?;
    }
    let password_hash = if input.password.is_empty() {
        String::new()
    } else {
        bcrypt::hash(input.password, 10).map_err(|e| e.to_string())?
    };
    let seed = temp.join("seed.json");
    jarvis_station::atomic(&seed, json!({"password_hash":password_hash,"timezone":input.timezone}).to_string().as_bytes()).map_err(|e| e.to_string())?;
    app.push(ip, &seed, &format!("{stage}/seed.json")).await?;
    app.shell(
        ip,
        &format!(
            "chmod 700 {stage}/jarvisd {stage}/jarvis-supervisor; chmod 600 {stage}/seed.json"
        ),
    )
    .await?;
    app.log("Добавляю автозапуск и проверяю, что панель поднимается…");
    let output = app
        .shell(
            ip,
            &install_script(stage, &sp.userland, &sums["jarvisd"], &image.version)?,
        )
        .await?;
    let manual_autostart = output.lines().any(|line| line.trim() == AUTOSTART_MANUAL);
    if manual_autostart {
        app.log("Скрипт для ручной настройки автозапуска сохранён в /data/opt/jarvisd/S98jarvisd");
    }
    app.log("Проверяю запуск панели…");
    let url = format!("http://{ip}:8080/");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .no_proxy()
        .build()
        .map_err(|e| e.to_string())?;
    for _ in 0..35 {
        if let Ok(response) = client.get(format!("{url}healthz")).send().await
            && let Ok(status) = response.json::<Value>().await
            && status["version"] == image.version
            && status["ready"] == true
            && app
                .shell(
                    ip,
                    concat!(
                        "test ! -e /data/opt/jarvisd/update-rust.json",
                        " && test ! -e /data/opt/jarvisd/update-rust.error",
                    ),
                )
                .await
                .is_ok()
        {
            if manual_autostart {
                app.job.lock().unwrap().warning = concat!(
                    "Автозапуск не удалось записать в прошивку. ",
                    "Настройте запуск /data/opt/jarvisd/S98jarvisd start до S99quasar ",
                    "и перезагрузите колонку.",
                )
                .into();
            }
            app.log("Готово. Панель Джарвиса отвечает");
            return Ok(url);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err("Панель новой сборки не ответила. Служба вернёт предыдущую версию, если она была установлена".into())
}

fn install_script(
    stage: &str,
    userland: &str,
    digest: &str,
    version: &str,
) -> Result<String, String> {
    if !matches!(userland, "armhf" | "arm64")
        || digest.len() != 64
        || !digest.bytes().all(|b| b.is_ascii_hexdigit())
        || !version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
        || version.is_empty()
    {
        return Err("Повреждён пакет установки".into());
    }
    let pending = json!({"version":version,"sha256":digest});
    Ok(format!(
        r#"set -eu
d=/data/opt/jarvisd; s={stage}
mkdir -p "$d"
test ! -e "$d/update-rust.json"
rm -f "$d/jarvisd.prev-rust"
cp "$s/jarvisd" "$d/jarvisd.next-rust"
chmod 755 "$d/jarvisd.next-rust"
cp "$s/jarvis-supervisor" "$d/jarvis-supervisor.new"
chmod 755 "$d/jarvis-supervisor.new"
for name in wake.bin oww.bin wake_oww.bin; do test -e "$d/$name" || cp "$s/$name" "$d/$name"; done
cp "$s/vqe_tap-{userland}.so" "$d/vqe_tap.so.new"
if [ -f "$s/spotcmd" ]; then cp "$s/spotcmd" "$d/spotcmd"; chmod 755 "$d/spotcmd"; fi
init=/etc/init.d/S98jarvisd
manual=0
if [ ! -f "$init" ]; then
    if mount -o remount,rw /; then
        tmp=/etc/init.d/.S98jarvisd.$$
        if [ -d "$init" ] || ! (cp "$s/S98jarvisd" "$tmp" && chmod 755 "$tmp" && mv -f "$tmp" "$init"); then
            rm -f "$tmp" || true
            manual=1
        fi
        mount -o remount,ro /
    else
        manual=1
    fi
fi
if [ "$manual" = 1 ]; then
    cp "$s/S98jarvisd" "$d/S98jarvisd"
    chmod 755 "$d/S98jarvisd"
fi
if [ -x "$d/jarvisd.sh" ]; then "$d/jarvisd.sh" stop; sleep 2; fi
for manifest in "$s"/module-*.json; do
    if [ -f "$manifest" ]; then
        id=${{manifest##*/module-}}
        id=${{id%.json}}
        [ ! -e "$d/plugins/$id/plugin.json" ] || continue
        mkdir -p "$d/plugins/$id"
        cp "$manifest" "$d/plugins/$id/plugin.json"
    fi
done
cp "$s/jarvisd.sh" "$d/jarvisd.sh"
chmod 755 "$d/jarvisd.sh"
mv "$d/jarvis-supervisor.new" "$d/jarvis-supervisor"
mv "$d/vqe_tap.so.new" "$d/vqe_tap.so"
cp "$s/seed.json" "$d/seed.json"
chmod 600 "$d/seed.json"
rm -f "$s/seed.json" "$d/disabled" "$d/update-rust.error"
printf '%s' '{pending}' > "$d/update-rust.tmp"
mv "$d/update-rust.tmp" "$d/update-rust.json"
sync
"$d/jarvisd.sh" boot
if [ "$manual" = 1 ]; then
    printf '%s\n' '{AUTOSTART_MANUAL}'
fi"#
    ))
}
