use crate::core::Row;
use crate::core::{DocBlock, Document, DocumentEntry};
use crate::core::{OutputItems, OutputResult};
use crate::dsl::{apply_output_pipeline, parse_pipeline};
use serde_json::{Value, json};

include!("ported/helpers.rs");
include!("ported/rows.rs");
include!("ported/nasty.rs");
include!("ported/structural.rs");
