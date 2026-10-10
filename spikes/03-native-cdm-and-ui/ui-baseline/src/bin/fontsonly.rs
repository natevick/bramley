use ui_baseline::variant::{self, Parts};

fn main() -> eframe::Result {
    variant::run("fontsonly", Parts { fonts: true, fallbacks: true, ..variant::BARELIST })
}
