//! Count Kenshi's gui resources and list a layout's widgets:
//! `cargo run -p kenshi-gui --example resources -- <install> [layout]`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let g = kenshi_gui::Gui::load(std::path::Path::new(&a[0]));
    println!("{} skins, {} templates, {} fonts, {} image sets, {} colours", g.skins.len(), g.templates.len(), g.fonts.len(), g.images.len(), g.colours.len());
    if let Some(l) = a.get(1) {
        fn show(d: &kenshi_gui::WidgetDef, g: &kenshi_gui::Gui, depth: usize) {
            let known = g.skins.contains_key(&d.skin) || g.templates.contains_key(&d.skin);
            println!("{}{} '{}' skin {}{} {:?}", "  ".repeat(depth), d.kind, d.name, d.skin, if known { "" } else { " (MISSING)" }, d.pos);
            for c in &d.children {
                show(c, g, depth + 1);
            }
        }
        for d in g.layout(l) {
            show(&d, &g, 0);
        }
    }
}
