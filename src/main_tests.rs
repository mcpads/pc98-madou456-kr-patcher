use std::collections::BTreeSet;

use clap::CommandFactory;

use super::Cli;

#[test]
fn default_builder_exposes_plain_and_localized_reassembly_without_analysis_commands() {
    let cli = Cli::command();
    let commands = cli
        .get_subcommands()
        .map(|command| command.get_name())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        commands,
        BTreeSet::from(["build", "build-localized", "verify-sources"])
    );
}
