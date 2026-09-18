//! Strict parsing and resolution for `.ahclkitconfigs` files.

mod ast;
mod parser;
mod schema;
mod skeleton;

pub use ast::ScalarValue;
pub use parser::{ConfigDocument, ConfigError};
pub use schema::{
    AhclVersion, CargoLockMode, CargoRule, CargoRuleClassification, CargoSettings, ConfigLimits,
    EffectiveConfig, GenerationSettings, Language, LicenseSettings, ProjectSettings, RustSettings,
};
pub use skeleton::{ConfigSkeleton, ProjectIdentity};
