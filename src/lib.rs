pub mod cli;
mod cloud;
mod controller;
mod protocol;
mod storage;

pub use controller::AcController;
use serde::{Deserialize, Serialize};
pub use storage::default_data_directory;

#[boltffi::error]
#[derive(Debug, thiserror::Error)]
pub enum AcError {
    #[error("{message}")]
    Problem { message: String },
    #[error("SmartHome error {code}: {message}")]
    Cloud { code: i32, message: String },
}
pub type Result<T, E = AcError> = std::result::Result<T, E>;
pub(crate) fn problem(message: impl Into<String>) -> AcError {
    AcError::Problem {
        message: message.into(),
    }
}

#[boltffi::data]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Choice {
    pub value: i32,
    pub name: String,
}

#[boltffi::data]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceSnapshot {
    pub id: String,
    pub local_id: u32,
    pub label: String,
    pub mac: String,
    pub connected: bool,
    pub online: bool,
    pub error: String,
    pub updated: f64,
    pub power: bool,
    pub temperature: Option<f64>,
    pub indoor: Option<f64>,
    pub outdoor: Option<f64>,
    pub mode: i32,
    pub fan: i32,
    pub swing: i32,
    pub boost: bool,
    pub custom_fan: bool,
    pub supports_boost: bool,
    pub min_temperature: f64,
    pub max_temperature: f64,
    pub modes: Vec<Choice>,
    pub fans: Vec<Choice>,
    pub swings: Vec<Choice>,
}

#[boltffi::data]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppSnapshot {
    pub signed_in: bool,
    pub devices: Vec<DeviceSnapshot>,
    pub route: String,
}

#[boltffi::data]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SettingsChange {
    pub power: Option<bool>,
    pub temperature: Option<f64>,
    pub mode: Option<i32>,
    pub fan: Option<i32>,
    pub swing: Option<i32>,
    pub boost: Option<bool>,
}

pub(crate) fn choices(values: &[i32], names: &[(i32, &str)]) -> Vec<Choice> {
    values
        .iter()
        .map(|&value| Choice {
            value,
            name: names
                .iter()
                .find(|(v, _)| *v == value)
                .map(|(_, n)| (*n).into())
                .unwrap_or_else(|| value.to_string()),
        })
        .collect()
}
pub const MODE_NAMES: &[(i32, &str)] = &[
    (1, "Auto"),
    (2, "Cool"),
    (3, "Dry"),
    (4, "Heat"),
    (5, "Fan"),
    (6, "Smart dry"),
];
pub const FAN_NAMES: &[(i32, &str)] = &[
    (20, "20%"),
    (40, "40%"),
    (60, "60%"),
    (80, "80%"),
    (100, "100%"),
    (102, "Auto"),
];
pub const SWING_NAMES: &[(i32, &str)] = &[
    (0, "Off"),
    (3, "Horizontal"),
    (12, "Vertical"),
    (15, "Both"),
];
