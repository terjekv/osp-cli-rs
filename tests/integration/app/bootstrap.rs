#[test]
fn native_configuration_is_available_to_help_and_execution() {
    use osp_cli::app::BufferedUiSink;
    use osp_cli::config::{ConfigLayer, ResolvedConfig};
    use osp_cli::{
        App, NativeCommand, NativeCommandContext, NativeCommandOutcome, NativeCommandRegistry,
    };
    use std::sync::{Arc, Mutex};

    struct ConfiguredCommand(Arc<Mutex<String>>);
    impl NativeCommand for ConfiguredCommand {
        fn configure(&self, config: &ResolvedConfig) {
            *self.0.lock().unwrap() = config
                .get_string("extensions.site.label")
                .unwrap()
                .to_string();
        }
        fn command(&self) -> clap::Command {
            clap::Command::new("site-probe").about(self.0.lock().unwrap().clone())
        }
        fn execute(
            &self,
            _: &[String],
            context: &NativeCommandContext<'_>,
        ) -> anyhow::Result<NativeCommandOutcome> {
            assert_eq!(
                self.0.lock().unwrap().as_str(),
                context.config.get_string("extensions.site.label").unwrap()
            );
            Ok(NativeCommandOutcome::Exit(0))
        }
    }
    let mut defaults = ConfigLayer::default();
    defaults.set("extensions.site.label", "configured-site");
    let app = App::new()
        .with_product_defaults(defaults)
        .with_native_commands(
            NativeCommandRegistry::new()
                .with_command(ConfiguredCommand(Arc::new(Mutex::new(String::new())))),
        );
    let mut sink = BufferedUiSink::default();
    assert_eq!(
        app.run_process_with_sink(["osp", "--defaults-only", "--help"], &mut sink),
        0
    );
    assert!(sink.stdout.contains("configured-site"));
    assert_eq!(
        app.run_process_with_sink(["osp", "--defaults-only", "site-probe"], &mut sink),
        0
    );
}
