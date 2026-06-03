#![allow(dead_code)]

use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Open,
    Restricted,
    Hermetic,
    Custom,
}

#[derive(Debug, thiserror::Error)]
#[error("unknown profile: '{0}'")]
pub struct UnknownProfile(pub String);

impl FromStr for Profile {
    type Err = UnknownProfile;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "open" => Ok(Self::Open),
            "restricted" => Ok(Self::Restricted),
            "hermetic" => Ok(Self::Hermetic),
            "custom" => Ok(Self::Custom),
            _ => Err(UnknownProfile(s.to_owned())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FilesystemAccess {
    pub allowed_paths: Vec<String>,
    pub writable_paths: Vec<String>,
}

impl Default for FilesystemAccess {
    fn default() -> Self {
        Self {
            allowed_paths: Vec::new(),
            writable_paths: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NetworkAccess {
    pub enabled: bool,
    pub allowed_hosts: Vec<String>,
}

impl Default for NetworkAccess {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_hosts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EnvironmentAccess {
    pub allowed_vars: Vec<String>,
}

impl Default for EnvironmentAccess {
    fn default() -> Self {
        Self {
            allowed_vars: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProcessAccess {
    pub allow_spawn: bool,
}

impl Default for ProcessAccess {
    fn default() -> Self {
        Self { allow_spawn: false }
    }
}

#[derive(Debug, Clone)]
pub struct ClockAccess {
    pub deterministic: bool,
}

impl Default for ClockAccess {
    fn default() -> Self {
        Self {
            deterministic: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SandboxConfig {
    pub profile: Profile,
    pub filesystem: FilesystemAccess,
    pub network: NetworkAccess,
    pub environment: EnvironmentAccess,
    pub process: ProcessAccess,
    pub clock: ClockAccess,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            profile: Profile::Custom,
            filesystem: FilesystemAccess::default(),
            network: NetworkAccess::default(),
            environment: EnvironmentAccess::default(),
            process: ProcessAccess::default(),
            clock: ClockAccess::default(),
        }
    }
}

impl SandboxConfig {
    #[must_use]
    pub fn for_profile(profile: Profile) -> Self {
        match profile {
            Profile::Open => Self {
                profile,
                network: NetworkAccess {
                    enabled: true,
                    ..Default::default()
                },
                process: ProcessAccess {
                    allow_spawn: true,
                },
                environment: EnvironmentAccess {
                    allowed_vars: vec!["*".to_string()],
                },
                ..Default::default()
            },
            Profile::Restricted => Self {
                profile,
                filesystem: FilesystemAccess {
                    allowed_paths: vec!["./".to_string()],
                    ..Default::default()
                },
                ..Default::default()
            },
            Profile::Hermetic => Self {
                profile,
                clock: ClockAccess {
                    deterministic: true,
                },
                ..Default::default()
            },
            Profile::Custom => Self {
                profile,
                ..Default::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_has_network_and_process() {
        let config = SandboxConfig::for_profile(Profile::Open);
        assert!(config.network.enabled);
        assert!(config.process.allow_spawn);
    }

    #[test]
    fn test_restricted_has_no_network() {
        let config = SandboxConfig::for_profile(Profile::Restricted);
        assert!(!config.network.enabled);
        assert!(!config.process.allow_spawn);
    }

    #[test]
    fn test_hermetic_has_deterministic_clock() {
        let config = SandboxConfig::for_profile(Profile::Hermetic);
        assert!(config.clock.deterministic);
    }

    #[test]
    fn test_custom_has_defaults() {
        let config = SandboxConfig::for_profile(Profile::Custom);
        assert!(!config.network.enabled);
        assert!(!config.process.allow_spawn);
        assert!(!config.clock.deterministic);
    }

    #[test]
    fn test_profile_from_string() {
        assert_eq!("open".parse::<Profile>().unwrap(), Profile::Open);
        assert_eq!("restricted".parse::<Profile>().unwrap(), Profile::Restricted);
        assert_eq!("hermetic".parse::<Profile>().unwrap(), Profile::Hermetic);
        assert_eq!("custom".parse::<Profile>().unwrap(), Profile::Custom);
        assert!("unknown".parse::<Profile>().is_err());
    }

    #[test]
    fn test_all_profiles_roundtrip() {
        let profiles = [
            Profile::Open,
            Profile::Restricted,
            Profile::Hermetic,
            Profile::Custom,
        ];
        for p in &profiles {
            let s = format!("{p:?}").to_lowercase();
            let parsed: Profile = s.parse().unwrap();
            assert_eq!(*p, parsed);
        }
    }
}
