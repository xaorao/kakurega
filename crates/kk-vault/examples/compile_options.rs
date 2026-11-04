use kk_vault::rusqlite::Connection;

fn main() {
    let c = Connection::open_in_memory().expect("open");
    let mut stmt = c.prepare("PRAGMA compile_options").expect("pragma");
    let rows = stmt.query_map([], |r| r.get::<_, String>(0)).expect("rows");
    for r in rows {
        println!("{}", r.expect("row"));
    }
}
