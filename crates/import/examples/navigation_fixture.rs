//! Convert a read-only board fixture into a native file for local navigation checks.
use std::{env, fs, path::Path, time::Instant};
use pcbplannercad_core::io;
fn main() {
    let args: Vec<_> = env::args().collect();
    assert_eq!(args.len(), 3, "Usage: navigation_fixture <source-board> <new-native-output>");
    let output = Path::new(&args[2]);
    assert!(!output.exists(), "Refusing to overwrite an existing design");
    let start = Instant::now();
    let board = pcbplannercad_import::structured::import_board(Path::new(&args[1])).expect("board import");
    fs::write(output, io::document_to_string(&board.document).unwrap()).expect("write native fixture");
    println!("Imported {} objects in {:.1} ms", board.document.len(), start.elapsed().as_secs_f64()*1000.0);
}