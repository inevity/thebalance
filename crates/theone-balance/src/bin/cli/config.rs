use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, ValueEnum, Serialize, Deserialize)]
pub enum ConfigSource {
    OneBalance,
    TheOne,
}
