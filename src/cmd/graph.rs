use std::path::PathBuf;

use clap::Parser;
use scoop_rs::knowledge::SpatialKnowledgeGraph;

use crate::Result;

/// Build and query Scooper's semantic repository knowledge graph
#[derive(Parser, Debug)]
pub struct Args {
    /// Repository root to index
    #[clap(default_value = ".")]
    root: PathBuf,

    /// Return evidence related to a name, symbol, or path
    #[clap(long)]
    query: Option<String>,
}

pub fn execute(args: Args) -> Result<()> {
    let graph = SpatialKnowledgeGraph::discover(&args.root, "DarbotLM/scooper")?;
    if let Some(query) = args.query {
        println!(
            "{}",
            serde_json::to_string_pretty(&graph.agent_context(&query))?
        );
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(graph.bitscoop_document())?
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use crate::cmd::Cli;

    #[test]
    fn command_definition_has_unique_options() {
        Cli::command().debug_assert();
    }
}
