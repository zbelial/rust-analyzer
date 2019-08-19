use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use ra_db::{
    salsa::{Database, Durability},
    FileId, SourceDatabase,
};
use ra_ide_api::{Analysis, AnalysisChange, AnalysisHost, FilePosition, LineCol};

use crate::Result;

pub(crate) enum Op {
    Highlight { path: PathBuf },
    Complete { path: PathBuf, line: u32, column: u32 },
}

pub(crate) fn run(verbose: bool, path: &Path, op: Op) -> Result<()> {
    let start = Instant::now();
    eprint!("loading: ");
    let (mut host, roots) = ra_batch::load_cargo(path)?;
    let db = host.raw_database();
    eprintln!("{:?}\n", start.elapsed());

    let file_id = {
        let path = match &op {
            Op::Highlight { path } => path,
            Op::Complete { path, .. } => path,
        };
        let path = std::env::current_dir()?.join(path).canonicalize()?;
        roots
            .iter()
            .find_map(|(source_root_id, project_root)| {
                if project_root.is_member() {
                    for (rel_path, file_id) in &db.source_root(*source_root_id).files {
                        let abs_path = rel_path.to_path(project_root.path());
                        if abs_path == path {
                            return Some(*file_id);
                        }
                    }
                }
                None
            })
            .ok_or_else(|| format!("Can't find {:?}", path))?
    };

    match op {
        Op::Highlight { .. } => {
            let res = do_work(&mut host, file_id, |analysis| {
                analysis.diagnostics(file_id).unwrap();
                analysis.highlight_as_html(file_id, false).unwrap()
            });
            if verbose {
                println!("\n{}", res);
            }
        }
        Op::Complete { line, column, .. } => {
            let offset = host
                .analysis()
                .file_line_index(file_id)?
                .offset(LineCol { line, col_utf16: column });
            let file_postion = FilePosition { file_id, offset };

            let res = do_work(&mut host, file_id, |analysis| analysis.completions(file_postion));
            if verbose {
                println!("\n{:#?}", res);
            }
        }
    }
    Ok(())
}

fn do_work<F: Fn(&Analysis) -> T, T>(host: &mut AnalysisHost, file_id: FileId, work: F) -> T {
    {
        let start = Instant::now();
        eprint!("from scratch:   ");
        work(&host.analysis());
        eprintln!("{:?}", start.elapsed());
    }
    {
        let start = Instant::now();
        eprint!("no change:      ");
        work(&host.analysis());
        eprintln!("{:?}", start.elapsed());
    }
    {
        let start = Instant::now();
        eprint!("trivial change: ");
        host.raw_database().salsa_runtime().synthetic_write(Durability::LOW);
        work(&host.analysis());
        eprintln!("{:?}", start.elapsed());
    }
    {
        let start = Instant::now();
        eprint!("comment change: ");
        {
            let mut text = host.analysis().file_text(file_id).unwrap().to_string();
            text.push_str("\n/* Hello world */\n");
            let mut change = AnalysisChange::new();
            change.change_file(file_id, Arc::new(text));
            host.apply_change(change);
        }
        work(&host.analysis());
        eprintln!("{:?}", start.elapsed());
    }
    {
        let start = Instant::now();
        eprint!("const change:   ");
        host.raw_database().salsa_runtime().synthetic_write(Durability::HIGH);
        let res = work(&host.analysis());
        eprintln!("{:?}", start.elapsed());
        res
    }
}
