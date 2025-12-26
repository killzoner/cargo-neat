#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use anyhow::anyhow;
use cargo::CargoResult;
use cargo::core::{Features, SourceId, Workspace};
use cargo::util::context::GlobalContext;
use cargo::util::interning::InternedString;
use cargo::util::toml::read_manifest;
use cargo::util::toml_mut::dependency::Source;
use log::debug;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::{env, vec};
use termtree::Tree;

#[derive(argh::FromArgs)]
#[argh(description = r#"
cargo-neat: Remove unused workspace dependencies

Exit code:
    0:  when no unused dependencies are found
    1:  when at least one unused dependency is found
    2:  on error
"#)]
struct CliArgs {
    /// print version.
    #[argh(switch)]
    version: bool,

    /// allow only workspace dependency (ie "workspace = true")
    #[argh(switch, short = 'm')]
    mandatory_workspace_dependencies: bool,

    /// path to directory that must be scanned.
    #[argh(positional, greedy)]
    path: Option<PathBuf>,
}

// cargo install --path .
fn main() {
    let exit_code = match run() {
        Ok(false) => 0,
        Ok(true) => 1,
        Err(err) => {
            eprintln!("Error: {err}");
            2
        }
    };

    std::process::exit(exit_code);
}

fn run() -> CargoResult<bool> {
    pretty_env_logger::init();

    let args: CliArgs =
        if std::env::var("CARGO").is_ok() && std::env::var("CARGO_PKG_NAME").is_err() {
            argh::cargo_from_env()
        } else {
            argh::from_env()
        };

    if args.version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    let path = match args.path {
        Some(dir) => {
            debug!("Running from location {:?}", dir);

            std::fs::canonicalize(dir)? // we need absolute path
        }
        None => {
            let dir = env::current_dir()?;
            debug!("Running from location {:?}", dir);

            dir
        }
    };

    let gctx = GlobalContext::default()?;
    // Load the workspace from the current directory
    let ws = Workspace::new(&path.join("Cargo.toml"), &gctx)?;

    // Get the root manifest path (root Cargo.toml)
    let root_cargo_toml = ws.root_manifest();

    debug!("Root workspace Cargo.toml: {:?}", root_cargo_toml);

    let workspace = cargo::core::Workspace::new(root_cargo_toml, &gctx)?;
    let workspace_members: Vec<_> = workspace.members().collect();
    let workspace_member_names: Vec<_> = workspace_members.iter().map(|e| e.name()).collect();
    debug!("Workspace members : {:?}", workspace_member_names);

    // read virtual manifest
    let source_id = SourceId::for_manifest_path(root_cargo_toml)?;
    let manifest = read_manifest(root_cargo_toml, source_id, &gctx)?;

    match manifest {
        cargo::core::EitherManifest::Real(_) => Err(anyhow!(
            "Failed to read virtual manifest at `{}`. Maybe you don't use a cargo workspace?",
            root_cargo_toml.display()
        )),
        cargo::core::EitherManifest::Virtual(virtual_manifest) => {
            let workspace_dependencies = virtual_manifest
                .document()
                .get_ref()
                .get("workspace")
                .and_then(|e| e.get_ref().get("dependencies"))
                .and_then(|e| e.get_ref().as_table())
                .map(|e| {
                    e.keys()
                        .map(|e| e.clone().into_inner())
                        .collect::<HashSet<_>>()
                })
                .unwrap_or_default();

            debug!("Workspace dependencies : {:?}", workspace_dependencies);

            let mut unused_workspace_dependencies = workspace_dependencies;
            let mut mandatory_workspace_dependencies_issues: HashMap<InternedString, Vec<String>> =
                HashMap::new();

            for pkg in workspace_members {
                let local_manifest =
                    cargo::util::toml_mut::manifest::LocalManifest::try_new(pkg.manifest_path())?;

                if args.mandatory_workspace_dependencies {
                    let deps_other: Vec<_> = local_manifest
                        .get_dependencies(&workspace, &Features::default())
                        .flat_map(|dep| dep.2.map(|e| (dep.0, e.source)))
                        .filter_map(|dep| dep.1.map(|e| (dep.0, e)))
                        .collect();

                    for (dep, source) in deps_other {
                        if let Source::Registry(_) = source {
                            let values = mandatory_workspace_dependencies_issues
                                .entry(pkg.name())
                                .or_insert(vec![]);
                            values.push(dep);
                        }
                    }
                }

                for dep in pkg.dependencies() {
                    let name = dep.package_name();
                    let name: &str = name.as_ref();
                    unused_workspace_dependencies.remove(name);
                }
            }

            if unused_workspace_dependencies.is_empty()
                && mandatory_workspace_dependencies_issues.is_empty()
            {
                println!("No unused workspace dependencies");

                if args.mandatory_workspace_dependencies {
                    println!("No non workspace dependencies");
                }

                Ok(false)
            } else {
                if !unused_workspace_dependencies.is_empty() {
                    let mut unused_workspace_dependencies: Vec<_> = unused_workspace_dependencies
                        .into_iter()
                        .map(|e| e.to_string())
                        .collect();
                    unused_workspace_dependencies.sort();

                    eprintln!(
                        "{}",
                        tree(
                            InternedString::new("Unused workspace dependencies :"),
                            &[(
                                InternedString::new(
                                    root_cargo_toml
                                        .to_str()
                                        .ok_or(anyhow!("cannot get root workspace"))?
                                ),
                                unused_workspace_dependencies
                            )]
                        )?
                    );
                }

                if !mandatory_workspace_dependencies_issues.is_empty() {
                    let parent_folder = root_cargo_toml
                        .parent()
                        .ok_or(anyhow!("cannot get root workspace folder"))?;

                    let mut mandatory_workspace_dependencies_issues: Vec<_> =
                        mandatory_workspace_dependencies_issues
                            .into_iter()
                            .flat_map(|e| {
                                PathBuf::from(parent_folder)
                                    .join(e.0)
                                    .join("Cargo.toml")
                                    .to_str()
                                    .ok_or(anyhow!("cannot get root workspace folder"))
                                    .map(|res| (InternedString::new(res), e.1))
                            })
                            .collect();
                    mandatory_workspace_dependencies_issues.sort();

                    eprintln!(
                        "{}",
                        tree(
                            InternedString::new("Non workspace dependencies :"),
                            &mandatory_workspace_dependencies_issues
                        )?
                    );
                }

                Ok(true)
            }
        }
    }
}

fn tree(
    root: InternedString,
    issues: &[(InternedString, Vec<String>)],
) -> anyhow::Result<Tree<InternedString>> {
    let mut tree: Tree<InternedString> = Tree::new(root);

    for (pkg, deps) in issues {
        let mut pkg: Tree<InternedString> = Tree::new(InternedString::new(pkg));

        for dep in deps {
            pkg.push(InternedString::new(dep.as_ref()));
        }

        tree.push(pkg);
    }

    Ok(tree)
}
