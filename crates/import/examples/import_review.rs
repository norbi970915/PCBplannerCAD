//! Read-only import summary, optionally publishing a new disposable folder.
use pcbplannercad_import::wizard::{self, Request};
use std::path::Path;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: import_review <source> [new-destination] [--board companion]")?;
    let (mut dest, mut board) = (None, None);
    while let Some(arg) = args.next() {
        if arg == "--board" {
            if board.is_some() {
                return Err("companion board supplied twice".into());
            }
            board = Some(args.next().ok_or("--board needs a file path")?);
        } else if dest.is_none() {
            dest = Some(arg);
        } else {
            return Err("unexpected argument".into());
        }
    }
    let plan = wizard::prepare(&Request {
        path,
        board_path: board,
        models: vec![],
    })
    .map_err(std::io::Error::other)?;
    // Ensure generated native files reopen before a disposable copy is published.
    for (file, bytes) in &plan.files {
        use pcbplannercad_core::{DocKind, io};
        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
        if matches!(ext, "arsch" | "arpcb" | "arlib" | "arproj") {
            let text = std::str::from_utf8(bytes)?;
            match ext {
                "arsch" => {
                    io::document_from_str(DocKind::Schematic, text)?;
                }
                "arpcb" => {
                    io::document_from_str(DocKind::Pcb, text)?;
                }
                "arlib" => {
                    io::library_from_str(text)?;
                }
                "arproj" => {
                    io::project_from_str(text)?;
                }
                _ => {}
            }
        }
    }
    println!("{}", serde_json::to_string_pretty(&plan.summary)?);
    if let Some(dest) = dest {
        plan.publish(Path::new(&dest))
            .map_err(std::io::Error::other)?;
    }
    Ok(())
}
