fn row(value: Value) -> Row {
    value
        .as_object()
        .cloned()
        .expect("fixture should be an object")
}

fn run_rows_pipeline(rows: Vec<Row>, pipeline: &str) -> OutputResult {
    let parsed = parse_pipeline(&format!("fixture | {pipeline}")).expect("pipeline should parse");
    apply_output_pipeline(OutputResult::from_rows(rows), &parsed.stages)
        .expect("pipeline should succeed")
}

fn run_guide_pipeline(view: Document, pipeline: &str) -> OutputResult {
    let parsed = parse_pipeline(&format!("fixture | {pipeline}")).expect("pipeline should parse");
    apply_output_pipeline(view.to_output_result(), &parsed.stages).expect("pipeline should succeed")
}

fn sample_guide() -> Document
/* result */ {
    Document { blocks: vec![DocBlock::section("Usage", vec![DocBlock::Paragraph { text: "osp intro".to_string() }]),
DocBlock::section("Commands", vec![DocBlock::Entries { entries: sample_commands() }])] }
}

fn sample_commands() -> Vec<DocumentEntry> {
    vec![
        DocumentEntry { name: "help".to_string(), description: "Show overview".to_string() },
        DocumentEntry { name: "config".to_string(), description: "Show config values".to_string() },
        DocumentEntry { name: "exit".to_string(), description: "Leave shell".to_string() },
    ]
}

fn guide_with_sections() -> Document
/* result */ {
    Document { blocks: vec![DocBlock::section("Commands", vec![DocBlock::Entries { entries: vec![
            DocumentEntry { name: "deploy".to_string(), description: "Deploy a VM".to_string() },
            DocumentEntry { name: "list".to_string(), description: "List all VMs".to_string() },
            DocumentEntry { name: "delete".to_string(), description: "Delete a VM".to_string() },
        ] }]),
DocBlock::section("Actions".to_string(), vec![DocBlock::Entries { entries: vec![
                    DocumentEntry { name: "start".to_string(), description: "Start the VM".to_string() },
                    DocumentEntry { name: "stop".to_string(), description: "Stop the VM".to_string() },
                    DocumentEntry { name: "restart".to_string(), description: "Restart the VM".to_string() },
                ] }]),
DocBlock::section("Utilities".to_string(), vec![DocBlock::Entries { entries: vec![
                    DocumentEntry { name: "version".to_string(), description: "Show version info".to_string() },
                    DocumentEntry { name: "doctor".to_string(), description: "Run diagnostics".to_string() },
                ] }])] }
}
