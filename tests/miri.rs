#![allow(missing_docs)]

use osp_cli::core::OutputFormat;
use osp_cli::core::OutputResult;
use osp_cli::row;
use osp_cli::ui::{RenderSettings, render};
use serde_json::json;

#[test]
fn miri_nested_key_value_rendering_stays_structured() {
    let mut output = OutputResult::from_rows(vec![row! {
        "uid" => "oistes",
        "netgroups" => json!(["it-uio-alle-drift", "vcs-virtprov-admins"]),
        "profile" => json!({
            "owner": "oistes",
            "groups": ["usit", "vortex-opptak"],
        }),
    }]);
    output.meta.key_index = vec![
        "uid".to_string(),
        "netgroups".to_string(),
        "profile".to_string(),
    ];

    let mut settings = RenderSettings::test_plain(OutputFormat::Mreg);
    settings.format_explicit = true;
    let rendered = render(&output, &osp_cli::ui::RenderRequest::new(&settings));

    assert!(rendered.contains("uid:"));
    assert!(rendered.contains("netgroups (2):"));
    assert!(rendered.contains("it-uio-alle-drift"));
    assert!(rendered.contains("profile:"));
    assert!(rendered.contains("groups (2): usit"));
    assert!(!rendered.contains("[\"it-uio-alle-drift\""));
}
