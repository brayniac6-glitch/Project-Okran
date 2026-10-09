//! Print a character appearance file (.bod2): `cargo run -p kenshi-io --example bod2 -- <file>`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let f = kenshi_io::fcs::read(std::path::Path::new(&a[0])).expect("body file");
    for r in &f.records {
        println!("record {:?} {:?}", r.name, r.string_id);
        println!("  bools {:?}", r.bools);
        println!("  ints {:?}", r.ints);
        println!("  floats {:?}", r.floats);
        println!("  strings {:?}", r.strings);
        println!("  vec3 {:?}", r.vec3s);
        println!("  vec4 {:?}", r.vec4s);
        println!("  files {:?}", r.files);
        println!("  refs {:?}", r.references);
    }
}
