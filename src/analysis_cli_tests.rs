use std::collections::BTreeSet;

use clap::CommandFactory;

use super::Cli;

#[test]
fn keeps_translation_workspace_commands_in_the_analysis_binary() {
    let commands = Cli::command()
        .get_subcommands()
        .map(|command| command.get_name().to_owned())
        .collect::<BTreeSet<_>>();
    assert!(commands.contains("prepare-translation-workspace"));
    assert!(commands.contains("audit-translation-draft"));
    assert!(commands.contains("audit-translation-assets"));
    assert!(commands.contains("audit-translation-contexts"));
    assert!(commands.contains("audit-translation-overlay"));
    assert!(commands.contains("audit-translation-catalog"));
    assert!(commands.contains("detect-message-overflow"));
    assert!(commands.contains("rebuild-message-files"));
}
