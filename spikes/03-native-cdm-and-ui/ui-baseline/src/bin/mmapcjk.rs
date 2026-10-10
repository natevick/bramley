use ui_baseline::variant::{self, Parts};

fn main() -> eframe::Result {
    variant::run("mmapcjk", Parts { fallbacks: false, mmap_cjk: true, ..variant::FULL })
}
