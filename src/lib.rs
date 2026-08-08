use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

pub const CONFIG_VERSION: u32 = 1;
pub const DEFAULT_LOG_RETENTION: usize = 256;
pub const MAX_LOG_RETENTION: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    InvalidLine(String),
    UnknownKey(String),
    DuplicateKey(String),
    InvalidValue(String),
    MissingValue(String),
    UnknownDependency { service: String, dependency: String },
    DependencyCycle(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLine(line) => write!(formatter, "invalid config line: {line}"),
            Self::UnknownKey(key) => write!(formatter, "unknown config key: {key}"),
            Self::DuplicateKey(key) => write!(formatter, "duplicate config key: {key}"),
            Self::InvalidValue(value) => write!(formatter, "invalid config value: {value}"),
            Self::MissingValue(key) => write!(formatter, "missing config value: {key}"),
            Self::UnknownDependency {
                service,
                dependency,
            } => write!(
                formatter,
                "service {service} depends on unknown service {dependency}"
            ),
            Self::DependencyCycle(service) => {
                write!(formatter, "service dependency cycle reaches {service}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryMode {
    Halt,
    Shell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    Never,
    OnFailure,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    Spawn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceConfig {
    pub command: String,
    pub args: Vec<String>,
    pub workdir: String,
    pub dependencies: Vec<String>,
    pub restart: RestartPolicy,
    pub max_restarts: u32,
    pub timeout_ms: u64,
    pub readiness: Readiness,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggingConfig {
    pub level: LogLevel,
    pub retention: usize,
    pub serial_fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitConfig {
    pub version: u32,
    pub recovery: RecoveryMode,
    pub environment: BTreeMap<String, String>,
    pub services: BTreeMap<String, ServiceConfig>,
    pub logging: LoggingConfig,
}

pub fn parse_config(source: &str) -> Result<InitConfig, ConfigError> {
    let mut section = String::from("init");
    let mut seen = BTreeSet::new();
    let mut version = None;
    let mut recovery = None;
    let mut environment = BTreeMap::new();
    let mut logging = LoggingConfig {
        level: LogLevel::Notice,
        retention: DEFAULT_LOG_RETENTION,
        serial_fallback: true,
    };
    let mut services = BTreeMap::<String, ServiceConfig>::new();

    for raw_line in source.lines() {
        let line = raw_line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            if !line.ends_with(']') {
                return Err(ConfigError::InvalidLine(line.to_owned()));
            }
            section = line[1..line.len() - 1].trim().to_owned();
            if section != "init" && section != "logging" && section != "env" {
                let name = section
                    .strip_prefix("service.")
                    .ok_or_else(|| ConfigError::InvalidLine(line.to_owned()))?;
                validate_name(name)?;
                services
                    .entry(name.to_owned())
                    .or_insert_with(default_service);
            }
            continue;
        }

        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| ConfigError::InvalidLine(line.to_owned()))?;
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() {
            return Err(ConfigError::InvalidLine(line.to_owned()));
        }
        let full_key = format!("{section}.{key}");
        if !seen.insert(full_key) {
            return Err(ConfigError::DuplicateKey(key.to_owned()));
        }

        match section.as_str() {
            "init" => match key {
                "version" => version = Some(parse_u32(value)?),
                "recovery" => {
                    recovery = Some(match parse_string(value)?.as_str() {
                        "halt" => RecoveryMode::Halt,
                        "shell" => RecoveryMode::Shell,
                        other => {
                            return Err(ConfigError::InvalidValue(format!("recovery={other}")))
                        }
                    })
                }
                _ => return Err(ConfigError::UnknownKey(key.to_owned())),
            },
            "logging" => match key {
                "level" => {
                    logging.level = parse_log_level(&parse_string(value)?)?;
                }
                "retention" => {
                    let retention = parse_u64(value)? as usize;
                    if retention == 0 || retention > MAX_LOG_RETENTION {
                        return Err(ConfigError::InvalidValue(format!("retention={retention}")));
                    }
                    logging.retention = retention;
                }
                "serial_fallback" => logging.serial_fallback = parse_bool(value)?,
                _ => return Err(ConfigError::UnknownKey(key.to_owned())),
            },
            "env" => {
                validate_env_name(key)?;
                environment.insert(key.to_owned(), parse_string(value)?);
            }
            _ => {
                let service = section.strip_prefix("service.").ok_or_else(|| {
                    ConfigError::InvalidLine(format!("unknown section [{section}]"))
                })?;
                let config = services
                    .get_mut(service)
                    .ok_or_else(|| ConfigError::InvalidLine(section.clone()))?;
                match key {
                    "command" => config.command = parse_string(value)?,
                    "args" => config.args = parse_array(value)?,
                    "workdir" => config.workdir = parse_string(value)?,
                    "dependencies" => config.dependencies = parse_array(value)?,
                    "restart" => {
                        config.restart = match parse_string(value)?.as_str() {
                            "never" => RestartPolicy::Never,
                            "on-failure" => RestartPolicy::OnFailure,
                            "always" => RestartPolicy::Always,
                            other => {
                                return Err(ConfigError::InvalidValue(format!(
                                    "{service}.restart={other}"
                                )))
                            }
                        }
                    }
                    "max_restarts" => config.max_restarts = parse_u32(value)?,
                    "timeout_ms" => config.timeout_ms = parse_u64(value)?,
                    "readiness" => {
                        if parse_string(value)? != "spawn" {
                            return Err(ConfigError::InvalidValue(format!("{service}.readiness")));
                        }
                        config.readiness = Readiness::Spawn;
                    }
                    "capabilities" => config.capabilities = parse_array(value)?,
                    _ => return Err(ConfigError::UnknownKey(format!("{service}.{key}"))),
                }
            }
        }
    }

    let version = version.ok_or_else(|| ConfigError::MissingValue("init.version".into()))?;
    if version != CONFIG_VERSION {
        return Err(ConfigError::InvalidValue(format!("init.version={version}")));
    }
    let recovery = recovery.unwrap_or(RecoveryMode::Halt);
    for (name, service) in &services {
        if service.command.is_empty() {
            return Err(ConfigError::MissingValue(format!("{name}.command")));
        }
        for dependency in &service.dependencies {
            if !services.contains_key(dependency) {
                return Err(ConfigError::UnknownDependency {
                    service: name.clone(),
                    dependency: dependency.clone(),
                });
            }
        }
    }
    detect_cycles(&services)?;

    Ok(InitConfig {
        version,
        recovery,
        environment,
        services,
        logging,
    })
}

fn default_service() -> ServiceConfig {
    ServiceConfig {
        command: String::new(),
        args: Vec::new(),
        workdir: String::from("/"),
        dependencies: Vec::new(),
        restart: RestartPolicy::Never,
        max_restarts: 3,
        timeout_ms: 0,
        readiness: Readiness::Spawn,
        capabilities: Vec::new(),
    }
}

fn validate_name(name: &str) -> Result<(), ConfigError> {
    let mut chars = name.bytes();
    if !chars.next().is_some_and(|byte| byte.is_ascii_lowercase()) {
        return Err(ConfigError::InvalidValue(format!("service name {name}")));
    }
    if chars.any(|byte| {
        !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_')
    }) {
        return Err(ConfigError::InvalidValue(format!("service name {name}")));
    }
    Ok(())
}

fn validate_env_name(name: &str) -> Result<(), ConfigError> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(ConfigError::InvalidValue(format!("env name {name}")));
    }
    Ok(())
}

fn parse_string(value: &str) -> Result<String, ConfigError> {
    if value.len() < 2 || !value.starts_with('"') || !value.ends_with('"') {
        return Err(ConfigError::InvalidValue(value.to_owned()));
    }
    let body = &value[1..value.len() - 1];
    if body.contains('\\') {
        return Err(ConfigError::InvalidValue(value.to_owned()));
    }
    Ok(body.to_owned())
}

fn parse_array(value: &str) -> Result<Vec<String>, ConfigError> {
    if !value.starts_with('[') || !value.ends_with(']') {
        return Err(ConfigError::InvalidValue(value.to_owned()));
    }
    let body = value[1..value.len() - 1].trim();
    if body.is_empty() {
        return Ok(Vec::new());
    }
    body.split(',')
        .map(|item| parse_string(item.trim()))
        .collect()
}

fn parse_u32(value: &str) -> Result<u32, ConfigError> {
    value
        .parse()
        .map_err(|_| ConfigError::InvalidValue(value.to_owned()))
}

fn parse_u64(value: &str) -> Result<u64, ConfigError> {
    value
        .parse()
        .map_err(|_| ConfigError::InvalidValue(value.to_owned()))
}

fn parse_bool(value: &str) -> Result<bool, ConfigError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(ConfigError::InvalidValue(value.to_owned())),
    }
}

fn parse_log_level(value: &str) -> Result<LogLevel, ConfigError> {
    match value {
        "error" => Ok(LogLevel::Error),
        "warn" => Ok(LogLevel::Warn),
        "notice" => Ok(LogLevel::Notice),
        "debug" => Ok(LogLevel::Debug),
        _ => Err(ConfigError::InvalidValue(format!("logging.level={value}"))),
    }
}

fn detect_cycles(services: &BTreeMap<String, ServiceConfig>) -> Result<(), ConfigError> {
    fn visit(
        name: &str,
        services: &BTreeMap<String, ServiceConfig>,
        active: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
    ) -> Result<(), ConfigError> {
        if done.contains(name) {
            return Ok(());
        }
        if !active.insert(name.to_owned()) {
            return Err(ConfigError::DependencyCycle(name.to_owned()));
        }
        for dependency in &services[name].dependencies {
            visit(dependency, services, active, done)?;
        }
        active.remove(name);
        done.insert(name.to_owned());
        Ok(())
    }

    let mut active = BTreeSet::new();
    let mut done = BTreeSet::new();
    for name in services.keys() {
        visit(name, services, &mut active, &mut done)?;
    }
    Ok(())
}

pub type ChildHandle = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Hangup,
    Terminate,
    Kill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildExit {
    pub code: Option<i32>,
    pub signal: Option<u32>,
}

impl ChildExit {
    pub const fn code(code: i32) -> Self {
        Self {
            code: Some(code),
            signal: None,
        }
    }

    pub const fn signal(signal: u32) -> Self {
        Self {
            code: None,
            signal: Some(signal),
        }
    }

    pub const fn success() -> Self {
        Self::code(0)
    }

    pub const fn is_success(self) -> bool {
        matches!(self.code, Some(0)) && self.signal.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnError {
    Unavailable,
    Invalid(String),
}

impl fmt::Display for SpawnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("process backend unavailable"),
            Self::Invalid(value) => write!(formatter, "invalid process request: {value}"),
        }
    }
}

impl std::error::Error for SpawnError {}

pub trait ProcessBackend {
    fn spawn(&mut self, name: &str, config: &ServiceConfig) -> Result<ChildHandle, SpawnError>;
    fn signal(&mut self, child: ChildHandle, signal: Signal) -> Result<(), SpawnError>;
    fn poll(&mut self) -> Vec<(ChildHandle, ChildExit)>;

    fn reap_orphans(&mut self) -> u32 {
        0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    Defined,
    Starting,
    Ready,
    Running,
    Backoff,
    Stopped,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownMode {
    Shutdown,
    Reboot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Error,
    Warn,
    Notice,
    Debug,
}

impl LogLevel {
    fn allowed(self, configured: Self) -> bool {
        self <= configured
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRecord {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub service: String,
    pub level: LogLevel,
    pub event: String,
    pub detail: String,
}

impl LogRecord {
    pub fn stable_line(&self) -> String {
        format!(
            "{:08} {:012} {:?} {} {} {}",
            self.sequence, self.timestamp_ms, self.level, self.service, self.event, self.detail
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitCause {
    pub code: Option<i32>,
    pub signal: Option<u32>,
}

#[derive(Debug, Clone)]
struct ServiceRuntime {
    state: ServiceState,
    child: Option<ChildHandle>,
    started_at_ms: u64,
    restart_count: u32,
    next_start_ms: u64,
    last_exit: Option<ExitCause>,
}

impl Default for ServiceRuntime {
    fn default() -> Self {
        Self {
            state: ServiceState::Defined,
            child: None,
            started_at_ms: 0,
            restart_count: 0,
            next_start_ms: 0,
            last_exit: None,
        }
    }
}

pub struct Supervisor {
    config: InitConfig,
    services: BTreeMap<String, ServiceRuntime>,
    logs: VecDeque<LogRecord>,
    serial: VecDeque<String>,
    sequence: u64,
    now_ms: u64,
    log_level: LogLevel,
    retention: usize,
    serial_fallback: bool,
    shutdown: Option<ShutdownMode>,
}

impl Supervisor {
    pub fn new(config: InitConfig) -> Self {
        let services = config
            .services
            .keys()
            .map(|name| (name.clone(), ServiceRuntime::default()))
            .collect();
        Self {
            log_level: config.logging.level,
            retention: config.logging.retention,
            serial_fallback: config.logging.serial_fallback,
            config,
            services,
            logs: VecDeque::new(),
            serial: VecDeque::new(),
            sequence: 0,
            now_ms: 0,
            shutdown: None,
        }
    }

    pub fn state(&self, name: &str) -> Option<ServiceState> {
        self.services.get(name).map(|runtime| runtime.state)
    }

    pub fn states(&self) -> BTreeMap<String, ServiceState> {
        self.services
            .iter()
            .map(|(name, runtime)| (name.clone(), runtime.state))
            .collect()
    }

    pub fn last_exit(&self, name: &str) -> Option<ExitCause> {
        self.services
            .get(name)
            .and_then(|runtime| runtime.last_exit)
    }

    pub fn logs(&self) -> impl ExactSizeIterator<Item = &LogRecord> {
        self.logs.iter()
    }

    pub fn serial_lines(&self) -> impl ExactSizeIterator<Item = &String> {
        self.serial.iter()
    }

    pub const fn shutdown_mode(&self) -> Option<ShutdownMode> {
        self.shutdown
    }

    pub const fn log_level(&self) -> LogLevel {
        self.log_level
    }

    pub fn set_log_level(&mut self, level: LogLevel) {
        self.log_level = level;
        self.record("init", LogLevel::Notice, "log-level", format!("{level:?}"));
    }

    pub fn tick<B: ProcessBackend>(
        &mut self,
        backend: &mut B,
        now_ms: u64,
    ) -> Result<(), SpawnError> {
        self.now_ms = now_ms;
        let exits = backend.poll();
        for (child, exit) in exits {
            let Some(name) = self
                .services
                .iter()
                .find_map(|(name, runtime)| (runtime.child == Some(child)).then_some(name.clone()))
            else {
                self.record(
                    "init",
                    LogLevel::Debug,
                    "orphan-exit",
                    format!(
                        "child={child} code={:?} signal={:?}",
                        exit.code, exit.signal
                    ),
                );
                continue;
            };

            let event = {
                let runtime = self.services.get_mut(&name).expect("service was found");
                runtime.child = None;
                runtime.last_exit = Some(ExitCause {
                    code: exit.code,
                    signal: exit.signal,
                });
                let failed = !exit.is_success();
                let should_restart = match self.config.services[&name].restart {
                    RestartPolicy::Never => false,
                    RestartPolicy::OnFailure => failed,
                    RestartPolicy::Always => true,
                };
                let event = if should_restart
                    && runtime.restart_count < self.config.services[&name].max_restarts
                {
                    runtime.restart_count += 1;
                    runtime.next_start_ms = now_ms + backoff_ms(runtime.restart_count);
                    runtime.state = ServiceState::Backoff;
                    Some((
                        LogLevel::Warn,
                        format!(
                            "exit=code:{:?},signal:{:?};restart={} at={}",
                            exit.code, exit.signal, runtime.restart_count, runtime.next_start_ms
                        ),
                    ))
                } else if should_restart {
                    runtime.state = ServiceState::Failed;
                    Some((
                        LogLevel::Error,
                        format!(
                            "exit=code:{:?},signal:{:?};restart-limit={}",
                            exit.code, exit.signal, self.config.services[&name].max_restarts
                        ),
                    ))
                } else {
                    runtime.state = ServiceState::Stopped;
                    Some((
                        if failed {
                            LogLevel::Warn
                        } else {
                            LogLevel::Notice
                        },
                        format!("exit=code:{:?},signal:{:?}", exit.code, exit.signal),
                    ))
                };
                event
            };
            if let Some((level, detail)) = event {
                self.record(&name, level, "exit", detail);
            }
        }

        let orphans = backend.reap_orphans();
        if orphans != 0 {
            self.record(
                "init",
                LogLevel::Notice,
                "reap",
                format!("orphans={orphans}"),
            );
        }

        if self.shutdown.is_some() {
            return Ok(());
        }

        let names: Vec<String> = self.services.keys().cloned().collect();
        for name in names {
            if !self.can_start(&name) {
                if self.dependencies_failed(&name) {
                    let changed = self.services[&name].state != ServiceState::Blocked;
                    self.services.get_mut(&name).expect("service exists").state =
                        ServiceState::Blocked;
                    if changed {
                        self.record(
                            &name,
                            LogLevel::Error,
                            "blocked",
                            "dependency failed".to_owned(),
                        );
                    }
                }
                continue;
            }
            if self.services[&name].child.is_some()
                || matches!(
                    self.services[&name].state,
                    ServiceState::Failed | ServiceState::Stopped | ServiceState::Blocked
                )
                || now_ms < self.services[&name].next_start_ms
            {
                continue;
            }

            let config = self.config.services[&name].clone();
            let spawn_result = backend.spawn(&name, &config);
            let event = match spawn_result {
                Ok(child) => {
                    let runtime = self.services.get_mut(&name).expect("service exists");
                    runtime.child = Some(child);
                    runtime.started_at_ms = now_ms;
                    runtime.state = match config.readiness {
                        Readiness::Spawn => ServiceState::Running,
                    };
                    Some((
                        LogLevel::Notice,
                        format!("child={child} state={:?}", runtime.state),
                    ))
                }
                Err(error) => {
                    let runtime = self.services.get_mut(&name).expect("service exists");
                    runtime.last_exit = None;
                    runtime.state = ServiceState::Failed;
                    Some((LogLevel::Error, error.to_string()))
                }
            };
            if let Some((level, detail)) = event {
                self.record(&name, level, "start", detail);
            }
        }

        let timed_out: Vec<(String, ChildHandle)> = self
            .services
            .iter()
            .filter_map(|(name, runtime)| {
                let timeout = self.config.services[name].timeout_ms;
                if timeout == 0 || now_ms.saturating_sub(runtime.started_at_ms) < timeout {
                    return None;
                }
                runtime.child.map(|child| (name.clone(), child))
            })
            .collect();
        for (name, child) in timed_out {
            backend.signal(child, Signal::Terminate)?;
            self.record(
                &name,
                LogLevel::Warn,
                "timeout",
                format!("child={child};signal=terminate"),
            );
        }
        Ok(())
    }

    pub fn stop<B: ProcessBackend>(
        &mut self,
        backend: &mut B,
        name: &str,
    ) -> Result<(), SpawnError> {
        let child = self
            .services
            .get(name)
            .ok_or_else(|| SpawnError::Invalid(format!("unknown service {name}")))?
            .child;
        if let Some(child) = child {
            backend.signal(child, Signal::Terminate)?;
        }
        self.services
            .get_mut(name)
            .expect("service was checked")
            .state = ServiceState::Stopped;
        self.record(name, LogLevel::Notice, "stop", "requested".to_owned());
        Ok(())
    }

    pub fn restart<B: ProcessBackend>(
        &mut self,
        backend: &mut B,
        name: &str,
    ) -> Result<(), SpawnError> {
        self.stop(backend, name)?;
        let runtime = self.services.get_mut(name).expect("service was checked");
        runtime.restart_count = 0;
        runtime.next_start_ms = self.now_ms;
        runtime.state = ServiceState::Defined;
        self.record(name, LogLevel::Notice, "restart", "queued".to_owned());
        Ok(())
    }

    pub fn forward_signal<B: ProcessBackend>(
        &mut self,
        backend: &mut B,
        signal: Signal,
    ) -> Result<(), SpawnError> {
        let children: Vec<(String, ChildHandle)> = self
            .services
            .iter()
            .filter_map(|(name, runtime)| runtime.child.map(|child| (name.clone(), child)))
            .collect();
        for (name, child) in children {
            backend.signal(child, signal)?;
            self.record(
                &name,
                LogLevel::Debug,
                "signal",
                format!("child={child};signal={signal:?}"),
            );
        }
        Ok(())
    }

    pub fn request_shutdown<B: ProcessBackend>(
        &mut self,
        backend: &mut B,
        mode: ShutdownMode,
    ) -> Result<(), SpawnError> {
        self.shutdown = Some(mode);
        self.forward_signal(backend, Signal::Terminate)?;
        self.record("init", LogLevel::Notice, "shutdown", format!("{mode:?}"));
        Ok(())
    }

    fn can_start(&self, name: &str) -> bool {
        self.config.services[name]
            .dependencies
            .iter()
            .all(|dependency| self.services[dependency].state == ServiceState::Running)
    }

    fn dependencies_failed(&self, name: &str) -> bool {
        self.config.services[name]
            .dependencies
            .iter()
            .any(|dependency| {
                matches!(
                    self.services[dependency].state,
                    ServiceState::Failed | ServiceState::Blocked
                )
            })
    }

    fn record(&mut self, service: &str, level: LogLevel, event: &str, detail: String) {
        if !level.allowed(self.log_level) {
            return;
        }
        self.sequence = self.sequence.saturating_add(1);
        let record = LogRecord {
            sequence: self.sequence,
            timestamp_ms: self.now_ms,
            service: service.to_owned(),
            level,
            event: event.to_owned(),
            detail,
        };
        let line = record.stable_line();
        self.logs.push_back(record);
        while self.logs.len() > self.retention {
            self.logs.pop_front();
        }
        if self.serial_fallback {
            self.serial.push_back(line);
            while self.serial.len() > self.retention {
                self.serial.pop_front();
            }
        }
    }
}

pub fn backoff_ms(attempt: u32) -> u64 {
    100_u64.saturating_mul(1_u64 << attempt.min(6))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Status,
    Logs { service: Option<String> },
    Start(String),
    Stop(String),
    Restart(String),
    LogLevel(LogLevel),
    Shutdown,
    Reboot,
}

pub fn parse_command(line: &str) -> Result<Command, &'static str> {
    let mut parts = line.split_whitespace();
    let command = parts.next().ok_or("empty command")?;
    let parsed = match command {
        "status" => Command::Status,
        "logs" => Command::Logs {
            service: parts.next().map(str::to_owned),
        },
        "start" => Command::Start(parts.next().ok_or("start needs a service")?.to_owned()),
        "stop" => Command::Stop(parts.next().ok_or("stop needs a service")?.to_owned()),
        "restart" => Command::Restart(parts.next().ok_or("restart needs a service")?.to_owned()),
        "log-level" => Command::LogLevel(match parts.next().ok_or("log-level needs a value")? {
            "error" => LogLevel::Error,
            "warn" => LogLevel::Warn,
            "notice" => LogLevel::Notice,
            "debug" => LogLevel::Debug,
            _ => return Err("unknown log level"),
        }),
        "shutdown" => Command::Shutdown,
        "reboot" => Command::Reboot,
        _ => return Err("unknown command"),
    };
    if parts.next().is_some() {
        return Err("too many command arguments");
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
        [init]
        version = 1
        recovery = "shell"

        [logging]
        level = "debug"
        retention = 3
        serial_fallback = true

        [env]
        PATH = "/bin"

        [service.logger]
        command = "/bin/logger"
        args = ["--boot"]
        workdir = "/"
        restart = "always"
        max_restarts = 2
        timeout_ms = 100

        [service.shell]
        command = "/bin/sh"
        dependencies = ["logger"]
        restart = "never"
    "#;

    #[test]
    fn parses_strict_config_and_defaults() {
        let config = parse_config(CONFIG).expect("config should parse");
        assert_eq!(config.version, 1);
        assert_eq!(config.recovery, RecoveryMode::Shell);
        assert_eq!(config.environment["PATH"], "/bin");
        assert_eq!(config.services["shell"].dependencies, vec!["logger"]);
        assert_eq!(config.services["shell"].readiness, Readiness::Spawn);
    }

    #[test]
    fn rejects_dependency_cycles() {
        let source = r#"
            [init]
            version = 1
            [service.a]
            command = "/bin/a"
            dependencies = ["b"]
            [service.b]
            command = "/bin/b"
            dependencies = ["a"]
        "#;
        assert!(matches!(
            parse_config(source),
            Err(ConfigError::DependencyCycle(_))
        ));
    }

    #[derive(Default)]
    struct FakeBackend {
        next_child: ChildHandle,
        spawned: Vec<String>,
        pending: VecDeque<(ChildHandle, ChildExit)>,
        signals: Vec<(ChildHandle, Signal)>,
    }

    impl ProcessBackend for FakeBackend {
        fn spawn(
            &mut self,
            name: &str,
            _config: &ServiceConfig,
        ) -> Result<ChildHandle, SpawnError> {
            self.next_child += 1;
            self.spawned.push(name.to_owned());
            Ok(self.next_child)
        }

        fn signal(&mut self, child: ChildHandle, signal: Signal) -> Result<(), SpawnError> {
            self.signals.push((child, signal));
            Ok(())
        }

        fn poll(&mut self) -> Vec<(ChildHandle, ChildExit)> {
            self.pending.drain(..).collect()
        }
    }

    #[test]
    fn starts_dependencies_in_stable_order() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            [service.app]
            command = "/bin/app"
            dependencies = ["logger"]
            [service.logger]
            command = "/bin/logger"
        "#,
        )
        .expect("config should parse");
        let mut supervisor = Supervisor::new(config);
        let mut backend = FakeBackend::default();
        supervisor.tick(&mut backend, 10).expect("tick should work");
        assert_eq!(backend.spawned, vec!["logger"]);
        supervisor.tick(&mut backend, 20).expect("tick should work");
        assert_eq!(backend.spawned, vec!["logger", "app"]);
        assert_eq!(supervisor.state("app"), Some(ServiceState::Running));
    }

    #[test]
    fn applies_backoff_and_crash_limit() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            [service.app]
            command = "/bin/app"
            restart = "on-failure"
            max_restarts = 1
        "#,
        )
        .expect("config should parse");
        let mut supervisor = Supervisor::new(config);
        let mut backend = FakeBackend::default();
        supervisor.tick(&mut backend, 0).expect("tick should work");
        backend.pending.push_back((1, ChildExit::code(1)));
        supervisor.tick(&mut backend, 1).expect("tick should work");
        assert_eq!(supervisor.state("app"), Some(ServiceState::Backoff));
        assert_eq!(backend.spawned, vec!["app"]);
        supervisor
            .tick(&mut backend, 201)
            .expect("tick should work");
        assert_eq!(backend.spawned, vec!["app", "app"]);
        backend.pending.push_back((2, ChildExit::signal(9)));
        supervisor
            .tick(&mut backend, 202)
            .expect("tick should work");
        assert_eq!(supervisor.state("app"), Some(ServiceState::Failed));
    }

    #[test]
    fn signals_timed_out_child_and_bounds_logs() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            [logging]
            level = "debug"
            retention = 2
            [service.app]
            command = "/bin/app"
            timeout_ms = 10
        "#,
        )
        .expect("config should parse");
        let mut supervisor = Supervisor::new(config);
        let mut backend = FakeBackend::default();
        supervisor.tick(&mut backend, 0).expect("tick should work");
        supervisor.tick(&mut backend, 10).expect("tick should work");
        assert_eq!(backend.signals, vec![(1, Signal::Terminate)]);
        assert_eq!(supervisor.logs().len(), 2);
        assert_eq!(supervisor.serial_lines().len(), 2);
    }

    #[test]
    fn parses_cli_commands() {
        assert_eq!(parse_command("status"), Ok(Command::Status));
        assert_eq!(
            parse_command("logs app"),
            Ok(Command::Logs {
                service: Some("app".into())
            })
        );
        assert_eq!(
            parse_command("log-level warn"),
            Ok(Command::LogLevel(LogLevel::Warn))
        );
        assert!(parse_command("stop").is_err());
        assert!(parse_command("status extra").is_err());
    }
}
