use crate::temp_support::make_temp_dir;
use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::{Value, json};

include!("plugins/fixtures.rs");
include!("plugins/management.rs");
include!("plugins/dispatch.rs");
include!("plugins/discovery_and_help.rs");
