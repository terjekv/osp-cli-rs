use crate::core::output::OutputFormat;
use crate::core::output_model::OutputResult;

use super::settings::{RenderProfile, RenderSettings, ResolvedRenderSettings, resolve_settings};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderPlan {
    pub format: OutputFormat,
    pub profile: RenderProfile,
    pub settings: ResolvedRenderSettings,
    pub document_long_help: bool,
    pub document_entry_gap: Option<usize>,
}

pub fn plan_output(
    output: &OutputResult,
    settings: &RenderSettings,
    profile: RenderProfile,
) -> RenderPlan {
    let format = settings.resolve_output_format(output);
    let effective_profile = if matches!(format, OutputFormat::Json) {
        RenderProfile::CopySafe
    } else {
        profile
    };
    RenderPlan {
        format,
        profile: effective_profile,
        settings: resolve_settings(settings, effective_profile),
        document_entry_gap: output.meta.document_entry_gap,
        document_long_help: output.meta.document_long_help,
    }
}
