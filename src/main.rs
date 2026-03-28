mod discover;
mod error;
mod extract;
mod filter;
mod graph;
mod merge;

use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;

use extract::LanguageExtractor;
use extract::rust::RustExtractor;

#[derive(Parser)]
#[command(
    name = "grapha",
    version,
    about = "Structural code graph for LLM consumption"
)]
struct Cli {
    /// File or directory to analyze
    path: PathBuf,

    /// Output file (default: stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Filter node kinds (comma-separated: fn,struct,enum,trait,impl,mod,field,variant)
    #[arg(long)]
    filter: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let extractor = RustExtractor;
    let files = discover::discover_files(&cli.path, extractor.file_extensions())
        .context("failed to discover files")?;

    let mut results = Vec::new();
    for file in &files {
        let source =
            std::fs::read(file).with_context(|| format!("failed to read {}", file.display()))?;

        // Make path relative to the input path for cleaner IDs
        let relative = if cli.path.is_dir() {
            file.strip_prefix(&cli.path).unwrap_or(file)
        } else {
            file.file_name()
                .map(|n| n.as_ref())
                .unwrap_or(file.as_path())
        };

        match extractor.extract(&source, relative) {
            Ok(result) => results.push(result),
            Err(e) => eprintln!("warning: skipping {}: {e}", file.display()),
        }
    }

    let mut graph = merge::merge(results);

    if let Some(ref filter_str) = cli.filter {
        let kinds = filter::parse_filter(filter_str)?;
        graph = filter::filter_graph(graph, &kinds);
    }

    let json = match &cli.output {
        Some(_) => serde_json::to_string(&graph)?,
        None => serde_json::to_string_pretty(&graph)?,
    };

    match cli.output {
        Some(path) => {
            std::fs::write(&path, &json)
                .with_context(|| format!("failed to write {}", path.display()))?;
            eprintln!("wrote {}", path.display());
        }
        None => println!("{json}"),
    }

    Ok(())
}
