use osp_cli::core::document::{DocBlock, Document, DocumentEntry};
use osp_cli::core::output_model::{OutputItems, OutputResult};
use osp_cli::core::row::Row;
use osp_cli::dsl::{apply_output_pipeline, parse_pipeline};
use serde_json::{Value, json};

include!("ported/helpers.rs");
include!("ported/rows.rs");
include!("ported/nasty.rs");
include!("ported/structural.rs");
