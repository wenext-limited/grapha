mod changes;
mod classify;
mod compress;
mod config;
mod delta;
mod filter;
mod localization;
mod progress;
mod query;
mod render;
mod search;
mod serve;
mod store;

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, anyhow};
use clap::{Parser, Subcommand, ValueEnum};

use store::Store;

#[derive(Parser)]
#[command(
    name = "grapha",
    version,
    about = "Structural code graph for LLM consumption"
)]
struct Cli {
    /// ANSI color mode for tree output
    #[arg(long, global = true, value_enum, default_value_t = ColorMode::Auto)]
    color: ColorMode,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum QueryOutputFormat {
    Json,
    Tree,
}

#[derive(Subcommand)]
enum Commands {
    /// Analyze source files and output graph
    Analyze {
        /// File or directory to analyze
        path: PathBuf,
        /// Output file (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Filter node kinds (comma-separated: fn,struct,enum,trait,impl,mod,field,variant)
        #[arg(long)]
        filter: Option<String>,
        /// Output in compact grouped format (optimized for LLM consumption)
        #[arg(long)]
        compact: bool,
    },
    /// Index a project into persistent storage
    Index {
        /// Project directory to index
        path: PathBuf,
        /// Storage format: "json" or "sqlite" (default: sqlite)
        #[arg(long, default_value = "sqlite")]
        format: String,
        /// Storage directory (default: .grapha/ in project root)
        #[arg(long)]
        store_dir: Option<PathBuf>,
        /// Force a full store/search rebuild instead of using incremental sync
        #[arg(long)]
        full_rebuild: bool,
    },
    /// Query symbol context (callers, callees, implementors)
    Context {
        /// Symbol name or ID
        symbol: String,
        /// Project directory (reads from .grapha/)
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        /// Output format
        #[arg(long, value_enum, default_value_t = QueryOutputFormat::Json)]
        format: QueryOutputFormat,
    },
    /// Analyze blast radius of changing a symbol
    Impact {
        /// Symbol name or ID
        symbol: String,
        /// Maximum traversal depth
        #[arg(long, default_value = "3")]
        depth: usize,
        /// Project directory
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        /// Output format
        #[arg(long, value_enum, default_value_t = QueryOutputFormat::Json)]
        format: QueryOutputFormat,
    },
    /// Detect code changes and analyze their impact
    Changes {
        /// Scope: "unstaged", "staged", "all", or a git ref (e.g., "main")
        #[arg(default_value = "all")]
        scope: String,
        /// Project directory
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
    /// Search symbols by name or file
    Search {
        /// Search query
        query: String,
        /// Max results
        #[arg(long, default_value = "20")]
        limit: usize,
        /// Project directory
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
    /// Forward-trace dataflow from an entry point to terminal operations
    Trace {
        /// Entry point symbol name or ID
        entry: String,
        /// Maximum traversal depth
        #[arg(long, default_value = "10")]
        depth: usize,
        /// Project directory
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        /// Output format
        #[arg(long, value_enum, default_value_t = QueryOutputFormat::Json)]
        format: QueryOutputFormat,
    },
    /// Derive a semantic effect graph from an entry point
    Dataflow {
        /// Entry point symbol name or ID
        entry: String,
        /// Maximum traversal depth
        #[arg(long, default_value = "10")]
        depth: usize,
        /// Project directory
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        /// Output format
        #[arg(long, value_enum, default_value_t = QueryOutputFormat::Json)]
        format: QueryOutputFormat,
    },
    /// Reverse query: which entry points are affected by this symbol?
    Reverse {
        /// Symbol name or ID
        symbol: String,
        /// Project directory
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        /// Output format
        #[arg(long, value_enum, default_value_t = QueryOutputFormat::Json)]
        format: QueryOutputFormat,
    },
    /// List auto-detected entry points
    Entries {
        /// Project directory
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        /// Output format
        #[arg(long, value_enum, default_value_t = QueryOutputFormat::Json)]
        format: QueryOutputFormat,
    },
    /// Resolve localization records reachable from a SwiftUI symbol subtree
    Localize {
        /// Symbol name or ID
        symbol: String,
        /// Project directory
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        /// Output format
        #[arg(long, value_enum, default_value_t = QueryOutputFormat::Json)]
        format: QueryOutputFormat,
    },
    /// Find SwiftUI usage sites for a localization key
    Usages {
        /// Localization key
        key: String,
        /// Optional table/catalog name
        #[arg(long)]
        table: Option<String>,
        /// Project directory
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        /// Output format
        #[arg(long, value_enum, default_value_t = QueryOutputFormat::Json)]
        format: QueryOutputFormat,
    },
    /// Launch web UI for interactive graph exploration
    Serve {
        /// Project directory
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        /// Port to listen on
        #[arg(long, default_value = "8080")]
        port: u16,
    },
}

fn builtin_registry() -> anyhow::Result<grapha_core::LanguageRegistry> {
    let mut registry = grapha_core::LanguageRegistry::new();
    grapha_rust::register_builtin(&mut registry)?;
    grapha_swift::register_builtin(&mut registry)?;
    Ok(registry)
}

/// Run the extraction pipeline on a path, returning a merged graph.
fn run_pipeline(path: &Path, verbose: bool) -> anyhow::Result<grapha_core::graph::Graph> {
    let t = Instant::now();
    let registry = builtin_registry()?;
    let project_context = grapha_core::project_context(path);
    let files = grapha_core::pipeline::discover_files(path, &registry)
        .context("failed to discover files")?;

    if verbose {
        progress::done(&format!("discovered {} files", files.len()), t);
    }

    let t = Instant::now();
    grapha_core::prepare_plugins(&registry, &project_context)?;
    if let Some(store) = grapha_swift::index_store_path()
        && verbose
    {
        progress::done(&format!("index store: {}", store.display()), t);
    }

    let module_map = grapha_core::discover_modules(&registry, &project_context)?;

    let t = Instant::now();
    let pb = if verbose && files.len() > 1 {
        Some(progress::bar(files.len() as u64, "extracting"))
    } else {
        None
    };

    use rayon::prelude::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let skipped = AtomicUsize::new(0);

    let results: Vec<_> = files
        .par_iter()
        .filter_map(|file| {
            let source = match std::fs::read(file) {
                Ok(s) => s,
                Err(_) => {
                    skipped.fetch_add(1, Ordering::Relaxed);
                    if let Some(ref pb) = pb {
                        pb.inc(1);
                    }
                    return None;
                }
            };
            let file_context = grapha_core::file_context(&project_context, &module_map, file);
            let extraction_result =
                grapha_core::extract_with_registry(&registry, &source, &file_context);

            if let Some(ref pb) = pb {
                pb.inc(1);
            }

            match extraction_result {
                Ok(result) => Some(result),
                Err(e) => {
                    skipped.fetch_add(1, Ordering::Relaxed);
                    if verbose && let Some(ref pb) = pb {
                        pb.suspend(|| {
                            eprintln!("  \x1b[33m!\x1b[0m skipping {}: {e}", file.display())
                        });
                    }
                    None
                }
            }
        })
        .collect();

    let skipped = skipped.load(Ordering::Relaxed);

    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    if verbose {
        let msg = if skipped > 0 {
            format!("extracted {} files ({} skipped)", results.len(), skipped)
        } else {
            format!("extracted {} files", results.len())
        };
        progress::done(&msg, t);
    }

    let cfg = config::load_config(path);
    let mut classifiers = registry.collect_classifiers();
    classifiers.insert(
        0,
        Box::new(classify::toml_rules::TomlRulesClassifier::new(
            &cfg.classifiers,
        )),
    );
    let composite = grapha_core::CompositeClassifier::new(classifiers);
    let preclassified_results: Vec<_> = results
        .into_iter()
        .map(|result| grapha_core::classify_extraction_result(result, &composite))
        .collect();

    let t = Instant::now();
    let merged = grapha_core::merge(preclassified_results);
    if verbose {
        progress::done(
            &format!(
                "merged → {} nodes, {} edges",
                merged.nodes.len(),
                merged.edges.len()
            ),
            t,
        );
    }

    let t = Instant::now();
    let mut graph = grapha_core::classify_graph(&merged, &composite);
    for pass in registry.collect_graph_passes() {
        graph = pass.apply(graph);
    }
    let graph = grapha_core::normalize_graph(graph);
    if verbose {
        let terminal_count = graph
            .nodes
            .iter()
            .filter(|n| matches!(n.role, Some(grapha_core::graph::NodeRole::Terminal { .. })))
            .count();
        let entry_count = graph
            .nodes
            .iter()
            .filter(|n| matches!(n.role, Some(grapha_core::graph::NodeRole::EntryPoint)))
            .count();
        progress::done(
            &format!(
                "classified → {} entries, {} terminals",
                entry_count, terminal_count
            ),
            t,
        );
    }

    Ok(graph)
}

fn load_graph(path: &Path) -> anyhow::Result<grapha_core::graph::Graph> {
    let db_path = path.join(".grapha/grapha.db");
    let s = store::sqlite::SqliteStore::new(db_path);
    s.load()
        .context("no index found — run `grapha index` first")
}

fn store_file_path(format: &str, store_path: &Path) -> anyhow::Result<PathBuf> {
    match format {
        "json" => Ok(store_path.join("graph.json")),
        "sqlite" => Ok(store_path.join("grapha.db")),
        other => Err(anyhow!("unknown store format: {other}")),
    }
}

fn build_store(format: &str, store_path: &Path) -> anyhow::Result<Box<dyn store::Store + Send>> {
    Ok(match format {
        "json" => Box::new(store::json::JsonStore::new(store_path.join("graph.json"))),
        "sqlite" => Box::new(store::sqlite::SqliteStore::new(
            store_path.join("grapha.db"),
        )),
        other => anyhow::bail!("unknown store format: {other}"),
    })
}

fn load_existing_graph(
    format: &str,
    store_path: &Path,
) -> anyhow::Result<Option<grapha_core::graph::Graph>> {
    let store_file = store_file_path(format, store_path)?;
    if !store_file.exists() {
        return Ok(None);
    }

    let store = build_store(format, store_path)?;
    match store.load() {
        Ok(graph) => Ok(Some(graph)),
        Err(error) => {
            eprintln!(
                "  \x1b[33m!\x1b[0m failed to load existing store, falling back to full rebuild: {error}"
            );
            Ok(None)
        }
    }
}

fn kind_label(kind: grapha_core::graph::NodeKind) -> String {
    serde_json::to_string(&kind)
        .unwrap_or_else(|_| format!("{kind:?}"))
        .trim_matches('"')
        .to_string()
}

fn format_ambiguity_error(query: &str, candidates: &[query::QueryCandidate]) -> String {
    let mut message = format!("ambiguous query: {query}\n");
    for candidate in candidates {
        message.push_str(&format!(
            "  - {} [{}] in {} ({})\n",
            candidate.name,
            kind_label(candidate.kind),
            candidate.file,
            candidate.id
        ));
    }
    message.push_str(&format!("hint: {}", query::ambiguity_hint()));
    message
}

fn resolve_query_result<T>(
    result: Result<T, query::QueryResolveError>,
    missing_label: &str,
) -> anyhow::Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(query::QueryResolveError::NotFound { query }) => {
            Err(anyhow!("{missing_label} not found: {query}"))
        }
        Err(query::QueryResolveError::Ambiguous { query, candidates }) => {
            Err(anyhow!(format_ambiguity_error(&query, &candidates)))
        }
    }
}

fn tree_render_options(color: ColorMode) -> render::RenderOptions {
    use std::io::IsTerminal;

    match color {
        ColorMode::Always => render::RenderOptions::color(),
        ColorMode::Never => render::RenderOptions::plain(),
        ColorMode::Auto => {
            if std::io::stdout().is_terminal() {
                render::RenderOptions::color()
            } else {
                render::RenderOptions::plain()
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let render_options = tree_render_options(cli.color);

    match cli.command {
        Commands::Analyze {
            path,
            output,
            filter,
            compact,
        } => {
            // verbose when writing to file (stdout is for JSON)
            let verbose = output.is_some();
            let mut graph = run_pipeline(&path, verbose)?;

            if let Some(ref filter_str) = filter {
                let kinds = filter::parse_filter(filter_str)?;
                graph = filter::filter_graph(graph, &kinds);
            }

            let json = if compact {
                let pruned = compress::prune::prune(graph, false);
                let grouped = compress::group::group(&pruned);
                match &output {
                    Some(_) => serde_json::to_string(&grouped)?,
                    None => serde_json::to_string_pretty(&grouped)?,
                }
            } else {
                match &output {
                    Some(_) => serde_json::to_string(&graph)?,
                    None => serde_json::to_string_pretty(&graph)?,
                }
            };

            match output {
                Some(p) => {
                    std::fs::write(&p, &json)
                        .with_context(|| format!("failed to write {}", p.display()))?;
                    eprintln!("  \x1b[32m✓\x1b[0m wrote {}", p.display());
                }
                None => println!("{json}"),
            }
        }
        Commands::Index {
            path,
            format,
            store_dir,
            full_rebuild,
        } => {
            let total_start = Instant::now();
            let store_path = store_dir.unwrap_or_else(|| path.join(".grapha"));
            let graph = run_pipeline(&path, true)?;

            std::fs::create_dir_all(&store_path)
                .with_context(|| format!("failed to create store dir {}", store_path.display()))?;

            let previous_graph = if full_rebuild {
                None
            } else {
                load_existing_graph(&format, &store_path)?
            };

            // Run store sync, search sync, and localization snapshot build in parallel.
            let search_index_path = store_path.join("search_index");
            let index_root = path.clone();
            let save_result = std::thread::scope(|scope| {
                let save_handle = scope.spawn(|| {
                    let t = Instant::now();
                    let s = build_store(&format, &store_path)?;
                    let stats = if full_rebuild {
                        let stats = store::StoreWriteStats::from_graphs(
                            previous_graph.as_ref(),
                            &graph,
                            delta::SyncMode::FullRebuild,
                        );
                        s.save(&graph)?;
                        stats
                    } else {
                        s.save_incremental(previous_graph.as_ref(), &graph)?
                    };
                    Ok::<_, anyhow::Error>((t.elapsed(), stats))
                });

                let search_handle = scope.spawn(|| {
                    let t = Instant::now();
                    let stats = search::sync_index(
                        previous_graph.as_ref(),
                        &graph,
                        &search_index_path,
                        full_rebuild,
                    )?;
                    Ok::<_, anyhow::Error>((t.elapsed(), stats))
                });

                let localization_handle = scope.spawn(|| {
                    let t = Instant::now();
                    let stats =
                        localization::build_and_save_catalog_snapshot(&index_root, &store_path)?;
                    Ok::<_, anyhow::Error>((t.elapsed(), stats))
                });

                let save = save_handle.join().expect("save thread panicked")?;
                let search = search_handle.join().expect("search thread panicked")?;
                let localization = localization_handle
                    .join()
                    .expect("localization thread panicked")?;
                Ok::<_, anyhow::Error>((save, search, localization))
            });
            let (
                (save_elapsed, save_stats),
                (search_elapsed, search_stats),
                (localize_elapsed, localize_stats),
            ) = save_result?;
            progress::done_elapsed(
                &format!(
                    "saved to {} ({}; {})",
                    store_path.display(),
                    format,
                    save_stats.summary()
                ),
                save_elapsed,
            );
            progress::done_elapsed(
                &format!("built search index ({})", search_stats.summary()),
                search_elapsed,
            );
            progress::done_elapsed(
                &format!(
                    "saved localization snapshot ({} records)",
                    localize_stats.record_count
                ),
                localize_elapsed,
            );
            for warning in &localize_stats.warnings {
                eprintln!(
                    "  \x1b[33m!\x1b[0m skipped invalid localization catalog {}: {}",
                    warning.catalog_file, warning.reason
                );
            }

            progress::summary(&format!(
                "\n  {} nodes, {} edges indexed in {:.1}s",
                graph.nodes.len(),
                graph.edges.len(),
                total_start.elapsed().as_secs_f64(),
            ));
        }
        Commands::Context {
            symbol,
            path,
            format,
        } => {
            let graph = load_graph(&path)?;
            let result =
                resolve_query_result(query::context::query_context(&graph, &symbol), "symbol")?;
            match format {
                QueryOutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
                QueryOutputFormat::Tree => {
                    println!(
                        "{}",
                        render::render_context_with_options(&result, render_options)
                    )
                }
            }
        }
        Commands::Impact {
            symbol,
            depth,
            path,
            format,
        } => {
            let graph = load_graph(&path)?;
            let result = resolve_query_result(
                query::impact::query_impact(&graph, &symbol, depth),
                "symbol",
            )?;
            match format {
                QueryOutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
                QueryOutputFormat::Tree => {
                    println!(
                        "{}",
                        render::render_impact_with_options(&result, render_options)
                    )
                }
            }
        }
        Commands::Changes { scope, path } => {
            let graph = load_graph(&path)?;
            let report = changes::detect_changes(&path, &graph, &scope)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Commands::Search {
            query: q,
            limit,
            path,
        } => {
            let search_index_path = path.join(".grapha/search_index");
            let index = if search_index_path.exists() {
                tantivy::Index::open_in_dir(&search_index_path)?
            } else {
                let graph = load_graph(&path)?;
                eprintln!("  building search index...");
                search::build_index(&graph, &search_index_path)?
            };
            let results = search::search(&index, &q, limit)?;
            println!("{}", serde_json::to_string_pretty(&results)?);
        }
        Commands::Trace {
            entry,
            depth,
            path,
            format,
        } => {
            let graph = load_graph(&path)?;
            let result = resolve_query_result(
                query::trace::query_trace(&graph, &entry, depth),
                "entry point",
            )?;
            match format {
                QueryOutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
                QueryOutputFormat::Tree => {
                    println!(
                        "{}",
                        render::render_trace_with_options(&result, render_options)
                    )
                }
            }
        }
        Commands::Dataflow {
            entry,
            depth,
            path,
            format,
        } => {
            let graph = load_graph(&path)?;
            let result = resolve_query_result(
                query::dataflow::query_dataflow(&graph, &entry, depth),
                "entry point",
            )?;
            match format {
                QueryOutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
                QueryOutputFormat::Tree => {
                    println!(
                        "{}",
                        render::render_dataflow_with_options(&result, render_options)
                    )
                }
            }
        }
        Commands::Reverse {
            symbol,
            path,
            format,
        } => {
            let graph = load_graph(&path)?;
            let result =
                resolve_query_result(query::reverse::query_reverse(&graph, &symbol), "symbol")?;
            match format {
                QueryOutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
                QueryOutputFormat::Tree => {
                    println!(
                        "{}",
                        render::render_reverse_with_options(&result, render_options)
                    )
                }
            }
        }
        Commands::Entries { path, format } => {
            let graph = load_graph(&path)?;
            let result = query::entries::query_entries(&graph);
            match format {
                QueryOutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
                QueryOutputFormat::Tree => {
                    println!(
                        "{}",
                        render::render_entries_with_options(&result, render_options)
                    )
                }
            }
        }
        Commands::Localize {
            symbol,
            path,
            format,
        } => {
            let graph = load_graph(&path)?;
            let catalogs = localization::load_catalog_index(&path)?;
            let result = resolve_query_result(
                query::localize::query_localize(&graph, &catalogs, &symbol),
                "symbol",
            )?;
            match format {
                QueryOutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
                QueryOutputFormat::Tree => {
                    println!(
                        "{}",
                        render::render_localize_with_options(&result, render_options)
                    )
                }
            }
        }
        Commands::Usages {
            key,
            table,
            path,
            format,
        } => {
            let graph = load_graph(&path)?;
            let catalogs = localization::load_catalog_index(&path)?;
            let result = query::usages::query_usages(&graph, &catalogs, &key, table.as_deref());
            match format {
                QueryOutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
                QueryOutputFormat::Tree => {
                    println!(
                        "{}",
                        render::render_usages_with_options(&result, render_options)
                    )
                }
            }
        }
        Commands::Serve { path, port } => {
            let graph = load_graph(&path)?;
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(serve::run(graph, port))?;
        }
    }

    Ok(())
}
