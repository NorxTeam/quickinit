use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

pub const CONFIG_VERSION: u32 = 1;
pub const DEFAULT_LOG_RETENTION: usize = 256;
pub const MAX_LOG_RETENTION: usize = 4096;
pub const DEFAULT_SHUTDOWN_TIMEOUT_MS: u64 = 5_000;
pub const MAX_SHUTDOWN_TIMEOUT_MS: u64 = 3_600_000;
pub const MAX_SERVICE_TIMEOUT_MS: u64 = 3_600_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    InvalidLine(String),
    UnknownKey(String),
    DuplicateKey(String),
    InvalidValue(String),
    MissingValue(String),
    UnknownService { context: String, service: String },
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
            Self::UnknownService { context, service } => {
                write!(formatter, "{context} references unknown service {service}")
            }
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
pub enum Signal {
    Hangup,
    Terminate,
    Kill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    Spawn,
    Notify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServiceMode {
    Foreground,
    #[default]
    Background,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServiceKind {
    #[default]
    Service,
    Daemon,
    Oneshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeoutAction {
    Restart,
    Fail,
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopReason {
    Manual,
    Timeout,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StreamSpec {
    #[default]
    Inherit,
    Null,
    Serial,
    File(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResourceLimits {
    pub cpu_time_ms: Option<u64>,
    pub memory_bytes: Option<u64>,
    pub open_fds: Option<u32>,
    pub child_processes: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownTimeoutAction {
    Kill,
    Halt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownConfig {
    pub signal: Signal,
    pub timeout_ms: u64,
    pub on_timeout: ShutdownTimeoutAction,
}

impl Default for ShutdownConfig {
    fn default() -> Self {
        Self {
            signal: Signal::Terminate,
            timeout_ms: DEFAULT_SHUTDOWN_TIMEOUT_MS,
            on_timeout: ShutdownTimeoutAction::Kill,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceConfig {
    pub command: String,
    pub args: Vec<String>,
    pub workdir: String,
    pub mode: ServiceMode,
    pub kind: ServiceKind,
    pub autostart: bool,
    pub stdin: StreamSpec,
    pub stdout: StreamSpec,
    pub stderr: StreamSpec,
    pub environment: BTreeMap<String, String>,
    pub dependencies: Vec<String>,
    pub order: u32,
    pub restart: RestartPolicy,
    pub max_restarts: u32,
    pub timeout_ms: u64,
    pub stop_timeout_ms: u64,
    pub stop_signal: Signal,
    pub kill_signal: Signal,
    pub timeout_action: TimeoutAction,
    pub readiness: Readiness,
    pub readiness_timeout_ms: u64,
    pub capabilities: Vec<String>,
    pub limits: ResourceLimits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageFailureAction {
    Serial,
    Continue,
    Halt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggingConfig {
    pub level: LogLevel,
    pub retention: usize,
    pub serial_fallback: bool,
    pub on_storage_unavailable: StorageFailureAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BootConfig {
    pub shell: Option<String>,
    pub recovery_shell: Option<String>,
    pub mounts: Vec<String>,
    pub devices: Vec<String>,
    pub logging: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitConfig {
    pub version: u32,
    pub recovery: RecoveryMode,
    pub environment: BTreeMap<String, String>,
    pub services: BTreeMap<String, ServiceConfig>,
    pub logging: LoggingConfig,
    pub boot: BootConfig,
    pub shutdown: ShutdownConfig,
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
        on_storage_unavailable: StorageFailureAction::Serial,
    };
    let mut shutdown = ShutdownConfig::default();
    let mut boot = BootConfig::default();
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
            if section != "init"
                && section != "logging"
                && section != "env"
                && section != "boot"
                && section != "shutdown"
            {
                let suffix = section
                    .strip_prefix("service.")
                    .ok_or_else(|| ConfigError::InvalidLine(line.to_owned()))?;
                let mut parts = suffix.split('.');
                let name = parts
                    .next()
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| ConfigError::InvalidLine(line.to_owned()))?;
                validate_name(name)?;
                if let Some(subsection) = parts.next() {
                    if subsection != "env" && subsection != "limits" || parts.next().is_some() {
                        return Err(ConfigError::InvalidLine(line.to_owned()));
                    }
                }
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
                "on_storage_unavailable" => {
                    logging.on_storage_unavailable = match parse_string(value)?.as_str() {
                        "serial" => StorageFailureAction::Serial,
                        "continue" => StorageFailureAction::Continue,
                        "halt" => StorageFailureAction::Halt,
                        other => {
                            return Err(ConfigError::InvalidValue(format!(
                                "logging.on_storage_unavailable={other}"
                            )))
                        }
                    };
                }
                _ => return Err(ConfigError::UnknownKey(key.to_owned())),
            },
            "boot" => match key {
                "shell" => boot.shell = Some(parse_string(value)?),
                "recovery_shell" => boot.recovery_shell = Some(parse_string(value)?),
                "mounts" => boot.mounts = parse_array(value)?,
                "devices" => boot.devices = parse_array(value)?,
                "logging" => boot.logging = parse_array(value)?,
                _ => return Err(ConfigError::UnknownKey(key.to_owned())),
            },
            "env" => {
                validate_env_name(key)?;
                environment.insert(key.to_owned(), parse_string(value)?);
            }
            "shutdown" => match key {
                "signal" => shutdown.signal = parse_signal(&parse_string(value)?)?,
                "timeout_ms" => {
                    shutdown.timeout_ms = parse_positive_bounded_u64(
                        value,
                        "shutdown.timeout_ms",
                        MAX_SHUTDOWN_TIMEOUT_MS,
                    )?;
                }
                "on_timeout" => {
                    shutdown.on_timeout = match parse_string(value)?.as_str() {
                        "kill" => ShutdownTimeoutAction::Kill,
                        "halt" => ShutdownTimeoutAction::Halt,
                        other => {
                            return Err(ConfigError::InvalidValue(format!(
                                "shutdown.on_timeout={other}"
                            )))
                        }
                    };
                }
                _ => return Err(ConfigError::UnknownKey(key.to_owned())),
            },
            _ => {
                let suffix = section.strip_prefix("service.").ok_or_else(|| {
                    ConfigError::InvalidLine(format!("unknown section [{section}]"))
                })?;
                let mut parts = suffix.split('.');
                let service = parts.next().ok_or_else(|| {
                    ConfigError::InvalidLine(format!("unknown section [{section}]"))
                })?;
                let subsection = parts.next();
                if parts.next().is_some() {
                    return Err(ConfigError::InvalidLine(format!(
                        "unknown section [{section}]"
                    )));
                }
                let config = services
                    .get_mut(service)
                    .ok_or_else(|| ConfigError::InvalidLine(section.clone()))?;
                match subsection {
                    None => match key {
                        "command" => config.command = parse_string(value)?,
                        "args" => config.args = parse_array(value)?,
                        "workdir" => config.workdir = parse_string(value)?,
                        "mode" => {
                            config.mode = match parse_string(value)?.as_str() {
                                "foreground" => ServiceMode::Foreground,
                                "background" => ServiceMode::Background,
                                other => {
                                    return Err(ConfigError::InvalidValue(format!(
                                        "{service}.mode={other}"
                                    )))
                                }
                            };
                        }
                        "kind" => {
                            config.kind = match parse_string(value)?.as_str() {
                                "service" => ServiceKind::Service,
                                "daemon" => ServiceKind::Daemon,
                                "oneshot" => ServiceKind::Oneshot,
                                other => {
                                    return Err(ConfigError::InvalidValue(format!(
                                        "{service}.kind={other}"
                                    )))
                                }
                            };
                        }
                        "autostart" => config.autostart = parse_bool(value)?,
                        "stdin" => config.stdin = parse_stream(value)?,
                        "stdout" => config.stdout = parse_stream(value)?,
                        "stderr" => config.stderr = parse_stream(value)?,
                        "dependencies" => config.dependencies = parse_array(value)?,
                        "order" => config.order = parse_u32(value)?,
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
                        "timeout_ms" => {
                            config.timeout_ms = parse_bounded_u64(
                                value,
                                &format!("{service}.timeout_ms"),
                                MAX_SERVICE_TIMEOUT_MS,
                            )?;
                        }
                        "stop_timeout_ms" => {
                            config.stop_timeout_ms = parse_positive_bounded_u64(
                                value,
                                &format!("{service}.stop_timeout_ms"),
                                MAX_SERVICE_TIMEOUT_MS,
                            )?;
                        }
                        "stop_signal" => config.stop_signal = parse_signal(&parse_string(value)?)?,
                        "kill_signal" => config.kill_signal = parse_signal(&parse_string(value)?)?,
                        "timeout_action" => {
                            config.timeout_action = match parse_string(value)?.as_str() {
                                "restart" => TimeoutAction::Restart,
                                "fail" => TimeoutAction::Fail,
                                "stop" => TimeoutAction::Stop,
                                other => {
                                    return Err(ConfigError::InvalidValue(format!(
                                        "{service}.timeout_action={other}"
                                    )))
                                }
                            };
                        }
                        "readiness" => {
                            config.readiness = match parse_string(value)?.as_str() {
                                "spawn" => Readiness::Spawn,
                                "notify" => Readiness::Notify,
                                other => {
                                    return Err(ConfigError::InvalidValue(format!(
                                        "{service}.readiness={other}"
                                    )))
                                }
                            };
                        }
                        "readiness_timeout_ms" => {
                            config.readiness_timeout_ms = parse_bounded_u64(
                                value,
                                &format!("{service}.readiness_timeout_ms"),
                                MAX_SERVICE_TIMEOUT_MS,
                            )?;
                        }
                        "capabilities" => config.capabilities = parse_array(value)?,
                        _ => return Err(ConfigError::UnknownKey(format!("{service}.{key}"))),
                    },
                    Some("env") => {
                        validate_env_name(key)?;
                        config
                            .environment
                            .insert(key.to_owned(), parse_string(value)?);
                    }
                    Some("limits") => match key {
                        "cpu_time_ms" => {
                            config.limits.cpu_time_ms = Some(parse_positive_u64(value, key)?)
                        }
                        "memory_bytes" => {
                            config.limits.memory_bytes = Some(parse_positive_u64(value, key)?)
                        }
                        "open_fds" => {
                            config.limits.open_fds = Some(parse_positive_u32(value, key)?)
                        }
                        "child_processes" => {
                            config.limits.child_processes = Some(parse_positive_u32(value, key)?)
                        }
                        _ => {
                            return Err(ConfigError::UnknownKey(format!("{service}.limits.{key}")))
                        }
                    },
                    Some(other) => {
                        return Err(ConfigError::InvalidLine(format!(
                            "unknown section [service.{service}.{other}]"
                        )))
                    }
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
        validate_path(&service.command, &format!("{name}.command"))?;
        validate_path(&service.workdir, &format!("{name}.workdir"))?;
        validate_capabilities(name, &service.capabilities)?;
        if service.kind == ServiceKind::Daemon && service.readiness != Readiness::Notify {
            return Err(ConfigError::InvalidValue(format!(
                "{name}.daemon requires readiness=notify"
            )));
        }
        if service.readiness == Readiness::Notify && service.readiness_timeout_ms == 0 {
            return Err(ConfigError::MissingValue(format!(
                "{name}.readiness_timeout_ms"
            )));
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
    for service in services.values_mut() {
        for (key, value) in &environment {
            service
                .environment
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }
    if let Some(shell) = &boot.shell {
        validate_boot_service(&services, "boot.shell", shell)?;
    }
    if let Some(recovery_shell) = &boot.recovery_shell {
        validate_boot_service(&services, "boot.recovery_shell", recovery_shell)?;
    }
    for (context, names) in [
        ("boot.mounts", &boot.mounts),
        ("boot.devices", &boot.devices),
        ("boot.logging", &boot.logging),
    ] {
        for name in names {
            validate_boot_service(&services, context, name)?;
        }
    }
    detect_cycles(&services)?;

    Ok(InitConfig {
        version,
        recovery,
        environment,
        services,
        logging,
        boot,
        shutdown,
    })
}

pub fn recovery_mode_for_source(source: &str) -> RecoveryMode {
    parse_config(source)
        .map(|config| config.recovery)
        .unwrap_or(RecoveryMode::Shell)
}

fn default_service() -> ServiceConfig {
    ServiceConfig {
        command: String::new(),
        args: Vec::new(),
        workdir: String::from("/"),
        mode: ServiceMode::Background,
        kind: ServiceKind::Service,
        autostart: true,
        stdin: StreamSpec::Inherit,
        stdout: StreamSpec::Inherit,
        stderr: StreamSpec::Inherit,
        environment: BTreeMap::new(),
        dependencies: Vec::new(),
        order: 0,
        restart: RestartPolicy::Never,
        max_restarts: 3,
        timeout_ms: 0,
        stop_timeout_ms: DEFAULT_SHUTDOWN_TIMEOUT_MS,
        stop_signal: Signal::Terminate,
        kill_signal: Signal::Kill,
        timeout_action: TimeoutAction::Restart,
        readiness: Readiness::Spawn,
        readiness_timeout_ms: 0,
        capabilities: Vec::new(),
        limits: ResourceLimits::default(),
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

fn validate_boot_service(
    services: &BTreeMap<String, ServiceConfig>,
    context: &str,
    name: &str,
) -> Result<(), ConfigError> {
    validate_name(name)?;
    if !services.contains_key(name) {
        return Err(ConfigError::UnknownService {
            context: context.to_owned(),
            service: name.to_owned(),
        });
    }
    Ok(())
}

fn validate_env_name(name: &str) -> Result<(), ConfigError> {
    let mut bytes = name.bytes();
    if !bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_uppercase() || byte == b'_')
        || bytes.any(|byte| !(byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'))
    {
        return Err(ConfigError::InvalidValue(format!("env name {name}")));
    }
    Ok(())
}

fn validate_path(path: &str, field: &str) -> Result<(), ConfigError> {
    if !path.starts_with('/') || path.contains('\0') || path.contains('\\') {
        return Err(ConfigError::InvalidValue(format!("{field}={path}")));
    }
    if path != "/"
        && path
            .split('/')
            .skip(1)
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(ConfigError::InvalidValue(format!("{field}={path}")));
    }
    Ok(())
}

fn validate_capabilities(service: &str, capabilities: &[String]) -> Result<(), ConfigError> {
    let mut seen = BTreeSet::new();
    for capability in capabilities {
        if !seen.insert(capability) {
            return Err(ConfigError::InvalidValue(format!(
                "{service}.capabilities duplicate={capability}"
            )));
        }
        if !matches!(
            capability.as_str(),
            "mount" | "raw-io" | "net-admin" | "net-raw" | "memory-map" | "device-admin"
        ) {
            return Err(ConfigError::InvalidValue(format!(
                "{service}.capabilities={capability}"
            )));
        }
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

fn parse_positive_u32(value: &str, field: &str) -> Result<u32, ConfigError> {
    let value = parse_u32(value)?;
    if value == 0 {
        return Err(ConfigError::InvalidValue(format!("{field}=0")));
    }
    Ok(value)
}

fn parse_positive_u64(value: &str, field: &str) -> Result<u64, ConfigError> {
    let value = parse_u64(value)?;
    if value == 0 {
        return Err(ConfigError::InvalidValue(format!("{field}=0")));
    }
    Ok(value)
}

fn parse_positive_bounded_u64(value: &str, field: &str, maximum: u64) -> Result<u64, ConfigError> {
    let value = parse_positive_u64(value, field)?;
    if value > maximum {
        return Err(ConfigError::InvalidValue(format!("{field}={value}")));
    }
    Ok(value)
}

fn parse_bounded_u64(value: &str, field: &str, maximum: u64) -> Result<u64, ConfigError> {
    let value = parse_u64(value)?;
    if value > maximum {
        return Err(ConfigError::InvalidValue(format!("{field}={value}")));
    }
    Ok(value)
}

fn parse_stream(value: &str) -> Result<StreamSpec, ConfigError> {
    let value = parse_string(value)?;
    match value.as_str() {
        "inherit" => Ok(StreamSpec::Inherit),
        "null" => Ok(StreamSpec::Null),
        "serial" => Ok(StreamSpec::Serial),
        value if value.starts_with("file:") => {
            let path = &value["file:".len()..];
            validate_path(path, "stream.file")?;
            Ok(StreamSpec::File(path.to_owned()))
        }
        _ => Err(ConfigError::InvalidValue(format!("stream={value}"))),
    }
}

fn parse_signal(value: &str) -> Result<Signal, ConfigError> {
    match value {
        "hangup" => Ok(Signal::Hangup),
        "terminate" => Ok(Signal::Terminate),
        "kill" => Ok(Signal::Kill),
        _ => Err(ConfigError::InvalidValue(format!(
            "shutdown.signal={value}"
        ))),
    }
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
    Stopping,
    Backoff,
    Stopped,
    Completed,
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
    stop_reason: Option<StopReason>,
    stop_deadline_ms: Option<u64>,
    stop_escalated: bool,
    restart_after_stop: bool,
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
            stop_reason: None,
            stop_deadline_ms: None,
            stop_escalated: false,
            restart_after_stop: false,
        }
    }
}

pub struct Supervisor {
    config: InitConfig,
    services: BTreeMap<String, ServiceRuntime>,
    manual_starts: BTreeSet<String>,
    logs: VecDeque<LogRecord>,
    serial: VecDeque<String>,
    sequence: u64,
    now_ms: u64,
    log_level: LogLevel,
    retention: usize,
    serial_fallback: bool,
    storage_available: bool,
    shutdown: Option<ShutdownMode>,
    shutdown_started_at_ms: u64,
    shutdown_escalated: bool,
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
            storage_available: true,
            config,
            services,
            manual_starts: BTreeSet::new(),
            logs: VecDeque::new(),
            serial: VecDeque::new(),
            sequence: 0,
            now_ms: 0,
            shutdown: None,
            shutdown_started_at_ms: 0,
            shutdown_escalated: false,
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

    pub const fn storage_available(&self) -> bool {
        self.storage_available
    }

    pub fn set_storage_available(&mut self, available: bool) {
        self.storage_available = available;
    }

    pub fn boot_ready(&self) -> bool {
        self.storage_available
            && self
                .config
                .boot
                .mounts
                .iter()
                .chain(&self.config.boot.devices)
                .chain(&self.config.boot.logging)
                .all(|name| {
                    matches!(
                        self.services[name].state,
                        ServiceState::Ready | ServiceState::Running | ServiceState::Completed
                    )
                })
    }

    pub fn start_recovery_shell(&mut self) -> Result<(), SpawnError> {
        let name =
            self.config.boot.recovery_shell.clone().ok_or_else(|| {
                SpawnError::Invalid("recovery shell is not configured".to_owned())
            })?;
        self.start(&name)
    }

    pub fn render_status(&self) -> String {
        if self.services.is_empty() {
            return "no services".to_owned();
        }
        self.services
            .iter()
            .map(|(name, runtime)| {
                let exit_code = runtime
                    .last_exit
                    .and_then(|exit| exit.code)
                    .map_or_else(|| "-".to_owned(), |code| code.to_string());
                let exit_signal = runtime
                    .last_exit
                    .and_then(|exit| exit.signal)
                    .map_or_else(|| "-".to_owned(), |signal| signal.to_string());
                format!(
                    "service={name} state={:?} exit_code={exit_code} exit_signal={exit_signal}",
                    runtime.state
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn render_logs(&self, service: Option<&str>) -> String {
        let lines = self
            .logs
            .iter()
            .filter(|record| service.is_none_or(|name| record.service == name))
            .map(LogRecord::stable_line)
            .collect::<Vec<_>>();
        if lines.is_empty() {
            "no logs".to_owned()
        } else {
            lines.join("\n")
        }
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

            let config = self.config.services[&name].clone();
            let mut queue_restart = false;
            let event = {
                let runtime = self.services.get_mut(&name).expect("service was found");
                runtime.child = None;
                runtime.last_exit = Some(ExitCause {
                    code: exit.code,
                    signal: exit.signal,
                });
                let stop_reason = runtime.stop_reason.take();
                let restart_after_stop = runtime.restart_after_stop;
                runtime.restart_after_stop = false;
                runtime.stop_deadline_ms = None;
                runtime.stop_escalated = false;
                let failed = !exit.is_success();
                if stop_reason == Some(StopReason::Manual) && restart_after_stop {
                    runtime.state = ServiceState::Defined;
                    runtime.next_start_ms = now_ms;
                    queue_restart = true;
                    Some((
                        LogLevel::Notice,
                        format!(
                            "stopped;restart queued;code={:?};signal={:?}",
                            exit.code, exit.signal
                        ),
                    ))
                } else if stop_reason == Some(StopReason::Manual) {
                    runtime.state = ServiceState::Stopped;
                    Some((
                        LogLevel::Notice,
                        format!("manual-stop;code={:?};signal={:?}", exit.code, exit.signal),
                    ))
                } else if stop_reason == Some(StopReason::Timeout)
                    && config.timeout_action == TimeoutAction::Stop
                {
                    runtime.state = ServiceState::Stopped;
                    Some((
                        LogLevel::Warn,
                        format!("timeout-stop;code={:?};signal={:?}", exit.code, exit.signal),
                    ))
                } else if stop_reason == Some(StopReason::Timeout)
                    && config.timeout_action == TimeoutAction::Fail
                {
                    runtime.state = ServiceState::Failed;
                    Some((
                        LogLevel::Error,
                        format!(
                            "timeout-failed;code={:?};signal={:?}",
                            exit.code, exit.signal
                        ),
                    ))
                } else if config.kind == ServiceKind::Oneshot && exit.is_success() {
                    runtime.state = ServiceState::Completed;
                    Some((LogLevel::Notice, "completed".to_owned()))
                } else {
                    let should_restart = match config.restart {
                        RestartPolicy::Never => false,
                        RestartPolicy::OnFailure => failed,
                        RestartPolicy::Always => true,
                    };
                    if should_restart && runtime.restart_count < config.max_restarts {
                        runtime.restart_count += 1;
                        runtime.next_start_ms = now_ms + backoff_ms(runtime.restart_count);
                        runtime.state = ServiceState::Backoff;
                        Some((
                            LogLevel::Warn,
                            format!(
                                "exit=code:{:?},signal:{:?};restart={} at={}",
                                exit.code,
                                exit.signal,
                                runtime.restart_count,
                                runtime.next_start_ms
                            ),
                        ))
                    } else if should_restart {
                        runtime.state = ServiceState::Failed;
                        Some((
                            LogLevel::Error,
                            format!(
                                "exit=code:{:?},signal:{:?};restart-limit={}",
                                exit.code, exit.signal, config.max_restarts
                            ),
                        ))
                    } else {
                        runtime.state = if failed {
                            ServiceState::Failed
                        } else {
                            ServiceState::Stopped
                        };
                        Some((
                            if failed {
                                LogLevel::Warn
                            } else {
                                LogLevel::Notice
                            },
                            format!("exit=code:{:?},signal:{:?}", exit.code, exit.signal),
                        ))
                    }
                }
            };
            if queue_restart {
                self.manual_starts.insert(name.clone());
            }
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
            if !self.shutdown_escalated
                && now_ms.saturating_sub(self.shutdown_started_at_ms)
                    >= self.config.shutdown.timeout_ms
            {
                self.shutdown_escalated = true;
                match self.config.shutdown.on_timeout {
                    ShutdownTimeoutAction::Kill => {
                        self.forward_signal(backend, Signal::Kill)?;
                        self.record(
                            "init",
                            LogLevel::Warn,
                            "shutdown-timeout",
                            "signal=kill".to_owned(),
                        );
                    }
                    ShutdownTimeoutAction::Halt => {
                        self.record(
                            "init",
                            LogLevel::Warn,
                            "shutdown-timeout",
                            "action=halt".to_owned(),
                        );
                    }
                }
            }
            return Ok(());
        }

        let mut names: Vec<String> = self.services.keys().cloned().collect();
        names.sort_by_key(|name| (self.config.services[name].order, name.clone()));
        for name in names {
            if self.config.boot.shell.as_deref() == Some(name.as_str()) && !self.boot_ready() {
                continue;
            }
            if !self.config.services[&name].autostart && !self.manual_starts.contains(&name) {
                continue;
            }
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
                    ServiceState::Failed
                        | ServiceState::Stopped
                        | ServiceState::Completed
                        | ServiceState::Blocked
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
                    runtime.stop_reason = None;
                    runtime.stop_deadline_ms = None;
                    runtime.stop_escalated = false;
                    runtime.restart_after_stop = false;
                    runtime.state = match config.readiness {
                        Readiness::Spawn => ServiceState::Running,
                        Readiness::Notify => ServiceState::Starting,
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

        let stop_actions: Vec<(String, ChildHandle, Signal, bool, bool)> = self
            .services
            .iter()
            .filter_map(|(name, runtime)| {
                if runtime.state == ServiceState::Starting {
                    let timeout = self.config.services[name].readiness_timeout_ms;
                    return (timeout != 0
                        && now_ms.saturating_sub(runtime.started_at_ms) >= timeout)
                        .then_some((
                            name.clone(),
                            runtime.child?,
                            self.config.services[name].stop_signal,
                            true,
                            true,
                        ));
                }
                if runtime.state == ServiceState::Stopping {
                    return (runtime
                        .stop_deadline_ms
                        .is_some_and(|deadline| now_ms >= deadline)
                        && !runtime.stop_escalated)
                        .then_some((
                            name.clone(),
                            runtime.child?,
                            self.config.services[name].kill_signal,
                            false,
                            false,
                        ));
                }
                let timeout = self.config.services[name].timeout_ms;
                if timeout == 0
                    || now_ms.saturating_sub(runtime.started_at_ms) < timeout
                    || !matches!(runtime.state, ServiceState::Ready | ServiceState::Running)
                {
                    return None;
                }
                runtime.child.map(|child| {
                    (
                        name.clone(),
                        child,
                        self.config.services[name].stop_signal,
                        true,
                        false,
                    )
                })
            })
            .collect();
        for (name, child, signal, begin_stop, readiness_timeout) in stop_actions {
            backend.signal(child, signal)?;
            let runtime = self.services.get_mut(&name).expect("service exists");
            if begin_stop {
                runtime.state = ServiceState::Stopping;
                runtime.stop_reason = Some(StopReason::Timeout);
                runtime.stop_deadline_ms =
                    Some(now_ms.saturating_add(self.config.services[&name].stop_timeout_ms));
                runtime.stop_escalated = false;
            } else {
                runtime.stop_escalated = true;
            }
            self.record(
                &name,
                LogLevel::Warn,
                if begin_stop {
                    if readiness_timeout {
                        "readiness-timeout"
                    } else {
                        "timeout"
                    }
                } else {
                    "stop-timeout"
                },
                format!("child={child};signal={signal:?}"),
            );
        }
        Ok(())
    }

    pub fn stop<B: ProcessBackend>(
        &mut self,
        backend: &mut B,
        name: &str,
    ) -> Result<(), SpawnError> {
        let runtime = self
            .services
            .get(name)
            .ok_or_else(|| SpawnError::Invalid(format!("unknown service {name}")))?;
        let child = runtime.child;
        let already_stopping = runtime.state == ServiceState::Stopping;
        let stop_signal = self.config.services[name].stop_signal;
        let stop_timeout_ms = self.config.services[name].stop_timeout_ms;
        if already_stopping {
            return Ok(());
        }
        if let Some(child) = child {
            backend.signal(child, stop_signal)?;
        }
        self.manual_starts.remove(name);
        let runtime = self.services.get_mut(name).expect("service was checked");
        runtime.restart_after_stop = false;
        runtime.stop_escalated = false;
        if child.is_some() {
            runtime.state = ServiceState::Stopping;
            runtime.stop_reason = Some(StopReason::Manual);
            runtime.stop_deadline_ms = Some(self.now_ms.saturating_add(stop_timeout_ms));
        } else {
            runtime.state = ServiceState::Stopped;
            runtime.stop_reason = None;
            runtime.stop_deadline_ms = None;
        }
        self.record(name, LogLevel::Notice, "stop", "requested".to_owned());
        Ok(())
    }

    pub fn restart<B: ProcessBackend>(
        &mut self,
        backend: &mut B,
        name: &str,
    ) -> Result<(), SpawnError> {
        let had_child = self
            .services
            .get(name)
            .ok_or_else(|| SpawnError::Invalid(format!("unknown service {name}")))?
            .child
            .is_some();
        self.stop(backend, name)?;
        self.manual_starts.insert(name.to_owned());
        let runtime = self.services.get_mut(name).expect("service was checked");
        runtime.restart_count = 0;
        runtime.next_start_ms = self.now_ms;
        runtime.restart_after_stop = had_child;
        if !had_child {
            runtime.state = ServiceState::Defined;
        }
        self.record(name, LogLevel::Notice, "restart", "queued".to_owned());
        Ok(())
    }

    pub fn start(&mut self, name: &str) -> Result<(), SpawnError> {
        let runtime = self
            .services
            .get(name)
            .ok_or_else(|| SpawnError::Invalid(format!("unknown service {name}")))?;
        if runtime.child.is_some()
            || matches!(
                runtime.state,
                ServiceState::Starting
                    | ServiceState::Ready
                    | ServiceState::Running
                    | ServiceState::Stopping
            )
        {
            return Err(SpawnError::Invalid(format!(
                "service {name} is already running"
            )));
        }
        self.manual_starts.insert(name.to_owned());
        let runtime = self.services.get_mut(name).expect("service was checked");
        runtime.state = ServiceState::Defined;
        runtime.next_start_ms = self.now_ms;
        runtime.restart_count = 0;
        self.record(name, LogLevel::Notice, "start", "queued".to_owned());
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
        let mut first_error = None;
        for (name, child) in children {
            match backend.signal(child, signal) {
                Ok(()) => self.record(
                    &name,
                    LogLevel::Debug,
                    "signal",
                    format!("child={child};signal={signal:?}"),
                ),
                Err(error) => {
                    self.record(
                        &name,
                        LogLevel::Error,
                        "signal-failed",
                        format!("child={child};signal={signal:?};error={error}"),
                    );
                    first_error.get_or_insert(error);
                }
            }
        }
        if let Some(error) = first_error {
            Err(error)
        } else {
            Ok(())
        }
    }

    pub fn request_shutdown<B: ProcessBackend>(
        &mut self,
        backend: &mut B,
        mode: ShutdownMode,
    ) -> Result<(), SpawnError> {
        self.shutdown = Some(mode);
        self.shutdown_started_at_ms = self.now_ms;
        self.shutdown_escalated = false;
        self.forward_signal(backend, self.config.shutdown.signal)?;
        self.record("init", LogLevel::Notice, "shutdown", format!("{mode:?}"));
        Ok(())
    }

    pub fn notify_ready(&mut self, name: &str) -> Result<(), SpawnError> {
        let runtime = self
            .services
            .get_mut(name)
            .ok_or_else(|| SpawnError::Invalid(format!("unknown service {name}")))?;
        if runtime.child.is_none() {
            return Err(SpawnError::Invalid(format!(
                "service {name} is not running"
            )));
        }
        match runtime.state {
            ServiceState::Starting => {
                runtime.state = ServiceState::Ready;
                self.record(name, LogLevel::Notice, "ready", "notification".to_owned());
                Ok(())
            }
            ServiceState::Ready | ServiceState::Running => Ok(()),
            state => Err(SpawnError::Invalid(format!(
                "service {name} cannot become ready from {state:?}"
            ))),
        }
    }

    pub const fn shutdown_config(&self) -> &ShutdownConfig {
        &self.config.shutdown
    }

    fn can_start(&self, name: &str) -> bool {
        self.config.services[name]
            .dependencies
            .iter()
            .all(|dependency| {
                matches!(
                    self.services[dependency].state,
                    ServiceState::Ready | ServiceState::Running | ServiceState::Completed
                )
            })
    }

    fn dependencies_failed(&self, name: &str) -> bool {
        self.config.services[name]
            .dependencies
            .iter()
            .any(|dependency| {
                matches!(
                    self.services[dependency].state,
                    ServiceState::Failed | ServiceState::Blocked | ServiceState::Stopped
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
        if !self.storage_available {
            let write_serial = match self.config.logging.on_storage_unavailable {
                StorageFailureAction::Serial | StorageFailureAction::Halt => true,
                StorageFailureAction::Continue => self.serial_fallback,
            };
            if write_serial {
                self.serial.push_back(line);
                while self.serial.len() > self.retention {
                    self.serial.pop_front();
                }
            }
            if self.config.logging.on_storage_unavailable == StorageFailureAction::Halt
                && self.shutdown.is_none()
            {
                self.shutdown = Some(ShutdownMode::Shutdown);
            }
            return;
        }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlError {
    Invalid(String),
    Backend(SpawnError),
}

impl fmt::Display for ControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(value) => formatter.write_str(value),
            Self::Backend(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ControlError {}

impl From<SpawnError> for ControlError {
    fn from(error: SpawnError) -> Self {
        Self::Backend(error)
    }
}

pub fn execute_command<B: ProcessBackend>(
    supervisor: &mut Supervisor,
    backend: &mut B,
    command: Command,
) -> Result<String, ControlError> {
    let output = match command {
        Command::Status => supervisor.render_status(),
        Command::Logs { service } => supervisor.render_logs(service.as_deref()),
        Command::Start(name) => {
            supervisor.start(&name)?;
            format!("start service={name} state=queued")
        }
        Command::Stop(name) => {
            supervisor.stop(backend, &name)?;
            format!(
                "stop service={name} state={:?}",
                supervisor.state(&name).expect("service was checked")
            )
        }
        Command::Restart(name) => {
            supervisor.restart(backend, &name)?;
            format!(
                "restart service={name} state={:?}",
                supervisor.state(&name).expect("service was checked")
            )
        }
        Command::LogLevel(level) => {
            supervisor.set_log_level(level);
            format!("log-level={level:?}")
        }
        Command::Shutdown => {
            supervisor.request_shutdown(backend, ShutdownMode::Shutdown)?;
            "shutdown requested".to_owned()
        }
        Command::Reboot => {
            supervisor.request_shutdown(backend, ShutdownMode::Reboot)?;
            "reboot requested".to_owned()
        }
    };
    Ok(output)
}

pub fn execute_line<B: ProcessBackend>(
    supervisor: &mut Supervisor,
    backend: &mut B,
    line: &str,
) -> Result<String, ControlError> {
    let command = parse_command(line).map_err(|error| ControlError::Invalid(error.to_owned()))?;
    execute_command(supervisor, backend, command)
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
        on_storage_unavailable = "serial"

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
        assert_eq!(
            config.logging.on_storage_unavailable,
            StorageFailureAction::Serial
        );
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

    #[test]
    fn parses_boot_gates_and_recovery_shell() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            recovery = "shell"
            [boot]
            shell = "shell"
            recovery_shell = "recovery"
            mounts = ["mounts"]
            devices = ["devices"]
            logging = ["logger"]
            [service.shell]
            command = "/bin/sh"
            [service.recovery]
            command = "/bin/recovery"
            autostart = false
            [service.mounts]
            command = "/bin/mounts"
            [service.devices]
            command = "/bin/devices"
            [service.logger]
            command = "/bin/logger"
        "#,
        )
        .expect("boot config should parse");
        assert_eq!(config.boot.shell.as_deref(), Some("shell"));
        assert_eq!(config.boot.recovery_shell.as_deref(), Some("recovery"));
        assert_eq!(config.boot.mounts, vec!["mounts"]);
        assert_eq!(config.boot.devices, vec!["devices"]);
        assert_eq!(config.boot.logging, vec!["logger"]);
        assert_eq!(recovery_mode_for_source("not valid"), RecoveryMode::Shell);
    }

    #[test]
    fn rejects_malformed_config_before_startup() {
        let duplicate = r#"
            [init]
            version = 1
            version = 1
        "#;
        assert!(matches!(
            parse_config(duplicate),
            Err(ConfigError::DuplicateKey(_))
        ));

        let unknown = r#"
            [init]
            version = 1
            unexpected = true
        "#;
        assert!(matches!(
            parse_config(unknown),
            Err(ConfigError::UnknownKey(_))
        ));
    }

    #[derive(Default)]
    struct FakeBackend {
        next_child: ChildHandle,
        spawned: Vec<String>,
        pending: VecDeque<(ChildHandle, ChildExit)>,
        signals: Vec<(ChildHandle, Signal)>,
        failed_signals: BTreeSet<ChildHandle>,
        spawn_error: Option<SpawnError>,
        orphan_count: u32,
    }

    impl ProcessBackend for FakeBackend {
        fn spawn(
            &mut self,
            name: &str,
            _config: &ServiceConfig,
        ) -> Result<ChildHandle, SpawnError> {
            if let Some(error) = &self.spawn_error {
                return Err(error.clone());
            }
            self.next_child += 1;
            self.spawned.push(name.to_owned());
            Ok(self.next_child)
        }

        fn signal(&mut self, child: ChildHandle, signal: Signal) -> Result<(), SpawnError> {
            self.signals.push((child, signal));
            if self.failed_signals.contains(&child) {
                return Err(SpawnError::Unavailable);
            }
            Ok(())
        }

        fn poll(&mut self) -> Vec<(ChildHandle, ChildExit)> {
            self.pending.drain(..).collect()
        }

        fn reap_orphans(&mut self) -> u32 {
            std::mem::take(&mut self.orphan_count)
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
    fn spawn_failure_fails_service_and_blocks_dependents() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            [service.app]
            command = "/bin/app"
            dependencies = ["missing"]
            [service.missing]
            command = "/bin/missing"
        "#,
        )
        .expect("config should parse");
        let mut supervisor = Supervisor::new(config);
        let mut backend = FakeBackend {
            spawn_error: Some(SpawnError::Unavailable),
            ..FakeBackend::default()
        };
        supervisor.tick(&mut backend, 0).expect("tick should work");
        assert_eq!(supervisor.state("missing"), Some(ServiceState::Failed));
        supervisor.tick(&mut backend, 1).expect("tick should work");
        assert_eq!(supervisor.state("app"), Some(ServiceState::Blocked));
        assert!(backend.spawned.is_empty());
    }

    #[test]
    fn reaps_orphans_and_keeps_sequence_logs_bounded() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            [logging]
            level = "debug"
            retention = 2
            [service.app]
            command = "/bin/app"
        "#,
        )
        .expect("config should parse");
        let mut supervisor = Supervisor::new(config);
        let mut backend = FakeBackend::default();
        supervisor.tick(&mut backend, 0).expect("tick should work");
        backend.orphan_count = 3;
        supervisor.tick(&mut backend, 1).expect("tick should work");
        assert_eq!(supervisor.logs().len(), 2);
        assert!(supervisor.render_logs(Some("init")).contains("reap"));
        let sequences: Vec<u64> = supervisor.logs().map(|record| record.sequence).collect();
        assert!(sequences.windows(2).all(|window| window[0] < window[1]));
    }

    #[test]
    fn gates_shell_until_mounts_devices_and_logging_are_ready() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            [boot]
            shell = "shell"
            recovery_shell = "recovery"
            mounts = ["mounts"]
            devices = ["devices"]
            logging = ["logger"]
            [service.shell]
            command = "/bin/sh"
            order = 0
            [service.recovery]
            command = "/bin/recovery"
            autostart = false
            [service.mounts]
            command = "/bin/mounts"
            order = 10
            [service.devices]
            command = "/bin/devices"
            order = 10
            [service.logger]
            command = "/bin/logger"
            order = 10
        "#,
        )
        .expect("config should parse");
        let mut supervisor = Supervisor::new(config);
        let mut backend = FakeBackend::default();
        supervisor.tick(&mut backend, 0).expect("tick should work");
        assert_eq!(backend.spawned, vec!["devices", "logger", "mounts"]);
        assert!(supervisor.boot_ready());
        assert_eq!(supervisor.state("shell"), Some(ServiceState::Defined));
        supervisor.tick(&mut backend, 1).expect("tick should work");
        assert_eq!(
            backend.spawned,
            vec!["devices", "logger", "mounts", "shell"]
        );

        supervisor
            .start_recovery_shell()
            .expect("recovery shell should queue");
        supervisor.tick(&mut backend, 2).expect("tick should work");
        assert!(backend.spawned.contains(&"recovery".to_owned()));
    }

    #[test]
    fn storage_unavailable_keeps_normal_shell_gated() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            [boot]
            shell = "shell"
            [service.shell]
            command = "/bin/sh"
        "#,
        )
        .expect("config should parse");
        let mut supervisor = Supervisor::new(config);
        let mut backend = FakeBackend::default();
        supervisor.set_storage_available(false);
        supervisor.tick(&mut backend, 0).expect("tick should work");
        assert!(backend.spawned.is_empty());
        supervisor.set_storage_available(true);
        supervisor.tick(&mut backend, 1).expect("tick should work");
        assert_eq!(backend.spawned, vec!["shell"]);
    }

    #[test]
    fn manual_start_controls_non_autostart_service() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            [service.app]
            command = "/bin/app"
            autostart = false
            mode = "foreground"
        "#,
        )
        .expect("config should parse");
        let mut supervisor = Supervisor::new(config);
        let mut backend = FakeBackend::default();
        supervisor.tick(&mut backend, 0).expect("tick should work");
        assert!(backend.spawned.is_empty());
        assert_eq!(supervisor.state("app"), Some(ServiceState::Defined));

        supervisor.start("app").expect("manual start should queue");
        supervisor.tick(&mut backend, 1).expect("tick should work");
        assert_eq!(backend.spawned, vec!["app"]);
        assert_eq!(supervisor.state("app"), Some(ServiceState::Running));
    }

    #[test]
    fn oneshot_success_completes_and_unblocks_dependents() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            [service.prepare]
            command = "/bin/prepare"
            kind = "oneshot"
            [service.app]
            command = "/bin/app"
            dependencies = ["prepare"]
        "#,
        )
        .expect("config should parse");
        let mut supervisor = Supervisor::new(config);
        let mut backend = FakeBackend::default();
        supervisor.tick(&mut backend, 0).expect("tick should work");
        assert_eq!(backend.spawned, vec!["prepare"]);
        backend.pending.push_back((1, ChildExit::success()));
        supervisor.tick(&mut backend, 1).expect("tick should work");
        assert_eq!(supervisor.state("prepare"), Some(ServiceState::Completed));
        assert_eq!(backend.spawned, vec!["prepare", "app"]);
        assert_eq!(supervisor.state("app"), Some(ServiceState::Running));
    }

    #[test]
    fn manual_stop_does_not_apply_always_restart() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            [service.app]
            command = "/bin/app"
            restart = "always"
        "#,
        )
        .expect("config should parse");
        let mut supervisor = Supervisor::new(config);
        let mut backend = FakeBackend::default();
        supervisor.tick(&mut backend, 0).expect("tick should work");
        supervisor
            .stop(&mut backend, "app")
            .expect("stop should work");
        assert_eq!(supervisor.state("app"), Some(ServiceState::Stopping));
        backend.pending.push_back((1, ChildExit::signal(15)));
        supervisor.tick(&mut backend, 1).expect("tick should work");
        assert_eq!(supervisor.state("app"), Some(ServiceState::Stopped));
        supervisor.tick(&mut backend, 2).expect("tick should work");
        assert_eq!(backend.spawned, vec!["app"]);
    }

    #[test]
    fn forwards_signal_to_all_children_after_one_failure() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            [service.a]
            command = "/bin/a"
            [service.b]
            command = "/bin/b"
        "#,
        )
        .expect("config should parse");
        let mut supervisor = Supervisor::new(config);
        let mut backend = FakeBackend::default();
        supervisor.tick(&mut backend, 0).expect("tick should work");
        backend.failed_signals.insert(1);
        assert_eq!(
            supervisor.forward_signal(&mut backend, Signal::Terminate),
            Err(SpawnError::Unavailable)
        );
        assert_eq!(
            backend.signals,
            vec![(1, Signal::Terminate), (2, Signal::Terminate)]
        );
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
    fn parses_full_service_contract() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            [env]
            PATH = "/bin"
            [shutdown]
            signal = "hangup"
            timeout_ms = 1000
            on_timeout = "halt"
            [service.app]
            command = "/bin/app"
            workdir = "/var/app"
            mode = "background"
            kind = "daemon"
            autostart = false
            stdin = "null"
            stdout = "serial"
            stderr = "file:/var/log/app.log"
            order = 7
            readiness = "notify"
            readiness_timeout_ms = 250
            stop_timeout_ms = 750
            stop_signal = "hangup"
            kill_signal = "kill"
            timeout_action = "fail"
            capabilities = ["net-admin", "memory-map"]
            [service.app.env]
            APP_MODE = "production"
            [service.app.limits]
            cpu_time_ms = 10000
            memory_bytes = 65536
            open_fds = 16
            child_processes = 4
        "#,
        )
        .expect("full service contract should parse");
        let service = &config.services["app"];
        assert_eq!(service.mode, ServiceMode::Background);
        assert_eq!(service.kind, ServiceKind::Daemon);
        assert!(!service.autostart);
        assert_eq!(service.stdin, StreamSpec::Null);
        assert_eq!(service.stdout, StreamSpec::Serial);
        assert_eq!(
            service.stderr,
            StreamSpec::File("/var/log/app.log".to_owned())
        );
        assert_eq!(service.environment["APP_MODE"], "production");
        assert_eq!(service.environment["PATH"], "/bin");
        assert_eq!(service.order, 7);
        assert_eq!(service.readiness, Readiness::Notify);
        assert_eq!(service.readiness_timeout_ms, 250);
        assert_eq!(service.stop_timeout_ms, 750);
        assert_eq!(service.stop_signal, Signal::Hangup);
        assert_eq!(service.timeout_action, TimeoutAction::Fail);
        assert_eq!(service.limits.memory_bytes, Some(65536));
        assert_eq!(config.shutdown.signal, Signal::Hangup);
        assert_eq!(config.shutdown.on_timeout, ShutdownTimeoutAction::Halt);
    }

    #[test]
    fn rejects_unsafe_contract_values() {
        let bad_stream = r#"
            [init]
            version = 1
            [service.app]
            command = "/bin/app"
            stdout = "file:relative.log"
        "#;
        assert!(matches!(
            parse_config(bad_stream),
            Err(ConfigError::InvalidValue(_))
        ));

        let bad_capability = r#"
            [init]
            version = 1
            [service.app]
            command = "/bin/app"
            capabilities = ["all"]
        "#;
        assert!(matches!(
            parse_config(bad_capability),
            Err(ConfigError::InvalidValue(_))
        ));

        let missing_readiness_timeout = r#"
            [init]
            version = 1
            [service.app]
            command = "/bin/app"
            readiness = "notify"
        "#;
        assert!(matches!(
            parse_config(missing_readiness_timeout),
            Err(ConfigError::MissingValue(_))
        ));

        let daemon_without_notification = r#"
            [init]
            version = 1
            [service.app]
            command = "/bin/app"
            kind = "daemon"
        "#;
        assert!(matches!(
            parse_config(daemon_without_notification),
            Err(ConfigError::InvalidValue(_))
        ));
    }

    #[test]
    fn starts_by_explicit_order_and_accepts_readiness_notification() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            [service.app]
            command = "/bin/app"
            order = 20
            dependencies = ["gate"]
            [service.gate]
            command = "/bin/gate"
            order = 10
            readiness = "notify"
            readiness_timeout_ms = 100
        "#,
        )
        .expect("config should parse");
        let mut supervisor = Supervisor::new(config);
        let mut backend = FakeBackend::default();
        supervisor.tick(&mut backend, 0).expect("tick should work");
        assert_eq!(backend.spawned, vec!["gate"]);
        assert_eq!(supervisor.state("gate"), Some(ServiceState::Starting));
        assert_eq!(supervisor.state("app"), Some(ServiceState::Defined));

        supervisor
            .notify_ready("gate")
            .expect("readiness notification should work");
        supervisor.tick(&mut backend, 1).expect("tick should work");
        assert_eq!(backend.spawned, vec!["gate", "app"]);
        assert_eq!(supervisor.state("gate"), Some(ServiceState::Ready));
        assert_eq!(supervisor.state("app"), Some(ServiceState::Running));
    }

    #[test]
    fn readiness_timeout_and_shutdown_use_declared_policy() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            [shutdown]
            signal = "kill"
            timeout_ms = 200
            on_timeout = "kill"
            [service.app]
            command = "/bin/app"
            readiness = "notify"
            readiness_timeout_ms = 5
        "#,
        )
        .expect("config should parse");
        let mut supervisor = Supervisor::new(config);
        let mut backend = FakeBackend::default();
        supervisor.tick(&mut backend, 0).expect("tick should work");
        supervisor.tick(&mut backend, 5).expect("tick should work");
        assert_eq!(supervisor.state("app"), Some(ServiceState::Stopping));
        assert_eq!(backend.signals, vec![(1, Signal::Terminate)]);
        assert_eq!(supervisor.shutdown_config().timeout_ms, 200);

        supervisor
            .request_shutdown(&mut backend, ShutdownMode::Reboot)
            .expect("shutdown should work");
        assert_eq!(supervisor.shutdown_mode(), Some(ShutdownMode::Reboot));
        assert_eq!(
            backend.signals,
            vec![(1, Signal::Terminate), (1, Signal::Kill)]
        );
        supervisor
            .tick(&mut backend, 205)
            .expect("tick should work");
        assert_eq!(
            backend.signals,
            vec![(1, Signal::Terminate), (1, Signal::Kill), (1, Signal::Kill)]
        );
    }

    #[test]
    fn runtime_timeout_escalates_once_and_applies_fail_action() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            [service.app]
            command = "/bin/app"
            timeout_ms = 5
            stop_timeout_ms = 10
            kill_signal = "hangup"
            timeout_action = "fail"
        "#,
        )
        .expect("config should parse");
        let mut supervisor = Supervisor::new(config);
        let mut backend = FakeBackend::default();
        supervisor.tick(&mut backend, 0).expect("tick should work");
        supervisor.tick(&mut backend, 5).expect("tick should work");
        assert_eq!(supervisor.state("app"), Some(ServiceState::Stopping));
        assert_eq!(backend.signals, vec![(1, Signal::Terminate)]);
        supervisor.tick(&mut backend, 15).expect("tick should work");
        assert_eq!(
            backend.signals,
            vec![(1, Signal::Terminate), (1, Signal::Hangup)]
        );
        backend.pending.push_back((1, ChildExit::signal(15)));
        supervisor.tick(&mut backend, 16).expect("tick should work");
        assert_eq!(supervisor.state("app"), Some(ServiceState::Failed));
        supervisor.tick(&mut backend, 17).expect("tick should work");
        assert_eq!(
            backend.signals,
            vec![(1, Signal::Terminate), (1, Signal::Hangup)]
        );
    }

    #[test]
    fn applies_explicit_storage_unavailable_policy() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            [logging]
            level = "debug"
            retention = 2
            serial_fallback = false
            on_storage_unavailable = "serial"
            [service.app]
            command = "/bin/app"
        "#,
        )
        .expect("config should parse");
        let mut supervisor = Supervisor::new(config);
        supervisor.set_storage_available(false);
        supervisor.set_log_level(LogLevel::Debug);
        assert!(supervisor.logs().next().is_none());
        assert_eq!(supervisor.serial_lines().len(), 1);
        assert!(supervisor
            .serial_lines()
            .next()
            .unwrap()
            .contains("log-level"));

        let halt_config = parse_config(
            r#"
            [init]
            version = 1
            [logging]
            on_storage_unavailable = "halt"
            [service.app]
            command = "/bin/app"
        "#,
        )
        .expect("config should parse");
        let mut halted = Supervisor::new(halt_config);
        halted.set_storage_available(false);
        halted.set_log_level(LogLevel::Notice);
        assert_eq!(halted.shutdown_mode(), Some(ShutdownMode::Shutdown));
    }

    #[test]
    fn renders_stable_status_and_logs() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            [logging]
            level = "debug"
            [service.app]
            command = "/bin/app"
        "#,
        )
        .expect("config should parse");
        let mut supervisor = Supervisor::new(config);
        let mut backend = FakeBackend::default();
        supervisor.tick(&mut backend, 42).expect("tick should work");
        assert_eq!(
            supervisor.render_status(),
            "service=app state=Running exit_code=- exit_signal=-"
        );
        let logs = supervisor.render_logs(Some("app"));
        assert!(logs.contains("app start"));
        assert_eq!(supervisor.render_logs(Some("missing")), "no logs");
    }

    #[test]
    fn dispatches_control_commands_and_reports_state() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            [service.app]
            command = "/bin/app"
            autostart = false
        "#,
        )
        .expect("config should parse");
        let mut supervisor = Supervisor::new(config);
        let mut backend = FakeBackend::default();
        assert_eq!(
            execute_line(&mut supervisor, &mut backend, "start app"),
            Ok("start service=app state=queued".to_owned())
        );
        supervisor.tick(&mut backend, 1).expect("tick should work");
        assert_eq!(
            execute_line(&mut supervisor, &mut backend, "stop app"),
            Ok("stop service=app state=Stopping".to_owned())
        );
        backend.pending.push_back((1, ChildExit::signal(15)));
        supervisor.tick(&mut backend, 2).expect("tick should work");
        assert!(execute_line(&mut supervisor, &mut backend, "start missing").is_err());
    }

    #[test]
    fn dispatches_shutdown_and_reboot_modes() {
        let config = parse_config(
            r#"
            [init]
            version = 1
            [service.app]
            command = "/bin/app"
        "#,
        )
        .expect("config should parse");
        let mut supervisor = Supervisor::new(config);
        let mut backend = FakeBackend::default();
        supervisor.tick(&mut backend, 0).expect("tick should work");
        assert_eq!(
            execute_line(&mut supervisor, &mut backend, "reboot"),
            Ok("reboot requested".to_owned())
        );
        assert_eq!(supervisor.shutdown_mode(), Some(ShutdownMode::Reboot));
        assert_eq!(backend.signals, vec![(1, Signal::Terminate)]);
        assert!(supervisor.render_logs(Some("init")).contains("shutdown"));
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
