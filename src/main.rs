use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand, ValueEnum};

use fmg::config::Config;
use fmg::graph::VaultGraph;
use petgraph::graph::NodeIndex;
use fmg::output::{self, Format};
use fmg::query::{self, TraversalDirection};
use fmg::vault;

#[derive(Parser)]
#[command(name = "fmg", version, about = "Fast, Obsidian-native CLI for traversing [[WikiLink]] relationships in frontmatter markdown vaults")]
struct Cli {
    /// Vault root directory
    #[arg(short = 'w', long = "workspace", default_value = ".")]
    workspace: PathBuf,

    /// Output format
    #[arg(short, long, default_value = "text", global = true)]
    format: FormatArg,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, ValueEnum)]
enum FormatArg {
    Text,
    Json,
    Mermaid,
    Paths,
}

impl From<FormatArg> for Format {
    fn from(arg: FormatArg) -> Self {
        match arg {
            FormatArg::Text => Format::Text,
            FormatArg::Json => Format::Json,
            FormatArg::Mermaid => Format::Mermaid,
            FormatArg::Paths => Format::Paths,
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Vault statistics: fields, types, node counts, health
    Describe,

    /// Multi-hop graph traversal from a node
    Query {
        /// The node title to start from
        node: String,

        /// Max hops (default: from config or 1)
        #[arg(short, long)]
        depth: Option<u32>,

        /// Traversal direction
        #[arg(long, default_value = "both")]
        direction: DirectionArg,

        /// Limit to specific relationship fields (comma-separated)
        #[arg(long, value_delimiter = ',')]
        fields: Option<Vec<String>>,
    },

    /// Pages with no inbound or outbound relationships
    Orphans,

    /// Unresolved [[WikiLinks]] (point to nothing)
    Broken,

    /// Shortest path between two nodes
    Bridge {
        /// Source node title
        a: String,
        /// Target node title
        b: String,
    },

    /// Most-connected nodes (hub discovery)
    Centrality {
        /// Number of results
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },

    /// Exportable neighborhood graph
    Subgraph {
        /// Center node title
        node: String,

        /// Radius in hops
        #[arg(short, long, default_value = "2")]
        depth: u32,
    },
}

#[derive(Clone, ValueEnum)]
enum DirectionArg {
    Up,
    Down,
    Both,
}

impl From<DirectionArg> for TraversalDirection {
    fn from(arg: DirectionArg) -> Self {
        match arg {
            DirectionArg::Up => TraversalDirection::Up,
            DirectionArg::Down => TraversalDirection::Down,
            DirectionArg::Both => TraversalDirection::Both,
        }
    }
}

/// Resolve a node title with fuzzy matching, or exit with helpful error.
fn resolve_node(vg: &VaultGraph, input: &str) -> NodeIndex {
    match vg.fuzzy_find_node(input) {
        Ok(idx) => idx,
        Err(candidates) if candidates.is_empty() => {
            eprintln!("Error: node '{}' not found in vault", input);
            process::exit(1);
        }
        Err(candidates) => {
            eprintln!("Error: '{}' is ambiguous. Did you mean one of:", input);
            for c in &candidates {
                eprintln!("  - {}", c);
            }
            process::exit(1);
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let format: Format = cli.format.into();

    let vault_root = cli.workspace.canonicalize().unwrap_or_else(|e| {
        eprintln!("Error: cannot access workspace '{}': {e}", cli.workspace.display());
        process::exit(1);
    });

    let config = Config::load(&vault_root);
    let pages = vault::scan_vault(&vault_root);

    if pages.is_empty() {
        eprintln!("No markdown files found in '{}'", vault_root.display());
        process::exit(1);
    }

    let vg = VaultGraph::build(&pages, &config);

    match cli.command {
        Commands::Describe => {
            print!("{}", output::format_describe(&vg, format));
        }

        Commands::Query {
            node,
            depth,
            direction,
            fields,
        } => {
            let depth = depth.unwrap_or(config.display.default_depth)
                .min(config.display.max_depth);
            let center = resolve_node(&vg, &node);
            let dir: TraversalDirection = direction.into();
            let result = query::query(
                &vg,
                center,
                depth,
                dir,
                fields.as_deref(),
            );
            print!("{}", output::format_query(&vg, &result, format));
        }

        Commands::Orphans => {
            let orphans = query::orphans(&vg);
            print!("{}", output::format_orphans(&vg, &orphans, format));
        }

        Commands::Broken => {
            let broken = query::broken_links(&vg);
            print!("{}", output::format_broken(&vg, &broken, format));
        }

        Commands::Bridge { a, b } => {
            let from = resolve_node(&vg, &a);
            let to = resolve_node(&vg, &b);
            match query::bridge(&vg, from, to) {
                Some(path) => {
                    print!("{}", output::format_bridge(&vg, from, to, &path, format));
                }
                None => {
                    eprintln!("No path found between '{}' and '{}'", a, b);
                    process::exit(1);
                }
            }
        }

        Commands::Centrality { limit } => {
            let results = query::centrality(&vg, limit);
            print!("{}", output::format_centrality(&vg, &results, format));
        }

        Commands::Subgraph { node, depth } => {
            let depth = depth.min(config.display.max_depth);
            let center = resolve_node(&vg, &node);
            let result = query::query(&vg, center, depth, TraversalDirection::Both, None);
            print!("{}", output::format_query(&vg, &result, format));
        }
    }
}
