use ui_baseline::variant::{self, Parts};

fn main() -> eframe::Result {
    variant::run("nofallbacks", Parts { fallbacks: false, ..variant::FULL })
}
